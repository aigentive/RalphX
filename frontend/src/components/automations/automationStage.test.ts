import { describe, expect, it } from "vitest";

import type { Automation, AutomationRun } from "@/api/automations";
import {
  describeAutomationStage,
  describeRunFailure,
  isOpenAutomationRun,
  latestRun,
} from "./automationStage";

function automation(overrides: Partial<Automation> = {}): Automation {
  const now = "2026-07-05T00:00:00Z";
  return {
    id: "automation-1",
    projectId: "project-1",
    name: "Ship migration loop",
    status: "active",
    pausedReasonCode: null,
    pausedReasonDetail: null,
    goalPrompt: "Keep landing migration tasks.",
    setupConversationId: "conversation-1",
    specArtifactId: null,
    providerHarness: "codex",
    modelId: "gpt-5.4",
    logicalEffort: "high",
    runMode: "edit",
    baseRefKind: "project_default",
    baseRef: "main",
    baseDisplayName: "main",
    baseSourcePullRequestJson: null,
    goalItemsJson: null,
    chainMode: "merged_base",
    completionSignal: "pr_merged",
    planApprovalMode: "manual",
    prMergeMode: "manual",
    planDeepVerification: false,
    maxRuns: 25,
    maxConsecutiveFailures: 3,
    firstRunPrompt: null,
    setupAnalysisSummary: null,
    createdAt: now,
    updatedAt: now,
    ...overrides,
  };
}

function run(overrides: Partial<AutomationRun> = {}): AutomationRun {
  const now = "2026-07-05T00:00:00Z";
  return {
    id: "run-1",
    automationId: "automation-1",
    runIndex: 1,
    status: "running",
    judgeState: "none",
    judgeLeaseExpiresAt: null,
    planJudgeState: "none",
    planRevisionRound: 0,
    planRevisionPending: false,
    planPhase: false,
    planArtifactId: null,
    planApprovedBy: null,
    planApprovedArtifactVersion: null,
    planApprovedAt: null,
    conversationId: "conversation-1",
    runPrompt: "Continue the automation.",
    promptAuthor: "judge",
    baseRefKind: "project_default",
    baseRefUsed: "main",
    baseFromRunId: null,
    goalItemId: null,
    branchName: "ralphx/test",
    prNumber: null,
    prUrl: null,
    prTitle: null,
    prHeadRefName: null,
    prBaseRefName: "main",
    prMergedAt: null,
    mergeCommitSha: null,
    diffStatsJson: null,
    agentSummary: null,
    judgeVerdictJson: null,
    judgeModelId: null,
    errorCode: null,
    errorDetail: null,
    signalCheckFailures: 0,
    startedAt: now,
    finishedAt: null,
    createdAt: now,
    updatedAt: now,
    ...overrides,
  };
}

describe("describeAutomationStage", () => {
  it("describes draft automations", () => {
    expect(describeAutomationStage(automation({ status: "draft" }), null)).toBe("Draft setup");
  });

  it("describes paused automations with and without a reason code", () => {
    expect(
      describeAutomationStage(
        automation({ status: "paused", pausedReasonCode: "judge_stopped_unmet" }),
        null,
      ),
    ).toBe("Judge stopped — goal unmet");
    expect(
      describeAutomationStage(
        automation({ status: "paused", pausedReasonCode: "release_gate" }),
        null,
      ),
    ).toBe("Release gate");
    expect(
      describeAutomationStage(automation({ status: "paused", pausedReasonCode: null }), null),
    ).toBe("Paused");
  });

  it("describes completed and stopped automations", () => {
    expect(describeAutomationStage(automation({ status: "completed" }), null)).toBe("Goal completed");
    expect(describeAutomationStage(automation({ status: "stopped" }), null)).toBe("Stopped");
  });

  it("describes an active automation without a run", () => {
    expect(describeAutomationStage(automation(), null)).toBe("Waiting for first run");
  });

  it("describes judging and judge-failed runs", () => {
    expect(
      describeAutomationStage(automation(), run({ judgeState: "in_progress" })),
    ).toBe("Terminal judge running");
    expect(
      describeAutomationStage(automation(), run({ judgeState: "failed" })),
    ).toBe("Terminal judge failed");
  });

  it("describes an in-progress run", () => {
    expect(
      describeAutomationStage(automation(), run({ runIndex: 4, status: "running" })),
    ).toBe("Run 4 in progress");
    expect(
      describeAutomationStage(
        automation(),
        run({ runIndex: 4, status: "running", planPhase: true }),
      ),
    ).toBe("Run 4 planning");
  });

  it("describes parked plan-approval runs before judge fallbacks", () => {
    expect(
      describeAutomationStage(
        automation(),
        run({
          status: "awaiting_plan_approval",
          planJudgeState: "in_progress",
          judgeState: "none",
        }),
      ),
    ).toBe("Plan judge running");
    expect(
      describeAutomationStage(
        automation(),
        run({
          status: "awaiting_plan_approval",
          planApprovedAt: "2026-07-09T13:45:00Z",
          planJudgeState: "none",
          judgeState: "none",
        }),
      ),
    ).toBe("Approved — resuming");
    expect(
      describeAutomationStage(
        automation(),
        run({
          status: "awaiting_plan_approval",
          planJudgeState: "none",
          judgeState: "none",
        }),
      ),
    ).toBe("Awaiting plan approval");
  });

  it("does not describe cancelled runs as waiting for a judge", () => {
    expect(
      describeAutomationStage(automation(), run({ status: "cancelled", judgeState: "none" })),
    ).toBe("Cancelled");
  });

  it("describes published runs waiting for a PR merge", () => {
    expect(
      describeAutomationStage(automation(), run({ status: "published", prNumber: 593 })),
    ).toBe("Waiting for PR #593 to merge");
    expect(
      describeAutomationStage(automation(), run({ status: "published", prNumber: null })),
    ).toBe("Waiting for PR merge");
  });

  it("describes merged runs awaiting or past the judge", () => {
    expect(
      describeAutomationStage(automation(), run({ status: "merged", judgeState: "none" })),
    ).toBe("Terminal judge pending");
    expect(
      describeAutomationStage(automation(), run({ status: "merged", judgeState: "done" })),
    ).toBe("Scheduling next run");
  });
});

describe("describeRunFailure", () => {
  it("returns null for a healthy run and a missing run", () => {
    expect(describeRunFailure(run({ status: "running" }))).toBeNull();
    expect(describeRunFailure(run({ status: "published", judgeState: "none" }))).toBeNull();
    expect(describeRunFailure(null)).toBeNull();
  });

  it("prefers the error detail when present", () => {
    expect(
      describeRunFailure(
        run({ status: "agent_failed", errorDetail: "Sandbox exited with code 137" }),
      ),
    ).toBe("Sandbox exited with code 137");
  });

  it("maps known error codes to human labels", () => {
    expect(describeRunFailure(run({ status: "agent_failed", errorCode: "no_changes" }))).toBe(
      "No changes to publish",
    );
    expect(describeRunFailure(run({ status: "agent_failed", errorCode: "publish_failed" }))).toBe(
      "Publish failed",
    );
    expect(describeRunFailure(run({ status: "agent_failed", errorCode: "timeout" }))).toBe(
      "Run timed out",
    );
    expect(describeRunFailure(run({ status: "agent_failed", errorCode: "agent_failed" }))).toBe(
      "Agent run failed",
    );
  });

  it("falls back to the raw error code when unknown", () => {
    expect(describeRunFailure(run({ status: "agent_failed", errorCode: "weird_code" }))).toBe(
      "weird_code",
    );
  });

  it("describes agent_failed, pr_closed and judge failures without error metadata", () => {
    expect(describeRunFailure(run({ status: "agent_failed" }))).toBe("Agent run failed");
    expect(describeRunFailure(run({ status: "pr_closed" }))).toBe("PR closed without merging");
    expect(describeRunFailure(run({ status: "merged", judgeState: "failed" }))).toBe("Judge failed");
  });
});

describe("isOpenAutomationRun", () => {
  it("returns false for a missing run", () => {
    expect(isOpenAutomationRun(null)).toBe(false);
  });

  it("treats in-flight run statuses as open", () => {
    for (const status of [
      "pending",
      "provisioning",
      "running",
      "awaiting_plan_approval",
      "published",
    ] as const) {
      expect(isOpenAutomationRun(run({ status, judgeState: "none" }))).toBe(true);
    }
  });

  it("keeps signal-terminal runs open until the judge settles", () => {
    expect(isOpenAutomationRun(run({ status: "merged", judgeState: "none" }))).toBe(true);
    expect(isOpenAutomationRun(run({ status: "merged", judgeState: "in_progress" }))).toBe(true);
    expect(isOpenAutomationRun(run({ status: "agent_failed", judgeState: "failed" }))).toBe(true);
    expect(isOpenAutomationRun(run({ status: "pr_closed", judgeState: "none" }))).toBe(true);
  });

  it("treats settled runs as closed", () => {
    expect(isOpenAutomationRun(run({ status: "merged", judgeState: "done" }))).toBe(false);
    expect(isOpenAutomationRun(run({ status: "merged", judgeState: "skipped" }))).toBe(false);
    expect(isOpenAutomationRun(run({ status: "cancelled", judgeState: "none" }))).toBe(false);
  });
});

describe("latestRun", () => {
  it("returns null when there are no runs", () => {
    expect(latestRun([])).toBeNull();
  });

  it("picks the run with the highest run index", () => {
    const picked = latestRun([
      run({ id: "a", runIndex: 1 }),
      run({ id: "c", runIndex: 3 }),
      run({ id: "b", runIndex: 2 }),
    ]);
    expect(picked?.id).toBe("c");
  });
});
