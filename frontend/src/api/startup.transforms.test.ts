import { describe, expect, it } from "vitest";

import {
  StartupDiagnosticsSchema,
  StartupStatusSchema,
} from "./startup.schemas";
import {
  transformStartupDiagnostics,
  transformStartupStatus,
} from "./startup.transforms";

describe("transformStartupStatus", () => {
  it("converts optional snake-case status fields without leaking wire names", () => {
    const raw = StartupStatusSchema.parse({
      boot_id: "boot-1",
      attempt_id: 1,
      stage: "migrating",
      started_at: "2026-07-24T09:00:00Z",
      stage_started_at: "2026-07-24T09:00:01Z",
      app_state_ready: false,
      runtime_ready: false,
      background_complete: false,
      retry_allowed: false,
      message_code: "migrating_workspace_data",
    });

    expect(transformStartupStatus(raw)).toEqual({
      bootId: "boot-1",
      attemptId: 1,
      stage: "migrating",
      startedAt: "2026-07-24T09:00:00Z",
      stageStartedAt: "2026-07-24T09:00:01Z",
      completedAt: null,
      appStateReady: false,
      runtimeReady: false,
      backgroundComplete: false,
      retryAllowed: false,
      progress: null,
      messageCode: "migrating_workspace_data",
      failureCode: null,
      diagnosticSummary: null,
    });
  });

  it("transforms only the redacted diagnostics contract", () => {
    const raw = StartupDiagnosticsSchema.parse({
      attempt_id: 3,
      stage: "failed",
      message_code: "startup_failed",
      failure_code: "local_runtime_bind",
      can_retry: false,
    });

    expect(transformStartupDiagnostics(raw)).toEqual({
      attemptId: 3,
      stage: "failed",
      messageCode: "startup_failed",
      failureCode: "local_runtime_bind",
      canRetry: false,
    });
  });
});
