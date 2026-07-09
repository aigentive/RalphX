import { describe, expect, it } from "vitest";

import type { Automation, AutomationRun } from "@/api/automations";
import {
  describeAutomationRunPrState,
  getAutomationRunView,
  isAutomationRunComposerReadOnly,
  latestRunHoldsGoalAuthority,
} from "./automationRunView";

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

describe("automationRunView", () => {
  it("mirrors goal authority independently from open-run semantics", () => {
    expect(
      latestRunHoldsGoalAuthority(run({ status: "merged", judgeState: "done" })),
    ).toBe(true);
    expect(
      latestRunHoldsGoalAuthority(run({ status: "agent_failed", judgeState: "failed" })),
    ).toBe(false);
    expect(
      latestRunHoldsGoalAuthority(run({ status: "cancelled", judgeState: "none" })),
    ).toBe(false);
  });

  it("keeps parked plan approval runs composer-editable while authority is held", () => {
    const parked = run({ status: "awaiting_plan_approval" });

    expect(latestRunHoldsGoalAuthority(parked)).toBe(true);
    expect(isAutomationRunComposerReadOnly(parked)).toBe(false);
    expect(
      isAutomationRunComposerReadOnly(run({ status: "running", judgeState: "none" })),
    ).toBe(true);
    expect(
      isAutomationRunComposerReadOnly(run({ status: "merged", judgeState: "skipped" })),
    ).toBe(false);
  });

  it("collects the core run view flags from one selector", () => {
    const view = getAutomationRunView(
      automation(),
      run({ status: "running", planPhase: true }),
    );

    expect(view).toEqual(
      expect.objectContaining({
        isOpen: true,
        isCancellable: true,
        holdsGoalAuthority: true,
        composerReadOnly: true,
        stageLabel: "Run 1 planning",
      }),
    );
  });

  it("uses status-neutral PR copy for cancelled runs", () => {
    expect(
      describeAutomationRunPrState(
        run({ status: "cancelled", judgeState: "none", prNumber: 593 }),
      ),
    ).toBe("PR #593 on cancelled run");
    expect(
      describeAutomationRunPrState(run({ status: "published", prNumber: 593 })),
    ).toBe("Current PR #593");
  });
});
