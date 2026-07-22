import { describe, expect, it } from "vitest";

import type {
  Automation,
  AutomationJudgeState,
  AutomationPlanJudgeState,
  AutomationRun,
  AutomationRunStatus,
} from "@/api/automations";
import { getRunCardBadges } from "./automationRunBadges";
import { AUTOMATION_RUN_STATUS_LABELS } from "./automationRunView";

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
    runIndex: 8,
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

describe("getRunCardBadges", () => {
  it("renders exactly one live accent badge for a plain running run", () => {
    const badges = getRunCardBadges(automation(), run());
    expect(badges).toEqual([
      { key: "status", label: "Running", tone: "accent", live: true },
    ]);
  });

  it("kills the historical Running + 'Run N in progress' duplication", () => {
    const badges = getRunCardBadges(automation(), run({ status: "running" }));
    expect(
      badges.some((badge) => badge.label === "Run 8 in progress"),
    ).toBe(false);
  });

  it("keeps informative stages like waiting for PR merge", () => {
    const badges = getRunCardBadges(
      automation(),
      run({ status: "published", prNumber: 840 }),
    );
    expect(badges.map((badge) => badge.label)).toEqual([
      "Published",
      "Waiting for PR #840 to merge",
    ]);
  });

  it("compresses 'Run N planning' to 'Planning'", () => {
    const badges = getRunCardBadges(
      automation(),
      run({ status: "running", planPhase: true }),
    );
    expect(badges.map((badge) => badge.label)).toEqual(["Running", "Planning"]);
  });

  it("collapses a settled merged run to only the green status badge", () => {
    const badges = getRunCardBadges(
      automation(),
      run({ status: "merged", judgeState: "done", finishedAt: "2026-07-05T01:00:00Z" }),
    );
    expect(badges).toEqual([
      { key: "status", label: "Merged", tone: "success", live: false },
    ]);
  });

  it("shows a failed judge badge with error tone on a failed run", () => {
    const badges = getRunCardBadges(
      automation(),
      run({ status: "agent_failed", judgeState: "failed" }),
    );
    expect(badges).toEqual([
      { key: "status", label: "Agent failed", tone: "error", live: false },
      { key: "judge", label: "Terminal judge failed", tone: "error", live: false },
    ]);
  });

  it("uses a concise warning stage when the automation is paused", () => {
    const badges = getRunCardBadges(
      automation({ status: "paused", pausedReasonCode: "workspace_review_blocked" }),
      run({ status: "published" }),
    );

    expect(badges).toEqual([
      { key: "status", label: "Published", tone: "success", live: false },
      { key: "stage", label: "Paused", tone: "warning", live: false },
    ]);
    expect(badges.some((badge) => badge.label.includes("workspace_review_blocked"))).toBe(false);
  });

  it("drops a judge badge that would restate the status label", () => {
    const badges = getRunCardBadges(
      automation({ planApprovalMode: "automatic" }),
      run({ status: "awaiting_plan_approval", planJudgeState: "in_progress" }),
    );
    expect(badges.map((badge) => badge.label)).toEqual(["Plan judge running"]);
  });

  it("never echoes the automation's current stage onto settled historical runs", () => {
    const badges = getRunCardBadges(
      automation(),
      run({ status: "pr_closed", judgeState: "done" }),
    );
    expect(
      badges.some((badge) => badge.label === "Scheduling next run"),
    ).toBe(false);
  });

  it("never yields duplicate labels across the full status × judge matrix", () => {
    const statuses = Object.keys(
      AUTOMATION_RUN_STATUS_LABELS,
    ) as AutomationRunStatus[];
    const judgeStates: AutomationJudgeState[] = [
      "none",
      "in_progress",
      "done",
      "failed",
      "skipped",
    ];
    const planJudgeStates: AutomationPlanJudgeState[] = [
      "none",
      "in_progress",
      "done",
      "failed",
    ];

    for (const status of statuses) {
      for (const judgeState of judgeStates) {
        for (const planJudgeState of planJudgeStates) {
          for (const planPhase of [false, true]) {
            const badges = getRunCardBadges(
              automation(),
              run({ status, judgeState, planJudgeState, planPhase, prNumber: 593 }),
            );
            const labels = badges.map((badge) => badge.label);
            expect(new Set(labels).size).toBe(labels.length);
            expect(labels.filter((label) => label.length === 0)).toEqual([]);
            expect(
              labels.some((label) => label === `Run 8 in progress`),
            ).toBe(false);
            expect(
              badges.filter((badge) => badge.key === "status"),
            ).toHaveLength(1);
          }
        }
      }
    }
  });
});
