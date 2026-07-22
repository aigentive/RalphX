import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import type { Automation, AutomationRun } from "@/api/automations";
import { TooltipProvider } from "@/components/ui/tooltip";

import { RunTimelineItem } from "./AutomationRunTimelineItem";

function automation(): Automation {
  const now = "2026-07-22T00:00:00Z";
  return {
    id: "automation-1",
    projectId: "project-1",
    name: "Ship migration loop",
    status: "active",
    pausedReasonCode: null,
    pausedReasonDetail: null,
    goalPrompt: "Keep landing migration tasks.",
    setupConversationId: "setup-conversation-1",
    specArtifactId: null,
    authoringMode: "spec",
    decompositionVerificationStatus: "not_started",
    decompositionVerificationVerdictJson: null,
    providerHarness: "codex",
    modelId: "gpt-5.6",
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
  };
}

function run(overrides: Partial<AutomationRun> = {}): AutomationRun {
  const now = "2026-07-22T00:00:00Z";
  return {
    id: "run-10",
    automationId: "automation-1",
    runIndex: 10,
    status: "agent_failed",
    judgeState: "failed",
    judgeLeaseExpiresAt: null,
    planJudgeState: "none",
    planRevisionRound: 0,
    planRevisionPending: false,
    planPhase: false,
    planArtifactId: null,
    planApprovedBy: null,
    planApprovedArtifactVersion: null,
    planApprovedAt: null,
    conversationId: null,
    runPrompt: "Continue the automation.",
    promptAuthor: "judge",
    baseRefKind: "project_default",
    baseRefUsed: "main",
    baseFromRunId: null,
    goalItemId: null,
    branchName: "ralphx/run-10",
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
    errorCode: "agent_failed",
    errorDetail: "Agent exited",
    signalCheckFailures: 0,
    startedAt: now,
    finishedAt: now,
    createdAt: now,
    updatedAt: now,
    ...overrides,
  };
}

function renderItem(
  candidate: AutomationRun,
  options: {
    isLatest?: boolean;
    onDeleteRun?: (run: AutomationRun) => void;
  } = {},
) {
  return render(
    <TooltipProvider delayDuration={0}>
      <RunTimelineItem
        run={candidate}
        automation={automation()}
        projectId={null}
        defaultExpanded={false}
        activeGoalItem={null}
        setupConversationId={null}
        {...(options.isLatest !== undefined && { isLatest: options.isLatest })}
        {...(options.onDeleteRun && { onDeleteRun: options.onDeleteRun })}
      />
    </TooltipProvider>,
  );
}

describe("RunTimelineItem run deletion", () => {
  it("keeps collapsed failures to one compact unboxed outcome line", () => {
    renderItem(run({
      judgeState: "done",
      agentSummary: "The agent stopped after the first step.",
    }));

    const outcome = screen.getByTestId("automation-run-run-10-failure");
    expect(outcome.tagName).toBe("P");
    expect(outcome).toHaveClass("truncate", "text-xs");
    expect(outcome).toHaveTextContent("Agent exited");
    expect(outcome).toHaveTextContent("The agent stopped after the first step.");
    expect(outcome.style.backgroundColor).toBe("");
    expect(screen.getByTestId("automation-run-run-10-card")).toHaveClass("p-3");
  });

  it("offers deletion for the latest failed run and passes that run to the handler", async () => {
    const user = userEvent.setup();
    const candidate = run();
    const onDeleteRun = vi.fn();
    renderItem(candidate, { isLatest: true, onDeleteRun });

    const deleteButton = screen.getByTestId("automation-run-run-10-delete");
    expect(deleteButton).toHaveAccessibleName("Delete run 10");

    await user.click(deleteButton);

    expect(onDeleteRun).toHaveBeenCalledOnce();
    expect(onDeleteRun).toHaveBeenCalledWith(candidate);
  });

  it("uses stop-and-delete copy for the latest running run", async () => {
    const user = userEvent.setup();
    renderItem(run({ status: "running", judgeState: "none", finishedAt: null }), {
      isLatest: true,
      onDeleteRun: vi.fn(),
    });

    const deleteButton = screen.getByRole("button", {
      name: "Stop and delete run 10",
    });
    await user.hover(deleteButton);

    expect(await screen.findByRole("tooltip")).toHaveTextContent(
      "Stop & delete run",
    );
  });

  it("does not offer deletion for a completed latest run", () => {
    renderItem(run({ status: "completed" }), {
      isLatest: true,
      onDeleteRun: vi.fn(),
    });

    expect(
      screen.queryByTestId("automation-run-run-10-delete"),
    ).not.toBeInTheDocument();
  });

  it("does not offer deletion for a failed non-latest run", () => {
    renderItem(run(), { isLatest: false, onDeleteRun: vi.fn() });

    expect(
      screen.queryByTestId("automation-run-run-10-delete"),
    ).not.toBeInTheDocument();
  });

  it("does not toggle expansion when delete is clicked", async () => {
    const user = userEvent.setup();
    renderItem(run(), { isLatest: true, onDeleteRun: vi.fn() });
    const expandButton = screen.getByRole("button", { name: "Expand run 10" });

    await user.click(screen.getByTestId("automation-run-run-10-delete"));

    expect(expandButton).toHaveAttribute("aria-expanded", "false");
    expect(
      screen.queryByTestId("automation-run-run-10-body"),
    ).not.toBeInTheDocument();
  });
});
