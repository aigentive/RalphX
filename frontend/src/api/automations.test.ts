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
    spec_artifact_id: null,
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
    plan_approval_mode: "manual",
    pr_merge_mode: "manual",
    plan_deep_verification: false,
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
    plan_judge_state: "none",
    plan_revision_round: 0,
    plan_revision_pending: false,
    plan_phase: false,
    plan_artifact_id: null,
    plan_approved_by: null,
    plan_approved_artifact_version: null,
    plan_approved_at: null,
    conversation_id: "conversation-run-1",
    run_prompt: "Open a docs PR",
    prompt_author: "setup_agent",
    base_ref_kind: "project_default",
    base_ref_used: "main",
    base_from_run_id: null,
    goal_item_id: null,
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

function usageResponse(overrides: Record<string, unknown> = {}) {
  return {
    input_tokens: 120,
    output_tokens: 30,
    cache_creation_tokens: 7,
    cache_read_tokens: 9,
    estimated_usd: 0.04,
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
        planApprovalMode: "manual",
        prMergeMode: "manual",
        planDeepVerification: false,
        maxRuns: 25,
        maxConsecutiveFailures: 3,
      }),
    ]);

    expect(invoke).toHaveBeenCalledWith("list_automations", {
      input: { projectId: "project-1" },
    });
  });

  it("passes a linked selected base when creating an automation draft", async () => {
    vi.mocked(invoke).mockResolvedValue({
      automation: automationResponse(),
      setup_conversation_id: "conversation-setup-1",
    });

    await automationsApi.createDraft({
      projectId: "project-1",
      name: "Branch-aware automation",
      base: {
        kind: "local_branch",
        branchMode: "linked",
        ref: "feature/linked-automation",
        displayName: "feature/linked-automation",
        sourcePullRequest: {
          number: 42,
          url: "https://github.com/example/repo/pull/42",
          title: "Linked automation base",
          headRefName: "feature/linked-automation",
          baseRefName: "release",
          headRefOid: "abc123",
        },
      },
    });

    expect(invoke).toHaveBeenCalledWith("create_automation_draft", {
      input: {
        projectId: "project-1",
        name: "Branch-aware automation",
        baseRefKind: "local_branch",
        baseBranchMode: "linked",
        baseRef: "feature/linked-automation",
        baseDisplayName: "feature/linked-automation",
        baseSourcePullRequest: {
          number: 42,
          url: "https://github.com/example/repo/pull/42",
          title: "Linked automation base",
          headRefName: "feature/linked-automation",
          baseRefName: "release",
          headRefOid: "abc123",
        },
      },
    });
  });

  it("sends camelCase Tauri command inputs for updates and pause reasons", async () => {
    vi.mocked(invoke).mockResolvedValue(automationResponse({ status: "paused" }));

    await automationsApi.updateSettings({
      id: "automation-1",
      maxRuns: 12,
      maxConsecutiveFailures: 2,
      planApprovalMode: "automatic",
      prMergeMode: "automatic",
      planDeepVerification: true,
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
        planApprovalMode: "automatic",
        prMergeMode: "automatic",
        planDeepVerification: true,
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
      .mockResolvedValueOnce(null)
      .mockResolvedValueOnce(null)
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
    ).resolves.toEqual(
      expect.objectContaining({ status: "cancelled", goalItemId: null }),
    );
    await expect(
      automationsApi.deleteRun({
        id: "automation-1",
        runId: "automation-run-1",
      }),
    ).resolves.toBeUndefined();
    await expect(
      automationsApi.resumeRun({
        id: "automation-1",
        runId: "automation-run-1",
      }),
    ).resolves.toBeUndefined();
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
    expect(invoke).toHaveBeenNthCalledWith(4, "delete_automation_run", {
      input: { id: "automation-1", runId: "automation-run-1" },
    });
    expect(invoke).toHaveBeenNthCalledWith(5, "resume_automation_run", {
      input: { id: "automation-1", runId: "automation-run-1" },
    });
    expect(invoke).toHaveBeenNthCalledWith(6, "delete_automation", {
      input: { id: "automation-1" },
    });
  });

  it("uses dedicated fresh-run restart and judge retry commands", async () => {
    vi.mocked(invoke)
      .mockResolvedValueOnce({ scheduled: true, reason: null })
      .mockResolvedValueOnce({ scheduled: true, reason: null })
      .mockResolvedValueOnce({ scheduled: false, reason: "plan judge already running" });

    await expect(automationsApi.restart("automation-1")).resolves.toEqual({
      scheduled: true,
      reason: null,
    });
    await expect(automationsApi.retryJudge("automation-1")).resolves.toEqual({
      scheduled: true,
      reason: null,
    });
    await expect(automationsApi.retryPlanJudge("automation-1")).resolves.toEqual({
      scheduled: false,
      reason: "plan judge already running",
    });

    expect(invoke).toHaveBeenNthCalledWith(1, "restart_automation", {
      input: { id: "automation-1" },
    });
    expect(invoke).toHaveBeenNthCalledWith(2, "retry_automation_judge", {
      input: { id: "automation-1" },
    });
    expect(invoke).toHaveBeenNthCalledWith(3, "retry_automation_plan_judge", {
      input: { id: "automation-1" },
    });
  });

  it("transforms automation detail runs", async () => {
    vi.mocked(invoke).mockResolvedValue({
      automation: automationResponse({
        plan_approval_mode: "automatic",
        pr_merge_mode: "automatic",
        plan_deep_verification: true,
      }),
      runs: [
        runResponse({
          status: "awaiting_plan_approval",
          plan_judge_state: "in_progress",
          plan_revision_round: 2,
          plan_revision_pending: true,
          plan_phase: true,
          plan_artifact_id: "plan-artifact-1",
          plan_approved_by: "judge",
          plan_approved_artifact_version: 3,
          plan_approved_at: "2026-07-09T13:45:00Z",
          goal_item_id: "phase-1",
        }),
      ],
      usage: usageResponse(),
      pipeline: {
        deliverable: "task_graph",
        status: "executing",
        ideation_session_id: "session-1",
        plan_artifact_id: "plan-artifact-1",
        proposal_count: 2,
        task_total: 2,
        task_merged: 1,
        task_terminal: 1,
        tasks: [
          {
            id: "task-2",
            title: "Build UI",
            status: "ready",
            blocked_by: ["task-1"],
          },
        ],
      },
    });

    await expect(automationsApi.get("automation-1")).resolves.toEqual(
      expect.objectContaining({
        automation: expect.objectContaining({
          baseDisplayName: "Default branch",
          firstRunPrompt: "Open a docs PR",
          planApprovalMode: "automatic",
          prMergeMode: "automatic",
          planDeepVerification: true,
        }),
        runs: [
          expect.objectContaining({
            automationId: "automation-1",
            runIndex: 1,
            status: "awaiting_plan_approval",
            judgeState: "in_progress",
            planJudgeState: "in_progress",
            planRevisionRound: 2,
            planRevisionPending: true,
            planPhase: true,
            planArtifactId: "plan-artifact-1",
            planApprovedBy: "judge",
            planApprovedArtifactVersion: 3,
            planApprovedAt: "2026-07-09T13:45:00Z",
            goalItemId: "phase-1",
            prNumber: 593,
            signalCheckFailures: 0,
          }),
        ],
        usage: expect.objectContaining({
          inputTokens: 120,
          outputTokens: 30,
          cacheCreationTokens: 7,
          cacheReadTokens: 9,
          estimatedUsd: 0.04,
        }),
        pipeline: {
          deliverable: "task_graph",
          status: "executing",
          ideationSessionId: "session-1",
          planArtifactId: "plan-artifact-1",
          proposalCount: 2,
          taskTotal: 2,
          taskMerged: 1,
          taskTerminal: 1,
          tasks: [
            {
              id: "task-2",
              title: "Build UI",
              status: "ready",
              blockedBy: ["task-1"],
            },
          ],
        },
      }),
    );
  });

  it("uses server-bound setup-agent HTTP APIs without a client-chosen automation id", async () => {
    fetchMock.mockResolvedValue(jsonResponse(automationResponse({ name: "Updated" })));

    await expect(
      automationsApi.setupAgent.updateAutomation("conversation-setup-1", {
        name: "Updated",
        maxRuns: 9,
        providerHarness: "codex",
        modelId: "gpt-5.5",
        logicalEffort: "xhigh",
        runMode: "plan",
        goalItemsJson:
          '[{"id":"phase-1","title":"Build shared context model","status":"pending"}]',
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
        provider_harness: "codex",
        model_id: "gpt-5.5",
        logical_effort: "xhigh",
        run_mode: "plan",
        goal_items_json:
          '[{"id":"phase-1","title":"Build shared context model","status":"pending"}]',
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
