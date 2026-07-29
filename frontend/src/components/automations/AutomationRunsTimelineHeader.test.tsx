import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import type { AutomationRun } from "@/api/automations";

import { AutomationRunsTimelineHeader } from "./AutomationRunsTimelineHeader";

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

describe("AutomationRunsTimelineHeader", () => {
  it("renders one tinted pill per non-empty outcome bucket", () => {
    render(
      <AutomationRunsTimelineHeader
        chainMode="merged_base"
        runs={[
          run({ id: "a", runIndex: 1, status: "merged" }),
          run({ id: "b", runIndex: 2, status: "merged" }),
          run({ id: "c", runIndex: 3, status: "agent_failed", errorCode: "agent_failed" }),
          run({ id: "d", runIndex: 4, status: "running", judgeState: "none", finishedAt: null }),
        ]}
      />,
    );

    expect(screen.getByTestId("automation-runs-summary-merged")).toHaveTextContent(
      "2 merged",
    );
    expect(screen.getByTestId("automation-runs-summary-merged")).toHaveAttribute(
      "data-tone",
      "success",
    );
    expect(screen.getByTestId("automation-runs-summary-failed")).toHaveTextContent(
      "1 failed",
    );
    expect(screen.getByTestId("automation-runs-summary-running")).toHaveAttribute(
      "data-tone",
      "accent",
    );
    expect(
      screen.queryByTestId("automation-runs-summary-cancelled"),
    ).not.toBeInTheDocument();
  });

  it("states the first run date and how the chain advances", () => {
    render(
      <AutomationRunsTimelineHeader
        chainMode="merged_base"
        runs={[
          run({ id: "b", runIndex: 2, createdAt: "2026-07-20T10:00:00Z" }),
          run({ id: "a", runIndex: 1, createdAt: "2026-07-16T08:00:00Z" }),
        ]}
      />,
    );

    const hint = screen.getByTestId("automation-runs-timeline-hint");
    expect(hint).toHaveTextContent("first run Jul 16");
    expect(hint).toHaveTextContent("every merge advances the base");
  });

  it("describes stacked chains without inventing a first-run date", () => {
    render(<AutomationRunsTimelineHeader chainMode="pr_head_stacked" runs={[]} />);

    const hint = screen.getByTestId("automation-runs-timeline-hint");
    expect(hint).toHaveTextContent("each run stacks on the previous PR head");
    expect(hint).not.toHaveTextContent("first run");
  });
});
