import { describe, expect, it } from "vitest";

import type { AutomationRun } from "@/api/automations";
import type { AutomationGoalItem } from "./automationGoalItems";
import {
  getAutomationPhaseGroups,
  getLatestMergedRun,
  getMergedPrByGoalItem,
  getPlanArtifactByGoalItem,
  getTrailingFailureStreak,
} from "./automationDetailPresentation";

function run(
  runIndex: number,
  overrides: Partial<AutomationRun> = {},
): AutomationRun {
  const timestamp = `2026-07-${String(runIndex).padStart(2, "0")}T12:00:00Z`;
  return {
    id: `run-${runIndex}`,
    automationId: "automation-1",
    runIndex,
    status: "completed",
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
    runPrompt: "",
    promptAuthor: "judge",
    baseRefKind: "project_default",
    baseRefUsed: "main",
    baseFromRunId: null,
    goalItemId: null,
    branchName: null,
    prNumber: null,
    prUrl: null,
    prTitle: null,
    prHeadRefName: null,
    prBaseRefName: null,
    prMergedAt: null,
    mergeCommitSha: null,
    diffStatsJson: null,
    agentSummary: null,
    judgeVerdictJson: null,
    judgeModelId: null,
    errorCode: null,
    errorDetail: null,
    signalCheckFailures: 0,
    startedAt: timestamp,
    finishedAt: timestamp,
    createdAt: timestamp,
    updatedAt: timestamp,
    ...overrides,
  };
}

describe("automation detail presentation", () => {
  it("counts trailing failures across open work and stops at the first settled success", () => {
    expect(getTrailingFailureStreak([
      run(1, { status: "merged" }),
      run(2, { status: "agent_failed", judgeState: "none" }),
      run(3, { status: "running", judgeState: "none" }),
      run(4, { status: "agent_failed", judgeState: "failed" }),
    ])).toBe(2);
    expect(getTrailingFailureStreak([])).toBe(0);
  });

  it("finds the newest merged run and newest merged PR per phase", () => {
    const runs = [
      run(1, {
        status: "merged",
        goalItemId: "A1",
        prNumber: 41,
        prUrl: "https://example.test/pr/41",
      }),
      run(2, {
        status: "completed",
        goalItemId: "A1",
        prNumber: 42,
        prUrl: "https://example.test/pr/42",
        prMergedAt: "2026-07-02T12:30:00Z",
      }),
      run(3, {
        status: "published",
        goalItemId: "B1",
        prNumber: 43,
        prUrl: "https://example.test/pr/43",
      }),
    ];

    expect(getLatestMergedRun(runs)?.runIndex).toBe(2);
    expect(getMergedPrByGoalItem(runs).get("A1")?.prNumber).toBe(42);
    expect(getMergedPrByGoalItem(runs).has("B1")).toBe(false);
  });

  it("keeps newest stamped plan artifacts without fabricating unmapped phases", () => {
    const plans = getPlanArtifactByGoalItem([
      run(1, { goalItemId: "A1", planArtifactId: "plan-old" }),
      run(2, { goalItemId: null, planArtifactId: "plan-unmapped" }),
      run(3, { goalItemId: "A1", planArtifactId: "plan-new" }),
    ]);

    expect(plans.get("A1")).toBe("plan-new");
    expect(plans.size).toBe(1);
  });

  it("groups majority letter-number phase ids and falls back to one flat group", () => {
    const groupedItems: AutomationGoalItem[] = [
      { id: "A1", title: "Foundation", status: "done" },
      { id: "A2", title: "Schema", status: "in_progress" },
      { id: "B1", title: "UI", status: "pending" },
      { id: "follow-up", title: "Follow-up", status: "pending" },
    ];
    const grouped = getAutomationPhaseGroups(groupedItems);
    expect(grouped.map((group) => group.label)).toEqual(["A", "B", "Other"]);
    expect(grouped[0]?.items.map((item) => item.id)).toEqual(["A1", "A2"]);

    const flat = getAutomationPhaseGroups([
      { id: "phase-one", title: "One", status: "done" },
      { id: "phase-two", title: "Two", status: "pending" },
    ]);
    expect(flat).toEqual([{
      key: "all",
      label: null,
      items: [
        { id: "phase-one", title: "One", status: "done" },
        { id: "phase-two", title: "Two", status: "pending" },
      ],
    }]);
  });
});
