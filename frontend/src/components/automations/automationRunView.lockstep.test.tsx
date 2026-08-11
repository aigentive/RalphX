import { cleanup, render, within } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import type { Automation, AutomationRun } from "@/api/automations";
import {
  AutomationJudgeStateSchema,
  AutomationRunStatusSchema,
} from "@/api/automations.schemas";
import type { AutomationGoalItem } from "./automationGoalItems";
import { AutomationRunStatusHeader } from "./AutomationRunStatusHeader";
import {
  OPEN_AUTOMATION_RUN_STATUS_SET,
  OPEN_JUDGE_PENDING_STATES,
  SIGNAL_TERMINAL_AUTOMATION_RUN_STATUS_SET,
} from "./automationRunStatusSets";
import { getAutomationRunView } from "./automationRunView";

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
    prNumber: 593,
    prUrl: "https://github.com/aigentive/ralphx.app/pull/593",
    prTitle: "P6",
    prHeadRefName: "ralphx/test",
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

const activeGoalItem: AutomationGoalItem = {
  id: "phase-1",
  title: "Land the automation selector contract",
  status: "in_progress",
};

const openJudgePendingStateSet = new Set(OPEN_JUDGE_PENDING_STATES);
const goalAuthorityJudgeStateSet = new Set<AutomationRun["judgeState"]>([
  "none",
  "in_progress",
  "done",
]);

function expectedIsOpen(candidate: AutomationRun): boolean {
  return (
    OPEN_AUTOMATION_RUN_STATUS_SET.has(candidate.status) ||
    (SIGNAL_TERMINAL_AUTOMATION_RUN_STATUS_SET.has(candidate.status) &&
      openJudgePendingStateSet.has(candidate.judgeState))
  );
}

function expectedHoldsGoalAuthority(candidate: AutomationRun): boolean {
  return (
    OPEN_AUTOMATION_RUN_STATUS_SET.has(candidate.status) ||
    (SIGNAL_TERMINAL_AUTOMATION_RUN_STATUS_SET.has(candidate.status) &&
      goalAuthorityJudgeStateSet.has(candidate.judgeState))
  );
}

describe("automation run view lockstep", () => {
  it("covers every status and judge-state combination with independent selector expectations", () => {
    let combinations = 0;

    for (const status of AutomationRunStatusSchema.options) {
      for (const judgeState of AutomationJudgeStateSchema.options) {
        combinations += 1;
        const candidate = run({ status, judgeState });
        const view = getAutomationRunView(automation(), candidate);
        const isOpen = expectedIsOpen(candidate);
        const composerReadOnly =
          candidate.status !== "awaiting_plan_approval" &&
          expectedHoldsGoalAuthority(candidate);

        expect(view.isOpen, `${status}/${judgeState} isOpen`).toBe(isOpen);
        expect(view.pr.rowLabel, `${status}/${judgeState} PR row label`).toBe(
          isOpen ? "Current PR" : "Last PR",
        );
        expect(
          view.composerReadOnly,
          `${status}/${judgeState} composer read-only`,
        ).toBe(composerReadOnly);
        if (status === "cancelled") {
          expect(view.stageLabel).not.toContain("Waiting for judge");
        }

        const { queryByTestId, unmount } = render(
          <AutomationRunStatusHeader
            automation={automation()}
            run={candidate}
            density="card"
            activeGoalItem={activeGoalItem}
            phaseTestId="phase-chip"
            showPr={false}
          />,
        );
        expect(
          Boolean(queryByTestId("phase-chip")),
          `${status}/${judgeState} phase chip`,
        ).toBe(isOpen);
        unmount();
        cleanup();
      }
    }

    expect(combinations).toBe(
      AutomationRunStatusSchema.options.length *
        AutomationJudgeStateSchema.options.length,
    );
    expect(combinations).toBe(50);
  });

  it.each([
    {
      status: "completed",
      judgeState: "none",
      expected: {
        isOpen: true,
        composerReadOnly: true,
        rowLabel: "Current PR",
        stageLabel: "Terminal judge pending",
      },
    },
    {
      status: "completed",
      judgeState: "done",
      expected: {
        isOpen: false,
        composerReadOnly: true,
        rowLabel: "Last PR",
        stageLabel: "Scheduling next run",
      },
    },
    {
      status: "completed",
      judgeState: "failed",
      expected: {
        isOpen: true,
        composerReadOnly: false,
        rowLabel: "Current PR",
        stageLabel: "Terminal judge failed",
      },
    },
    {
      status: "merged",
      judgeState: "failed",
      expected: {
        isOpen: true,
        composerReadOnly: false,
        rowLabel: "Current PR",
        stageLabel: "Terminal judge failed",
      },
    },
    {
      status: "merged",
      judgeState: "done",
      expected: {
        isOpen: false,
        composerReadOnly: true,
        rowLabel: "Last PR",
        stageLabel: "Scheduling next run",
      },
    },
    {
      status: "agent_failed",
      judgeState: "skipped",
      expected: {
        isOpen: false,
        composerReadOnly: false,
        rowLabel: "Last PR",
        stageLabel: "Scheduling next run",
      },
    },
  ] as const)(
    "pins tricky signal-terminal combo $status/$judgeState",
    ({ status, judgeState, expected }) => {
      const view = getAutomationRunView(automation(), run({ status, judgeState }));

      expect({
        isOpen: view.isOpen,
        composerReadOnly: view.composerReadOnly,
        rowLabel: view.pr.rowLabel,
        stageLabel: view.stageLabel,
      }).toEqual(expected);

      const { container } = render(
        <AutomationRunStatusHeader
          automation={automation()}
          run={run({ status, judgeState })}
          density="card"
          activeGoalItem={activeGoalItem}
          phaseTestId="phase-chip"
          showPr={false}
        />,
      );
      expect(Boolean(within(container).queryByTestId("phase-chip"))).toBe(
        expected.isOpen,
      );
    },
  );
});
