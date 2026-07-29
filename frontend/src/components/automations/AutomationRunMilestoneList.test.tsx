import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import type { AutomationRun } from "@/api/automations";

import { AutomationRunMilestoneList } from "./AutomationRunMilestoneList";

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

describe("AutomationRunMilestoneList", () => {
  it("renders each derived milestone with its elapsed gutter and PR chip", () => {
    render(
      <AutomationRunMilestoneList
        run={run({
          startedAt: "2026-07-22T10:00:00Z",
          planArtifactId: "plan-1",
          prNumber: 905,
          prMergedAt: "2026-07-22T10:28:00Z",
          finishedAt: "2026-07-22T10:28:00Z",
        })}
      />,
    );

    const list = screen.getByTestId("automation-run-run-1-milestones");
    expect(list).toHaveTextContent("00:00");
    expect(list).toHaveTextContent("Run started");
    expect(screen.getByTestId("automation-run-run-1-milestone-plan")).toHaveTextContent(
      "Plan artifact published",
    );
    expect(list).toHaveTextContent("PR #905");
    expect(screen.getByTestId("automation-run-run-1-milestone-merged")).toHaveTextContent(
      "Merged into base",
    );
    expect(list).toHaveTextContent("28:00");
  });

  it("uses a dash rather than a fake offset for untimed milestones", () => {
    render(
      <AutomationRunMilestoneList
        run={run({ startedAt: "2026-07-22T10:00:00Z", planArtifactId: "plan-1" })}
      />,
    );

    const planRow = screen
      .getByTestId("automation-run-run-1-milestone-plan")
      .closest("li");
    expect(planRow).toHaveTextContent("—");
  });

  it("renders nothing when the run recorded no milestones at all", () => {
    const { container } = render(
      <AutomationRunMilestoneList
        run={run({ status: "pending", judgeState: "none", startedAt: null })}
      />,
    );

    // A queued run is still "open", so it reports its live status and nothing else.
    expect(container).toHaveTextContent("Pending");
    expect(
      screen.queryByTestId("automation-run-run-1-milestone-started"),
    ).not.toBeInTheDocument();
  });
});
