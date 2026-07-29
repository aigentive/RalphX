import { describe, expect, it } from "vitest";

import type { AutomationRun } from "@/api/automations";

import { summarizeAutomationRuns } from "./automationRunsSummary";

function run(overrides: Partial<AutomationRun> = {}): AutomationRun {
  const now = "2026-07-22T00:00:00Z";
  return {
    id: "run-1",
    automationId: "automation-1",
    runIndex: 1,
    status: "merged",
    judgeState: "done",
    judgeLeaseExpiresAt: null,
    planJudgeState: "none",
    planRevisionRound: 0,
    planRevisionPending: false,
    planPhase: false,
    planArtifactId: null,
    planBlueprintArtifactId: null,
    parkedPlanArtifactId: null,
    parkedPlanBlueprintArtifactId: null,
    planApprovedBy: null,
    planApprovedArtifactVersion: null,
    planApprovedAt: null,
    conversationId: null,
    runPrompt: "Continue.",
    promptAuthor: "judge",
    baseRefKind: "project_default",
    baseRefUsed: "main",
    baseFromRunId: null,
    goalItemId: null,
    branchName: "ralphx/run-1",
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
    finishedAt: now,
    createdAt: now,
    updatedAt: now,
    ...overrides,
  };
}

describe("summarizeAutomationRuns", () => {
  it("counts every outcome bucket from real run statuses", () => {
    const summary = summarizeAutomationRuns([
      run({ id: "a", runIndex: 1, status: "merged" }),
      run({ id: "b", runIndex: 2, status: "merged" }),
      run({ id: "c", runIndex: 3, status: "agent_failed", judgeState: "failed" }),
      run({ id: "d", runIndex: 4, status: "pr_closed", judgeState: "done" }),
      run({ id: "e", runIndex: 5, status: "cancelled", judgeState: "none" }),
      run({ id: "f", runIndex: 6, status: "running", judgeState: "none", finishedAt: null }),
      run({ id: "g", runIndex: 7, status: "completed", judgeState: "done" }),
    ]);

    expect(summary.counts).toEqual([
      { key: "merged", label: "2 merged", tone: "success" },
      { key: "failed", label: "2 failed", tone: "error" },
      { key: "cancelled", label: "1 cancelled", tone: "neutral" },
      { key: "running", label: "1 running", tone: "accent" },
      { key: "completed", label: "1 completed", tone: "neutral" },
    ]);
  });

  it("omits empty buckets so the strip only shows real outcomes", () => {
    const summary = summarizeAutomationRuns([
      run({ id: "a", runIndex: 1, status: "merged" }),
    ]);

    expect(summary.counts).toEqual([
      { key: "merged", label: "1 merged", tone: "success" },
    ]);
  });

  it("treats a signal-terminal run with a pending judge as still running", () => {
    const summary = summarizeAutomationRuns([
      run({ id: "a", runIndex: 1, status: "completed", judgeState: "in_progress" }),
    ]);

    expect(summary.counts).toEqual([
      { key: "running", label: "1 running", tone: "accent" },
    ]);
  });

  it("counts a failed run as failed even while its judge is still pending", () => {
    const summary = summarizeAutomationRuns([
      run({ id: "a", runIndex: 1, status: "agent_failed", judgeState: "none" }),
    ]);

    expect(summary.counts).toEqual([
      { key: "failed", label: "1 failed", tone: "error" },
    ]);
  });

  it("reports the earliest run's creation time as the first run", () => {
    const summary = summarizeAutomationRuns([
      run({ id: "b", runIndex: 2, createdAt: "2026-07-20T10:00:00Z" }),
      run({ id: "a", runIndex: 1, createdAt: "2026-07-16T08:00:00Z" }),
    ]);

    expect(summary.firstRunAt).toBe("2026-07-16T08:00:00Z");
    expect(summary.total).toBe(2);
  });

  it("returns an empty summary when there are no runs", () => {
    const summary = summarizeAutomationRuns([]);

    expect(summary.counts).toEqual([]);
    expect(summary.firstRunAt).toBeNull();
    expect(summary.total).toBe(0);
  });
});

describe("chain mode copy", () => {
  it("explains how each chain mode advances the base", async () => {
    const { describeAutomationChainMode } = await import("./automationRunsSummary");

    expect(describeAutomationChainMode("merged_base")).toBe(
      "every merge advances the base",
    );
    expect(describeAutomationChainMode("pr_head_stacked")).toBe(
      "each run stacks on the previous PR head",
    );
  });
});
