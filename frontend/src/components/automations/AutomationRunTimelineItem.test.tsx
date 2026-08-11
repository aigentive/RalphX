import { render, screen } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

import type { Automation, AutomationRun } from "@/api/automations";
import { TooltipProvider } from "@/components/ui/tooltip";

import { RunTimelineItem } from "./AutomationRunTimelineItem";

const { listConversationTasksMock } = vi.hoisted(() => ({
  listConversationTasksMock: vi.fn(),
}));

vi.mock("@/api/agent-tasks", () => ({
  agentTaskApi: {
    listConversationTasks: (...args: unknown[]) => listConversationTasksMock(...args),
  },
}));

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
    isLastInTimeline?: boolean;
    onDeleteRun?: (run: AutomationRun) => void;
    onResumeRun?: (run: AutomationRun) => void;
    projectId?: string | null;
    onOpenRunConversation?: (projectId: string, conversationId: string) => void;
    defaultExpanded?: boolean;
  } = {},
) {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false, gcTime: 0 } },
  });
  return render(
    <QueryClientProvider client={queryClient}>
      <TooltipProvider delayDuration={0}>
        <RunTimelineItem
          run={candidate}
          automation={automation()}
          projectId={options.projectId ?? null}
          defaultExpanded={options.defaultExpanded ?? false}
          activeGoalItem={null}
          setupConversationId={null}
          {...(options.isLatest !== undefined && { isLatest: options.isLatest })}
          {...(options.isLastInTimeline !== undefined && {
            isLastInTimeline: options.isLastInTimeline,
          })}
          {...(options.onDeleteRun && { onDeleteRun: options.onDeleteRun })}
          {...(options.onResumeRun && { onResumeRun: options.onResumeRun })}
          {...(options.onOpenRunConversation && {
            onOpenRunConversation: options.onOpenRunConversation,
          })}
        />
      </TooltipProvider>
    </QueryClientProvider>,
  );
}

describe("RunTimelineItem run deletion", () => {
  beforeEach(() => {
    listConversationTasksMock.mockReset();
    listConversationTasksMock.mockResolvedValue([]);
  });

  it("keeps the collapsed PR link, timestamp, and chevron on the header line", () => {
    renderItem(run({
      status: "completed",
      judgeState: "done",
      prNumber: 612,
      prUrl: "https://github.com/aigentive/ralphx.app/pull/612",
    }));

    const headerRow = screen.getByTestId("automation-run-run-10-header-row");
    const prLink = screen.getByTestId("automation-run-run-10-pr-link");
    const updatedAt = screen.getByTestId("automation-run-run-10-updated-at");
    const expandButton = screen.getByRole("button", { name: "Expand run 10" });

    expect(headerRow).toHaveClass("flex-nowrap");
    expect(headerRow).toContainElement(prLink);
    expect(headerRow).toContainElement(updatedAt);
    expect(prLink.compareDocumentPosition(updatedAt) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
    expect(expandButton).not.toContainElement(prLink);
  });

  it("uses a compact spine marker and a soft darker edge for failed runs", () => {
    renderItem(run());

    const marker = screen.getByTestId("automation-run-run-10-marker");
    expect(marker).toHaveClass("h-2.5", "w-2.5");
    expect(marker).not.toHaveClass("animate-pulse");
    const card = screen.getByTestId("automation-run-run-10-card");
    expect(card.style.backgroundColor).toContain("--bg-surface");
    expect(card.style.borderTopColor).toContain("--border-default");
    expect(card.style.borderStyle).toBe("solid");
    expect(card.style.borderTopWidth).toBe("1px");
    // Status also reads from the left edge rule, per the runs-tab timeline design.
    expect(card.style.borderLeftColor).toContain("--status-error-strong");
    expect(card.style.borderLeftWidth).toBe("3px");
  });

  it("pulses the marker and accents the edge only while the run is live", () => {
    renderItem(run({ status: "running", judgeState: "none", errorCode: null, errorDetail: null, finishedAt: null }));

    expect(screen.getByTestId("automation-run-run-10-marker")).toHaveClass("animate-pulse");
    expect(
      screen.getByTestId("automation-run-run-10-card").style.borderLeftColor,
    ).toContain("--accent-primary");
  });

  it("draws the spine connector for every run except the oldest", () => {
    const { unmount } = renderItem(run());
    expect(screen.getByTestId("automation-run-run-10-connector")).toBeInTheDocument();
    unmount();

    renderItem(run(), { isLastInTimeline: true });
    expect(
      screen.queryByTestId("automation-run-run-10-connector"),
    ).not.toBeInTheDocument();
  });

  it("shows a settled run's duration next to its timestamp and omits it while live", () => {
    const { unmount } = renderItem(run({
      startedAt: "2026-07-22T00:00:00Z",
      finishedAt: "2026-07-22T00:28:00Z",
    }));

    expect(screen.getByTestId("automation-run-run-10-duration")).toHaveTextContent("28m");
    unmount();

    renderItem(run({ status: "running", judgeState: "none", finishedAt: null }));
    expect(
      screen.queryByTestId("automation-run-run-10-duration"),
    ).not.toBeInTheDocument();
  });

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
    expect(screen.getByTestId("automation-run-run-10-card")).toHaveClass("px-4", "py-3");
  });

  it("offers deletion for the latest failed run and passes that run to the handler", async () => {
    const user = userEvent.setup();
    const candidate = run();
    const onDeleteRun = vi.fn();
    renderItem(candidate, { isLatest: true, onDeleteRun, defaultExpanded: true });

    const deleteButton = screen.getByTestId("automation-run-run-10-delete");
    expect(deleteButton).toHaveAccessibleName("Delete run 10");
    // Delete lives in the footer now, never inside the collapse control.
    expect(screen.getByRole("button", { name: "Collapse run 10" }))
      .not.toContainElement(deleteButton);

    await user.click(deleteButton);

    expect(onDeleteRun).toHaveBeenCalledOnce();
    expect(onDeleteRun).toHaveBeenCalledWith(candidate);
  });

  it("uses stop-and-delete copy for the latest running run", async () => {
    const user = userEvent.setup();
    renderItem(run({ status: "running", judgeState: "none", finishedAt: null }), {
      isLatest: true,
      onDeleteRun: vi.fn(),
      defaultExpanded: true,
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
      defaultExpanded: true,
    });

    expect(
      screen.queryByTestId("automation-run-run-10-delete"),
    ).not.toBeInTheDocument();
  });

  it("does not offer deletion for a failed non-latest run", () => {
    renderItem(run(), { isLatest: false, onDeleteRun: vi.fn(), defaultExpanded: true });

    expect(
      screen.queryByTestId("automation-run-run-10-delete"),
    ).not.toBeInTheDocument();
  });

  it("does not collapse the card when delete is clicked", async () => {
    const user = userEvent.setup();
    const onDeleteRun = vi.fn();
    renderItem(run(), { isLatest: true, onDeleteRun, defaultExpanded: true });

    // Delete sits in the footer, separate from the collapse control, so clicking it
    // triggers the handler without bubbling to the card's expand/collapse toggle.
    await user.click(screen.getByTestId("automation-run-run-10-delete"));

    expect(onDeleteRun).toHaveBeenCalledOnce();
    expect(screen.getByRole("button", { name: "Collapse run 10" })).toHaveAttribute(
      "aria-expanded",
      "true",
    );
    expect(screen.getByTestId("automation-run-run-10-body")).toBeInTheDocument();
  });

  it("offers resume for the latest failed run and passes that run to the handler", async () => {
    const user = userEvent.setup();
    const candidate = run();
    const onResumeRun = vi.fn();
    renderItem(candidate, { isLatest: true, onResumeRun, defaultExpanded: true });

    const resumeButton = screen.getByTestId("automation-run-run-10-resume");
    expect(resumeButton).toHaveAccessibleName("Resume run 10");

    await user.click(resumeButton);

    expect(onResumeRun).toHaveBeenCalledOnce();
    expect(onResumeRun).toHaveBeenCalledWith(candidate);
  });

  it.each(["completed", "merged"] as const)(
    "does not offer resume for a latest %s run",
    (status) => {
      renderItem(run({ status }), {
        isLatest: true,
        onResumeRun: vi.fn(),
        defaultExpanded: true,
      });

      expect(
        screen.queryByTestId("automation-run-run-10-resume"),
      ).not.toBeInTheDocument();
    },
  );

  it("does not offer resume for a failed non-latest run", () => {
    renderItem(run(), {
      isLatest: false,
      onResumeRun: vi.fn(),
      defaultExpanded: true,
    });

    expect(
      screen.queryByTestId("automation-run-run-10-resume"),
    ).not.toBeInTheDocument();
  });

  it("renders delete and resume together for the latest failed run", () => {
    renderItem(run(), {
      isLatest: true,
      onDeleteRun: vi.fn(),
      onResumeRun: vi.fn(),
      defaultExpanded: true,
    });

    expect(screen.getByTestId("automation-run-run-10-delete")).toBeInTheDocument();
    expect(screen.getByTestId("automation-run-run-10-resume")).toBeInTheDocument();
  });

  it("renders Run prompt as a minimal inline disclosure", async () => {
    const user = userEvent.setup();
    renderItem(run());

    await user.click(screen.getByRole("button", { name: "Expand run 10" }));

    const promptToggle = screen.getByTestId("automation-run-run-10-prompt-toggle");
    expect(promptToggle).toHaveClass("border-0", "bg-transparent", "p-0", "text-xs");
    expect(promptToggle).toHaveClass("text-[var(--text-muted)]");
  });

  it("uses one status pill and demotes secondary run states to joined text", () => {
    renderItem(run());

    expect(screen.getByTestId("automation-run-run-10-header-status")).toHaveClass(
      "rounded-full",
    );
    const judgeState = screen.getByTestId("automation-run-run-10-header-judge");
    expect(judgeState).not.toHaveClass("rounded-full");
    expect(judgeState.closest("[data-testid='automation-run-run-10-header-secondary-states']"))
      .toHaveTextContent("failed");
  });

  it("anchors metadata, task ownership, and workflow actions in their zones", async () => {
    const user = userEvent.setup();
    const onOpenRunConversation = vi.fn();
    listConversationTasksMock.mockResolvedValue([
      {
        taskId: "task-1",
        taskNumber: 1,
        title: "Implement run card",
        state: "active",
        ownerAgent: "frontend-specialist",
        blockedBy: [],
        blocks: [],
        availability: "available",
        updatedAt: "2026-07-22T00:00:00Z",
      },
    ]);
    renderItem(
      run({
        status: "running",
        judgeState: "none",
        finishedAt: null,
        conversationId: "conversation-10",
        planArtifactId: "plan-10",
        prNumber: 612,
        prUrl: "https://github.com/aigentive/ralphx.app/pull/612",
      }),
      { projectId: "project-1", onOpenRunConversation, defaultExpanded: true },
    );

    const header = screen.getByTestId("automation-run-run-10-header-row");
    expect(header).toContainElement(screen.getByTestId("automation-run-run-10-pr-link"));
    const facts = screen.getByTestId("automation-run-run-10-facts");
    expect(await screen.findByText("frontend-specialist")).toBeInTheDocument();
    expect(facts).toHaveTextContent("Agent");
    expect(facts).toContainElement(screen.getByRole("button", { name: "Copy base ref" }));

    const footer = screen.getByTestId("automation-run-run-10-footer");
    const planButton = screen.getByRole("button", { name: "View run plan" });
    const conversationButton = screen.getByRole("button", { name: "Open conversation" });
    expect(footer).toContainElement(planButton);
    expect(footer).toContainElement(conversationButton);
    expect(planButton.compareDocumentPosition(conversationButton) & Node.DOCUMENT_POSITION_FOLLOWING)
      .toBeTruthy();

    await user.click(conversationButton);
    expect(onOpenRunConversation).toHaveBeenCalledWith("project-1", "conversation-10");
  });

  it("renders expanded failures as a left rule and keeps the footer action persistent", async () => {
    const user = userEvent.setup();
    renderItem(run());

    await user.click(screen.getByRole("button", { name: "Expand run 10" }));

    const failure = screen.getByTestId("automation-run-run-10-failure");
    expect(failure.tagName).toBe("DIV");
    expect(failure.style.borderLeftColor).toContain("--status-error");
    expect(failure.style.borderLeftStyle).toBe("solid");
    expect(failure.style.borderLeftWidth).toBe("2px");
    expect(screen.getByRole("button", { name: "Open conversation" })).toBeDisabled();
    expect(screen.getByRole("button", { name: "Open conversation" })).toHaveTextContent(
      "Open conversation",
    );
    expect(screen.getByTestId("automation-run-run-10-footer")).toBeInTheDocument();

    await user.hover(screen.getByTestId("automation-run-run-10-conversation-disabled-trigger"));
    expect(await screen.findByRole("tooltip")).toHaveTextContent("Run has not started");
  });
});
