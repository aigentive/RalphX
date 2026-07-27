import { invoke } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { startupApi } from "./startup";

const startupSnapshot = {
  boot_id: "boot-1",
  attempt_id: 2,
  stage: "runtime_ready",
  started_at: "2026-07-24T09:00:00Z",
  stage_started_at: "2026-07-24T09:00:01Z",
  completed_at: null,
  app_state_ready: true,
  runtime_ready: true,
  background_complete: false,
  retry_allowed: false,
  progress: { completed_units: 2, total_units: 4 },
  message_code: "runtime_ready",
  failure_code: null,
  diagnostic_summary: null,
};

describe("startupApi", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("validates and transforms the authoritative startup snapshot", async () => {
    vi.mocked(invoke).mockResolvedValue(startupSnapshot);

    await expect(startupApi.getStatus()).resolves.toEqual({
      bootId: "boot-1",
      attemptId: 2,
      stage: "runtime_ready",
      startedAt: "2026-07-24T09:00:00Z",
      stageStartedAt: "2026-07-24T09:00:01Z",
      completedAt: null,
      appStateReady: true,
      runtimeReady: true,
      backgroundComplete: false,
      retryAllowed: false,
      progress: { completedUnits: 2, totalUnits: 4 },
      messageCode: "runtime_ready",
      failureCode: null,
      diagnosticSummary: null,
    });
    expect(invoke).toHaveBeenCalledWith("get_startup_status", {});
  });

  it("reports a shell-paint milestone with current boot identity", async () => {
    vi.mocked(invoke).mockResolvedValue(null);

    await expect(
      startupApi.reportFrontendMilestone({
        bootId: "boot-1",
        attemptId: 2,
        milestone: "shell_painted",
      }),
    ).resolves.toBeUndefined();

    expect(invoke).toHaveBeenCalledWith("report_startup_frontend_milestone", {
      input: {
        bootId: "boot-1",
        attemptId: 2,
        milestone: "shell_painted",
      },
    });
  });

  it("uses a no-path log command and validates redacted diagnostics", async () => {
    vi.mocked(invoke)
      .mockResolvedValueOnce(null)
      .mockResolvedValueOnce({
        attempt_id: 2,
        stage: "failed",
        message_code: "startup_failed",
        failure_code: "app_state_construction",
        can_retry: true,
      });

    await startupApi.openLogs();
    await expect(startupApi.getDiagnostics()).resolves.toEqual({
      attemptId: 2,
      stage: "failed",
      messageCode: "startup_failed",
      failureCode: "app_state_construction",
      canRetry: true,
    });

    expect(invoke).toHaveBeenNthCalledWith(1, "open_startup_logs", {});
    expect(invoke).toHaveBeenNthCalledWith(2, "get_startup_diagnostics", {});
  });

  it("rejects diagnostics with unapproved fields", async () => {
    vi.mocked(invoke).mockResolvedValue({
      attempt_id: 2,
      stage: "failed",
      message_code: "startup_failed",
      failure_code: null,
      can_retry: false,
      diagnostic_summary: "secret",
    });

    await expect(startupApi.getDiagnostics()).rejects.toThrow();
  });

  it("keeps malformed snapshots outside the UI boundary", async () => {
    vi.mocked(invoke).mockResolvedValue({ ...startupSnapshot, runtime_ready: "yes" });

    await expect(startupApi.getStatus()).rejects.toThrow();
  });
});
