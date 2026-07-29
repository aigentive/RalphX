import { describe, expect, it } from "vitest";

import type { AutomationRun } from "@/api/automations";

import { deriveAutomationRunMilestones } from "./automationRunMilestones";

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
    finishedAt: null,
    createdAt: now,
    updatedAt: now,
    ...overrides,
  };
}

describe("deriveAutomationRunMilestones", () => {
  it("builds the merged happy path with elapsed offsets from the run start", () => {
    const milestones = deriveAutomationRunMilestones(
      run({
        startedAt: "2026-07-22T10:00:00Z",
        planArtifactId: "plan-1",
        prNumber: 905,
        prUrl: "https://example.test/pull/905",
        prMergedAt: "2026-07-22T10:28:00Z",
        finishedAt: "2026-07-22T10:28:00Z",
        status: "merged",
      }),
    );

    expect(milestones.map((m) => [m.key, m.elapsed, m.label])).toEqual([
      ["started", "00:00", "Run started"],
      ["plan", null, "Plan artifact published"],
      ["pr", null, "PR opened against base"],
      ["merged", "28:00", "Merged into base"],
    ]);
    expect(milestones.find((m) => m.key === "pr")?.chip).toBe("PR #905");
    expect(milestones.find((m) => m.key === "merged")?.tone).toBe("success");
  });

  it("ends a failed run on its failure reason rather than a generic finish", () => {
    const milestones = deriveAutomationRunMilestones(
      run({
        status: "agent_failed",
        judgeState: "failed",
        startedAt: "2026-07-22T10:00:00Z",
        finishedAt: "2026-07-22T10:09:00Z",
        errorCode: "agent_failed",
        errorDetail: "Agent exited",
      }),
    );

    const last = milestones.at(-1);
    expect(last?.key).toBe("failed");
    expect(last?.tone).toBe("error");
    expect(last?.label).toBe("Agent exited");
    expect(last?.elapsed).toBe("09:00");
    expect(milestones.some((m) => m.key === "finished")).toBe(false);
  });

  it("marks a cancelled run as cancelled", () => {
    const milestones = deriveAutomationRunMilestones(
      run({
        status: "cancelled",
        judgeState: "none",
        startedAt: "2026-07-22T10:00:00Z",
        finishedAt: "2026-07-22T10:02:00Z",
      }),
    );

    expect(milestones.at(-1)).toMatchObject({
      key: "cancelled",
      label: "Cancelled",
      tone: "neutral",
    });
  });

  it("reports an in-flight run as still running with no terminal milestone", () => {
    const milestones = deriveAutomationRunMilestones(
      run({ status: "running", judgeState: "none", planArtifactId: null }),
    );

    expect(milestones.map((m) => m.key)).toEqual(["started", "running"]);
    expect(milestones.at(-1)?.tone).toBe("accent");
  });

  it("omits the start milestone when the run never recorded a start time", () => {
    const milestones = deriveAutomationRunMilestones(
      run({ status: "pending", judgeState: "none", startedAt: null }),
    );

    expect(milestones.some((m) => m.key === "started")).toBe(false);
  });

  it("never invents commit or check milestones the backend does not record", () => {
    const milestones = deriveAutomationRunMilestones(
      run({
        diffStatsJson: JSON.stringify({ filesChanged: 6, additions: 120, deletions: 4 }),
        prNumber: 905,
        prMergedAt: "2026-07-22T10:28:00Z",
        finishedAt: "2026-07-22T10:28:00Z",
      }),
    );

    const labels = milestones.map((m) => m.label.toLowerCase());
    expect(labels.some((label) => label.includes("commit"))).toBe(false);
    expect(labels.some((label) => label.includes("check"))).toBe(false);
  });

  it("closes a completed run that produced no PR on its finish time", () => {
    const milestones = deriveAutomationRunMilestones(
      run({
        status: "completed",
        judgeState: "done",
        startedAt: "2026-07-22T10:00:00Z",
        finishedAt: "2026-07-22T10:15:00Z",
      }),
    );

    expect(milestones.at(-1)).toMatchObject({
      key: "finished",
      label: "Agent completed",
      elapsed: "15:00",
    });
  });
});
