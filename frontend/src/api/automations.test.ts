import { invoke } from "@tauri-apps/api/core";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { automationsApi } from "./automations";
import { backendApiUrl } from "./backend";

const fetchMock = vi.fn();

function automationResponse(overrides: Record<string, unknown> = {}) {
  return {
    id: "automation-1",
    project_id: "project-1",
    name: "Nightly docs",
    status: "draft",
    paused_reason_code: null,
    paused_reason_detail: null,
    goal_prompt: "Keep docs current",
    setup_conversation_id: "conversation-setup-1",
    provider_harness: "codex",
    model_id: "gpt-5.5",
    logical_effort: "high",
    run_mode: "edit",
    base_ref_kind: "project_default",
    base_ref: "",
    base_display_name: "Default branch",
    base_source_pull_request_json: null,
    goal_items_json: "[{\"text\":\"Update docs\"}]",
    chain_mode: "merged_base",
    completion_signal: "pr_merged",
    max_runs: 25,
    max_consecutive_failures: 3,
    first_run_prompt: "Open a docs PR",
    setup_analysis_summary: null,
    created_at: "2026-07-05T00:00:00Z",
    updated_at: "2026-07-05T00:00:01Z",
    ...overrides,
  };
}

function runResponse(overrides: Record<string, unknown> = {}) {
  return {
    id: "automation-run-1",
    automation_id: "automation-1",
    run_index: 1,
    status: "published",
    judge_state: "in_progress",
    judge_lease_expires_at: null,
    conversation_id: "conversation-run-1",
    run_prompt: "Open a docs PR",
    prompt_author: "setup_agent",
    base_ref_kind: "project_default",
    base_ref_used: "main",
    base_from_run_id: null,
    branch_name: "agent/docs",
    pr_number: 593,
    pr_url: "https://github.com/example/repo/pull/593",
    pr_title: "Update docs",
    pr_head_ref_name: "agent/docs",
    pr_base_ref_name: "main",
    pr_merged_at: null,
    merge_commit_sha: null,
    diff_stats_json: null,
    agent_summary: null,
    judge_verdict_json: null,
    judge_model_id: null,
    error_code: null,
    error_detail: null,
    signal_check_failures: 0,
    started_at: "2026-07-05T00:01:00Z",
    finished_at: null,
    created_at: "2026-07-05T00:00:59Z",
    updated_at: "2026-07-05T00:01:00Z",
    ...overrides,
  };
}

function jsonResponse(body: unknown, init: ResponseInit = {}) {
  return new Response(JSON.stringify(body), {
    status: 200,
    headers: { "Content-Type": "application/json" },
    ...init,
  });
}

describe("automationsApi", () => {
  beforeEach(() => {
    vi.mocked(invoke).mockReset();
    fetchMock.mockReset();
    vi.stubGlobal("fetch", fetchMock);
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("lists automations through the wrapped Tauri command and transforms snake_case fields", async () => {
    vi.mocked(invoke).mockResolvedValue([automationResponse()]);

    await expect(
      automationsApi.list({ projectId: "project-1" }),
    ).resolves.toEqual([
      expect.objectContaining({
        id: "automation-1",
        projectId: "project-1",
        setupConversationId: "conversation-setup-1",
        providerHarness: "codex",
        maxRuns: 25,
        maxConsecutiveFailures: 3,
      }),
    ]);

    expect(invoke).toHaveBeenCalledWith("list_automations", {
      input: { projectId: "project-1" },
    });
  });

  it("sends camelCase Tauri command inputs for updates and pause reasons", async () => {
    vi.mocked(invoke).mockResolvedValue(automationResponse({ status: "paused" }));

    await automationsApi.updateSettings({
      id: "automation-1",
      maxRuns: 12,
      maxConsecutiveFailures: 2,
    });
    await automationsApi.pause({
      id: "automation-1",
      reasonCode: "user_paused",
      reasonDetail: "Waiting on release branch",
    });

    expect(invoke).toHaveBeenNthCalledWith(1, "update_automation_settings", {
      input: {
        id: "automation-1",
        maxRuns: 12,
        maxConsecutiveFailures: 2,
      },
    });
    expect(invoke).toHaveBeenNthCalledWith(2, "pause_automation", {
      input: {
        id: "automation-1",
        reasonCode: "user_paused",
        reasonDetail: "Waiting on release branch",
      },
    });
  });

  it("sends remaining control commands with wrapped camelCase inputs", async () => {
    vi.mocked(invoke)
      .mockResolvedValueOnce({
        scheduled: false,
        reason: "automation run-now scheduling is implemented in a later scheduler phase",
      })
      .mockResolvedValueOnce({
        scheduled: false,
        reason: "judge already started",
      })
      .mockResolvedValueOnce(runResponse({ status: "cancelled" }))
      .mockResolvedValueOnce(null);

    await expect(automationsApi.triggerRunNow("automation-1")).resolves.toEqual({
      scheduled: false,
      reason: "automation run-now scheduling is implemented in a later scheduler phase",
    });
    await expect(
      automationsApi.skipJudge({
        id: "automation-1",
        runId: "automation-run-1",
      }),
    ).resolves.toEqual({
      scheduled: false,
      reason: "judge already started",
    });
    await expect(
      automationsApi.cancelRun({
        id: "automation-1",
        runId: "automation-run-1",
      }),
    ).resolves.toEqual(expect.objectContaining({ status: "cancelled" }));
    await expect(automationsApi.delete("automation-1")).resolves.toBeUndefined();

    expect(invoke).toHaveBeenNthCalledWith(1, "trigger_automation_run_now", {
      input: { id: "automation-1" },
    });
    expect(invoke).toHaveBeenNthCalledWith(2, "skip_automation_judge", {
      input: { id: "automation-1", runId: "automation-run-1" },
    });
    expect(invoke).toHaveBeenNthCalledWith(3, "cancel_automation_run", {
      input: { id: "automation-1", runId: "automation-run-1" },
    });
    expect(invoke).toHaveBeenNthCalledWith(4, "delete_automation", {
      input: { id: "automation-1" },
    });
  });

  it("transforms automation detail runs", async () => {
    vi.mocked(invoke).mockResolvedValue({
      automation: automationResponse(),
      runs: [runResponse()],
    });

    await expect(automationsApi.get("automation-1")).resolves.toEqual(
      expect.objectContaining({
        automation: expect.objectContaining({
          baseDisplayName: "Default branch",
          firstRunPrompt: "Open a docs PR",
        }),
        runs: [
          expect.objectContaining({
            automationId: "automation-1",
            runIndex: 1,
            judgeState: "in_progress",
            prNumber: 593,
            signalCheckFailures: 0,
          }),
        ],
      }),
    );
  });

  it("uses server-bound setup-agent HTTP APIs without a client-chosen automation id", async () => {
    fetchMock.mockResolvedValue(jsonResponse(automationResponse({ name: "Updated" })));

    await expect(
      automationsApi.setupAgent.updateAutomation("conversation-setup-1", {
        name: "Updated",
        maxRuns: 9,
      }),
    ).resolves.toEqual(
      expect.objectContaining({
        name: "Updated",
        maxRuns: 25,
      }),
    );

    expect(fetchMock).toHaveBeenCalledWith(backendApiUrl("update_automation"), {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
        "x-ralphx-caller-session-id": "conversation-setup-1",
      },
      body: JSON.stringify({
        name: "Updated",
        max_runs: 9,
      }),
    });
    const [, init] = fetchMock.mock.calls[0]!;
    expect(JSON.parse(String((init as RequestInit).body))).not.toHaveProperty("id");
  });

  it("uses setup-agent finalize with only injected caller identity", async () => {
    fetchMock.mockResolvedValue(
      jsonResponse(automationResponse({ status: "active" })),
    );

    await expect(
      automationsApi.setupAgent.finalizeAutomation("conversation-setup-1"),
    ).resolves.toEqual(expect.objectContaining({ status: "active" }));

    expect(fetchMock).toHaveBeenCalledWith(backendApiUrl("finalize_automation"), {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
        "x-ralphx-caller-session-id": "conversation-setup-1",
      },
    });
  });

  it("reports HTTP errors with backend detail when setup-agent calls fail", async () => {
    fetchMock.mockResolvedValue(
      jsonResponse(
        { error: "Caller conversation is not bound to an automation" },
        { status: 403, statusText: "Forbidden" },
      ),
    );

    await expect(
      automationsApi.setupAgent.getAutomation("conversation-setup-1"),
    ).rejects.toThrow(
      "Automation request failed: 403 Forbidden: Caller conversation is not bound to an automation",
    );
  });
});
