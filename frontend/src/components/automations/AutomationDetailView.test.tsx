import { render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { TooltipProvider } from "@/components/ui/tooltip";
import { AutomationDetailView } from "./AutomationDetailView";
import type { Automation, AutomationDetail, AutomationRun } from "@/api/automations.types";

const {
  getAutomationMock,
  pauseAutomationMock,
  resumeAutomationMock,
  finalizeAutomationMock,
  stopAutomationMock,
  restartAutomationMock,
  retryJudgeMock,
  retryPlanJudgeMock,
  triggerRunNowMock,
  skipJudgeMock,
  deleteRunMock,
  deleteAutomationMock,
  useArtifactMock,
  listConversationTasksMock,
  openExternalUrlMock,
  toastSuccessMock,
  toastErrorMock,
  toastInfoMock,
} = vi.hoisted(() => ({
  getAutomationMock: vi.fn(),
  pauseAutomationMock: vi.fn(),
  resumeAutomationMock: vi.fn(),
  finalizeAutomationMock: vi.fn(),
  stopAutomationMock: vi.fn(),
  restartAutomationMock: vi.fn(),
  retryJudgeMock: vi.fn(),
  retryPlanJudgeMock: vi.fn(),
  triggerRunNowMock: vi.fn(),
  skipJudgeMock: vi.fn(),
  deleteRunMock: vi.fn(),
  deleteAutomationMock: vi.fn(),
  useArtifactMock: vi.fn(),
  listConversationTasksMock: vi.fn(),
  openExternalUrlMock: vi.fn(),
  toastSuccessMock: vi.fn(),
  toastErrorMock: vi.fn(),
  toastInfoMock: vi.fn(),
}));

vi.mock("@/api/agent-tasks", () => ({
  agentTaskApi: {
    listConversationTasks: (...args: unknown[]) => listConversationTasksMock(...args),
  },
}));

vi.mock("@/hooks/useArtifacts", () => ({
  useArtifact: (...args: unknown[]) => useArtifactMock(...args),
}));

vi.mock("sonner", () => ({
  toast: {
    success: toastSuccessMock,
    error: toastErrorMock,
    info: toastInfoMock,
  },
}));

vi.mock("@/lib/open-external", () => ({
  openExternalUrl: (...args: unknown[]) => openExternalUrlMock(...args),
}));

vi.mock("@/api/automations", () => ({
  automationsApi: {
    get: getAutomationMock,
    pause: pauseAutomationMock,
    resume: resumeAutomationMock,
    finalize: finalizeAutomationMock,
    stop: stopAutomationMock,
    restart: restartAutomationMock,
    retryJudge: retryJudgeMock,
    retryPlanJudge: retryPlanJudgeMock,
    triggerRunNow: triggerRunNowMock,
    skipJudge: skipJudgeMock,
    deleteRun: deleteRunMock,
    delete: deleteAutomationMock,
  },
}));

function automation(overrides: Partial<Automation> = {}): Automation {
  const now = "2026-07-05T00:00:00Z";
  return {
    id: "automation-1",
    projectId: "project-1",
    name: "Ship migration loop",
    status: "active",
    pausedReasonCode: null,
    pausedReasonDetail: null,
    goalPrompt: Array.from({ length: 12 }, (_, index) => `Goal line ${index + 1}`).join("\n"),
    setupConversationId: "setup-conversation-1",
    specArtifactId: null,
    providerHarness: "codex",
    modelId: "gpt-5.4",
    logicalEffort: "high",
    runMode: "edit",
    baseRefKind: "project_default",
    baseRef: "main",
    baseDisplayName: "main",
    baseSourcePullRequestJson: JSON.stringify({
      number: 593,
      title: "Automation backend",
      url: "https://github.com/aigentive/ralphx.app/pull/593",
    }),
    goalItemsJson: JSON.stringify([{ id: "item-1", title: "Land P6", status: "in_progress" }]),
    chainMode: "merged_base",
    completionSignal: "pr_merged",
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
    status: "merged",
    judgeState: "done",
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
    runPrompt: Array.from({ length: 12 }, (_, index) => `Prompt line ${index + 1}`).join("\n"),
    promptAuthor: "setup_agent",
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
    prMergedAt: now,
    mergeCommitSha: "abc123",
    diffStatsJson: JSON.stringify({ filesChanged: 3, additions: 12, deletions: 4 }),
    agentSummary: "Implemented the run.",
    judgeVerdictJson: JSON.stringify({
      decision: "continue",
      reason: "More work remains.",
      confidence: 0.87,
      nextRunPrompt: "Continue with the next scoped automation task.",
    }),
    judgeModelId: "gpt-5.4-mini",
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

const usage = {
  inputTokens: 200,
  outputTokens: 50,
  cacheCreationTokens: 7,
  cacheReadTokens: 9,
  estimatedUsd: 0.06,
};

function renderDetail(
  detail: AutomationDetail,
  onOpenRunConversation = vi.fn(),
  onOpenAutomationRun = vi.fn(),
) {
  const queryClient = new QueryClient({
    defaultOptions: {
      queries: { retry: false, gcTime: 0 },
      mutations: { retry: false },
    },
  });
  getAutomationMock.mockResolvedValue(detail);
  return {
    onOpenAutomationRun,
    onOpenRunConversation,
    ...render(
      <QueryClientProvider client={queryClient}>
        <TooltipProvider>
          <AutomationDetailView
            automationId="automation-1"
            projectId="project-1"
            projectName="Demo Project"
            onBack={vi.fn()}
            onOpenRunConversation={onOpenRunConversation}
            onOpenAutomationRun={onOpenAutomationRun}
          />
        </TooltipProvider>
      </QueryClientProvider>,
    ),
  };
}

function renderDetailWithQuery(onBack = vi.fn()) {
  const queryClient = new QueryClient({
    defaultOptions: {
      queries: { retry: false, gcTime: 0 },
      mutations: { retry: false },
    },
  });
  return render(
    <QueryClientProvider client={queryClient}>
      <TooltipProvider>
        <AutomationDetailView
          automationId="automation-1"
          projectId="project-1"
          projectName="Demo Project"
          onBack={onBack}
          onOpenRunConversation={vi.fn()}
        />
      </TooltipProvider>
    </QueryClientProvider>,
  );
}

/** Select the page-level Runs tab and wait for the deferred timeline mount. */
async function openRunsTab() {
  await userEvent.click(await screen.findByTestId("automation-tab-runs"));
  await screen.findByTestId("automation-runs-timeline");
}

describe("AutomationDetailView", () => {
  beforeEach(() => {
    vi.stubGlobal(
      "requestAnimationFrame",
      (cb: FrameRequestCallback): number =>
        window.setTimeout(() => cb(performance.now()), 0),
    );
    vi.stubGlobal("cancelAnimationFrame", (handle: number): void => {
      window.clearTimeout(handle);
    });
    getAutomationMock.mockReset();
    pauseAutomationMock.mockReset().mockResolvedValue(automation({ status: "paused" }));
    resumeAutomationMock.mockReset().mockResolvedValue(automation());
    finalizeAutomationMock.mockReset().mockResolvedValue(automation({ status: "active" }));
    stopAutomationMock.mockReset().mockResolvedValue(automation({ status: "stopped" }));
    restartAutomationMock.mockReset().mockResolvedValue({ scheduled: true, reason: null });
    retryJudgeMock.mockReset().mockResolvedValue({ scheduled: true, reason: null });
    retryPlanJudgeMock.mockReset().mockResolvedValue({ scheduled: true, reason: null });
    triggerRunNowMock.mockReset().mockResolvedValue({ scheduled: true, reason: null });
    skipJudgeMock.mockReset().mockResolvedValue({ scheduled: true, reason: null });
    deleteRunMock.mockReset().mockResolvedValue(undefined);
    deleteAutomationMock.mockReset().mockResolvedValue(undefined);
    useArtifactMock.mockReset().mockReturnValue({
      data: null,
      isLoading: false,
      isError: false,
    });
    listConversationTasksMock.mockReset().mockResolvedValue([]);
    openExternalUrlMock.mockReset().mockResolvedValue(undefined);
    toastSuccessMock.mockReset();
    toastErrorMock.mockReset();
    toastInfoMock.mockReset();
  });

  it("shows ideation task-graph progress in the automation pipeline", async () => {
    renderDetail({
      automation: automation({
        runMode: "ideation",
        completionSignal: "ideation_finalized",
      }),
      runs: [run({ status: "completed", prNumber: null, prUrl: null })],
      usage,
      pipeline: {
        deliverable: "task_graph",
        status: "executing",
        ideationSessionId: "session-1",
        planArtifactId: "plan-1",
        proposalCount: 2,
        taskTotal: 2,
        taskMerged: 1,
        taskTerminal: 1,
        tasks: [
          {
            id: "task-2",
            title: "Build the detail surface",
            status: "ready",
            blockedBy: ["task-1"],
          },
        ],
      },
    });

    const pipeline = await screen.findByTestId("automation-pipeline-progress");
    expect(within(pipeline).getByText("Task pipeline")).toBeInTheDocument();
    expect(within(pipeline).getByText("1 / 2 merged")).toBeInTheDocument();
    expect(within(pipeline).getByText("Build the detail surface")).toBeInTheDocument();
    expect(within(pipeline).getByText("1 dependency")).toBeInTheDocument();
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("renders the detail controls and newest-first timeline with collapsed prompt bodies", async () => {
    const olderRun = run({ id: "run-1", runIndex: 1 });
    const latestRun = run({
      id: "run-2",
      runIndex: 2,
      status: "merged",
      judgeState: "none",
      conversationId: "conversation-2",
      promptAuthor: "judge",
    });

    const { onOpenAutomationRun, onOpenRunConversation } = renderDetail({
      automation: automation(),
      runs: [olderRun, latestRun],
      usage,
    });

    expect(await screen.findByTestId("automation-detail-view")).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Ship migration loop" })).toBeInTheDocument();
    expect(screen.getByLabelText("Pause automation")).toBeInTheDocument();
    expect(screen.getByLabelText("Run now")).toBeInTheDocument();
    expect(screen.getByLabelText("Cancel automation")).toBeInTheDocument();
    expect(screen.getByText("Input tokens")).toBeInTheDocument();
    expect(screen.getByText("200")).toBeInTheDocument();
    expect(screen.getByText("Estimated cost")).toBeInTheDocument();
    expect(screen.getByText("$0.06")).toBeInTheDocument();

    const goalCard = screen.getByTestId("automation-goal-card");
    expect(goalCard).toHaveTextContent("Goal line 10");
    expect(goalCard).not.toHaveTextContent("Goal line 11");

    await userEvent.click(screen.getAllByRole("button", { name: "Expand" })[0]);

    expect(goalCard).toHaveTextContent("Goal line 11");

    await userEvent.click(screen.getByTestId("automation-setup-conversation-link"));
    expect(onOpenRunConversation).toHaveBeenCalledWith(
      "project-1",
      "setup-conversation-1",
    );

    await openRunsTab();
    const runTwo = screen.getByTestId("automation-run-run-2");
    const runOne = screen.getByTestId("automation-run-run-1");
    expect(runTwo.compareDocumentPosition(runOne)).toBe(Node.DOCUMENT_POSITION_FOLLOWING);
    expect(within(runTwo).getByText("Run 2")).toBeInTheDocument();

    await userEvent.click(within(runTwo).getByRole("button", { name: "Open conversation" }));
    expect(onOpenAutomationRun).toHaveBeenCalledWith({
      projectId: "project-1",
      automationId: "automation-1",
      runId: "run-2",
      conversationId: "conversation-2",
      setupConversationId: "setup-conversation-1",
      runStatus: "merged",
      judgeState: "none",
      planPhase: false,
      planArtifactId: null,
      prNumber: 593,
      prUrl: "https://github.com/aigentive/ralphx.app/pull/593",
    });

    await userEvent.click(within(runTwo).getByRole("button", { name: "Show next prompt" }));
    expect(within(runTwo).getByText("Continue with the next scoped automation task.")).toBeInTheDocument();
  });

  it("renders a copyable concrete branch in the config grid", async () => {
    const writeText = vi.fn().mockResolvedValue(undefined);
    const user = userEvent.setup();
    const originalClipboard = (navigator as unknown as { clipboard?: unknown }).clipboard;
    Object.defineProperty(globalThis.navigator, "clipboard", {
      configurable: true,
      value: { writeText },
      writable: true,
    });

    try {
      renderDetail({
        automation: automation({
          baseDisplayName: "Automation workspace",
          baseRef: "ralphx/automation-workspace/automation-1",
        }),
        runs: [],
        usage,
      });

      await screen.findByTestId("automation-detail-view");
      expect(screen.getByText("Branch")).toBeInTheDocument();
      expect(screen.getAllByText("Automation workspace").length).toBeGreaterThan(0);
      expect(screen.getByTestId("automation-branch-value")).toHaveTextContent(
        "ralphx/automation-workspace/automation-1",
      );

      await user.click(screen.getByRole("button", { name: "Copy branch" }));

      expect(writeText).toHaveBeenCalledWith(
        "ralphx/automation-workspace/automation-1",
      );
      expect(toastSuccessMock).toHaveBeenCalledWith("Branch copied");
    } finally {
      if (originalClipboard !== undefined) {
        Object.defineProperty(navigator, "clipboard", {
          configurable: true,
          value: originalClipboard,
        });
      } else {
        delete (navigator as unknown as { clipboard?: unknown }).clipboard;
      }
    }
  });

  it("opens run pull requests through the external opener seam", async () => {
    const user = userEvent.setup();
    renderDetail({
      automation: automation(),
      runs: [
        run({
          id: "run-pr",
          prNumber: 612,
          prUrl: "https://github.com/aigentive/ralphx.app/pull/612",
        }),
      ],
      usage,
    });

    await screen.findByTestId("automation-detail-view");
    await openRunsTab();
    expect(
      screen.getAllByRole("button", { name: "Open PR #612 in browser" }),
    ).toHaveLength(1);
    expect(screen.getByTestId("automation-run-run-pr-pr-link")).toHaveTextContent(
      "PR #612",
    );
    expect(
      screen.queryByTestId("automation-run-run-pr-pr-field-link"),
    ).not.toBeInTheDocument();

    await user.click(screen.getByTestId("automation-run-run-pr-pr-link"));

    expect(openExternalUrlMock).toHaveBeenCalledWith(
      "https://github.com/aigentive/ralphx.app/pull/612",
    );
  });

  it("renders a URL-only run pull request link", async () => {
    const user = userEvent.setup();
    renderDetail({
      automation: automation(),
      runs: [
        run({
          id: "run-pr-url-only",
          prNumber: null,
          prUrl: "https://github.com/aigentive/ralphx.app/pull/preview",
        }),
      ],
      usage,
    });

    await screen.findByTestId("automation-detail-view");
    await openRunsTab();
    expect(
      screen.getAllByRole("button", { name: "Open PR in browser" }),
    ).toHaveLength(1);
    expect(
      screen.getByTestId("automation-run-run-pr-url-only-pr-link"),
    ).toHaveTextContent("PR");
    expect(
      screen.queryByTestId("automation-run-run-pr-url-only-pr-field-link"),
    ).not.toBeInTheDocument();
    expect(screen.queryByText("Not published")).not.toBeInTheDocument();

    await user.click(screen.getByTestId("automation-run-run-pr-url-only-pr-link"));

    expect(openExternalUrlMock).toHaveBeenCalledWith(
      "https://github.com/aigentive/ralphx.app/pull/preview",
    );
  });

  it("renders non-HTTP PR URLs as disabled text", async () => {
    renderDetail({
      automation: automation(),
      runs: [
        run({
          id: "run-pr-local",
          prNumber: 612,
          prUrl: "file:///tmp/preview",
        }),
      ],
      usage,
    });

    await screen.findByTestId("automation-detail-view");
    await openRunsTab();

    const disabledPr = screen.getByTestId("automation-run-run-pr-local-pr-link");
    expect(disabledPr).toHaveTextContent("PR #612");
    expect(disabledPr).toHaveAttribute("aria-disabled", "true");
    expect(
      screen.queryByRole("button", { name: "Open PR #612 in browser" }),
    ).not.toBeInTheDocument();
    expect(openExternalUrlMock).not.toHaveBeenCalled();
  });

  it("does not render an external PR link when a run is unpublished", async () => {
    renderDetail({
      automation: automation(),
      runs: [run({ id: "run-unpublished", prNumber: null, prUrl: null })],
      usage,
    });

    await screen.findByTestId("automation-detail-view");
    await openRunsTab();

    expect(
      screen.queryByRole("button", { name: /Open PR #/i }),
    ).not.toBeInTheDocument();
    expect(screen.queryByText("Not published")).not.toBeInTheDocument();
    expect(
      screen.queryByTestId("automation-run-run-unpublished-pr-state"),
    ).not.toBeInTheDocument();
  });

  it("expands the latest and open runs by default and collapses older terminal runs", async () => {
    const olderRun = run({
      id: "run-1",
      runIndex: 1,
      status: "merged",
      judgeState: "done",
    });
    const openRun = run({
      id: "run-2",
      runIndex: 2,
      status: "published",
      judgeState: "none",
      conversationId: "conversation-2",
    });
    const latestTerminal = run({
      id: "run-3",
      runIndex: 3,
      status: "merged",
      judgeState: "done",
    });

    renderDetail({
      automation: automation({ status: "active" }),
      runs: [olderRun, openRun, latestTerminal],
      usage,
    });

    await screen.findByTestId("automation-detail-view");
    await openRunsTab();

    // Latest run (index 3) is expanded even though it is terminal.
    expect(screen.getByTestId("automation-run-run-3-body")).toBeInTheDocument();
    // Open run (published/none) is expanded regardless of being the latest.
    expect(screen.getByTestId("automation-run-run-2-body")).toBeInTheDocument();
    // Older terminal run collapses by default.
    expect(screen.queryByTestId("automation-run-run-1-body")).not.toBeInTheDocument();
  });

  it("toggles a collapsed run body open and closed", async () => {
    const olderRun = run({
      id: "run-1",
      runIndex: 1,
      status: "merged",
      judgeState: "done",
    });
    const latestRun = run({ id: "run-2", runIndex: 2, status: "merged", judgeState: "done" });

    renderDetail({
      automation: automation({ status: "active" }),
      runs: [olderRun, latestRun],
      usage,
    });

    await screen.findByTestId("automation-detail-view");
    await openRunsTab();

    expect(screen.queryByTestId("automation-run-run-1-body")).not.toBeInTheDocument();

    await userEvent.click(screen.getByRole("button", { name: "Expand run 1" }));
    expect(screen.getByTestId("automation-run-run-1-body")).toBeInTheDocument();

    await userEvent.click(screen.getByRole("button", { name: "Collapse run 1" }));
    expect(screen.queryByTestId("automation-run-run-1-body")).not.toBeInTheDocument();
  });

  it("renders the live task ledger inside an expanded open run", async () => {
    listConversationTasksMock.mockResolvedValue([
      {
        taskId: "task-a",
        taskNumber: 1,
        title: "Refactor scheduler",
        state: "active",
        ownerAgent: "coder-1",
        blockedBy: [],
        blocks: [],
        availability: "available",
        updatedAt: "2026-07-05T00:00:00Z",
      },
    ]);

    renderDetail({
      automation: automation({ status: "active" }),
      runs: [
        run({
          id: "run-live",
          runIndex: 1,
          status: "running",
          judgeState: "none",
          conversationId: "conversation-live",
        }),
      ],
      usage,
    });

    await screen.findByTestId("automation-detail-view");
    await openRunsTab();

    const ledger = await screen.findByTestId("automation-run-task-ledger");
    expect(within(ledger).getByText("Refactor scheduler")).toBeInTheDocument();
    await waitFor(() =>
      expect(listConversationTasksMock).toHaveBeenCalledWith(
        expect.objectContaining({
          conversationId: "conversation-live",
          projectId: "project-1",
          includeDone: true,
        }),
      ),
    );
  });

  it("renders the linked spec and a Phases heading in the goal card", async () => {
    const user = userEvent.setup();
    useArtifactMock.mockReturnValue({
      data: {
        id: "artifact-spec-1",
        name: "Migration loop spec",
        artifact_type: "specification",
        content_type: "inline",
        content: "## Phase 1\nBuild the shared context model.",
        created_at: "2026-07-05T00:00:00Z",
        created_by: "setup-agent",
        version: 1,
        bucket_id: null,
        task_id: null,
        process_id: null,
        derived_from: [],
      },
      isLoading: false,
      isError: false,
    });

    renderDetail({
      automation: automation({ specArtifactId: "artifact-spec-1" }),
      runs: [run()],
      usage,
    });

    await screen.findByTestId("automation-detail-view");
    await user.click(screen.getByRole("tab", { name: "Spec" }));

    const specCard = screen.getByTestId("automation-spec-card");
    expect(specCard).toHaveTextContent("Migration loop spec");
    expect(specCard).toHaveTextContent("Build the shared context model.");
    expect(useArtifactMock).toHaveBeenCalledWith("artifact-spec-1");

    const goalCard = screen.getByTestId("automation-goal-card");
    expect(goalCard).toHaveTextContent("Phases");
    expect(goalCard).toHaveTextContent("Land P6");
  });

  it("shows phase progress and a collapsed expandable spec", async () => {
    const user = userEvent.setup();
    useArtifactMock.mockReturnValue({
      data: {
        id: "artifact-spec-1",
        name: "Migration loop spec",
        artifact_type: "specification",
        content_type: "inline",
        content: "## Phase 1\nBuild the shared context model.",
        created_at: "2026-07-05T00:00:00Z",
        created_by: "setup-agent",
        version: 1,
        bucket_id: null,
        task_id: null,
        process_id: null,
        derived_from: [],
      },
      isLoading: false,
      isError: false,
    });

    renderDetail({
      automation: automation({
        specArtifactId: "artifact-spec-1",
        goalItemsJson: JSON.stringify([
          { id: "a", title: "Phase A", status: "done" },
          { id: "b", title: "Phase B", status: "in_progress" },
          { id: "c", title: "Phase C", status: "pending" },
        ]),
      }),
      runs: [run()],
      usage,
    });

    await screen.findByTestId("automation-detail-view");

    const goalCard = screen.getByTestId("automation-goal-card");
    expect(goalCard).toHaveTextContent("1/3 done");
    expect(within(goalCard).getByRole("progressbar")).toHaveAttribute(
      "aria-valuenow",
      "1",
    );
    expect(within(goalCard).getByText("In progress")).toBeInTheDocument();

    // Spec stays collapsed by default with an expand affordance; heavy markdown
    // is not mounted until the user expands it.
    await user.click(screen.getByRole("tab", { name: "Spec" }));
    const specCard = screen.getByTestId("automation-spec-card");
    expect(within(specCard).getByTestId("automation-spec-toggle")).toHaveTextContent(
      "Show full spec",
    );
    expect(
      within(specCard).queryByTestId("automation-spec-markdown"),
    ).not.toBeInTheDocument();
  });

  it("keeps every phase available in a scrollable list", async () => {
    const phases = Array.from({ length: 25 }, (_, index) => ({
      id: `phase-${index + 1}`,
      title: `Phase ${index + 1}`,
      status: index === 0 ? "in_progress" : "pending",
    }));

    renderDetail({
      automation: automation({ goalItemsJson: JSON.stringify(phases) }),
      runs: [run()],
      usage,
    });

    await screen.findByTestId("automation-detail-view");

    const goalCard = screen.getByTestId("automation-goal-card");
    const phaseList = within(goalCard).getByRole("list", {
      name: "Automation phases",
    });
    expect(within(phaseList).getAllByRole("listitem")).toHaveLength(25);
    expect(within(phaseList).getByText("Phase 25")).toBeInTheDocument();
    expect(phaseList).toHaveClass("max-h-64", "overflow-y-auto");
    expect(phaseList).toHaveAttribute("tabindex", "0");
  });

  it("shows the current phase chip on open timeline runs only", async () => {
    renderDetail({
      automation: automation({
        goalItemsJson: JSON.stringify([
          { id: "item-1", title: "Finish the live telemetry plumbing", status: "in_progress" },
          { id: "item-2", title: "Polish docs", status: "pending" },
        ]),
      }),
      runs: [
        run({
          id: "run-open",
          runIndex: 2,
          status: "running",
          judgeState: "none",
          finishedAt: null,
        }),
        run({
          id: "run-terminal",
          runIndex: 1,
          status: "merged",
          judgeState: "done",
        }),
      ],
      usage,
    });

    await screen.findByTestId("automation-detail-view");
    await openRunsTab();

    const openRun = screen.getByTestId("automation-run-run-open");
    expect(
      within(openRun).getByTestId("automation-run-run-open-phase"),
    ).toHaveTextContent("Finish the live telemetry plumbing");

    const terminalRun = screen.getByTestId("automation-run-run-terminal");
    expect(
      within(terminalRun).queryByTestId("automation-run-run-terminal-phase"),
    ).not.toBeInTheDocument();
  });

  it("matches timeline card and marker highlights to the run outcome", async () => {
    renderDetail({
      automation: automation(),
      runs: [
        run({ id: "run-success", runIndex: 3, status: "merged", judgeState: "done" }),
        run({
          id: "run-progress",
          runIndex: 2,
          status: "running",
          judgeState: "none",
          finishedAt: null,
        }),
        run({
          id: "run-failed",
          runIndex: 1,
          status: "agent_failed",
          judgeState: "failed",
          errorDetail: "Agent exited before publishing",
        }),
      ],
      usage,
    });

    await screen.findByTestId("automation-detail-view");
    await openRunsTab();

    const successCard = screen.getByTestId("automation-run-run-success-card");
    expect(successCard.style.backgroundColor).toBe("var(--status-success-muted)");
    expect(successCard.style.borderColor).toBe("var(--status-success-border)");
    expect(screen.getByTestId("automation-run-run-success-marker").style.backgroundColor).toBe(
      "var(--status-success)",
    );

    const progressCard = screen.getByTestId("automation-run-run-progress-card");
    expect(progressCard.style.backgroundColor).toBe("var(--accent-muted)");
    expect(progressCard.style.borderColor).toBe("var(--accent-border)");
    expect(screen.getByTestId("automation-run-run-progress-marker").style.backgroundColor).toBe(
      "var(--accent-primary)",
    );

    const failedCard = screen.getByTestId("automation-run-run-failed-card");
    expect(failedCard.style.backgroundColor).toBe("var(--bg-hover)");
    expect(failedCard.style.borderColor).toBe("var(--border-default)");
    expect(screen.getByTestId("automation-run-run-failed-marker").style.backgroundColor).toBe(
      "var(--text-muted)",
    );
  });

  it("does not show a timeline phase chip when no goal item is in progress", async () => {
    renderDetail({
      automation: automation({
        goalItemsJson: JSON.stringify([
          { id: "item-1", title: "Pending work", status: "pending" },
        ]),
      }),
      runs: [
        run({
          id: "run-open",
          status: "running",
          judgeState: "none",
          finishedAt: null,
        }),
      ],
      usage,
    });

    await screen.findByTestId("automation-detail-view");
    await openRunsTab();

    expect(
      within(screen.getByTestId("automation-run-run-open")).queryByTestId(
        "automation-run-run-open-phase",
      ),
    ).not.toBeInTheDocument();
  });

  it("shows the spec fallback when no spec is linked", async () => {
    const user = userEvent.setup();
    renderDetail({
      automation: automation({ specArtifactId: null }),
      runs: [run()],
      usage,
    });

    await screen.findByTestId("automation-detail-view");
    await user.click(screen.getByRole("tab", { name: "Spec" }));

    expect(screen.getByTestId("automation-spec-card")).toHaveTextContent(
      "No spec linked yet.",
    );
  });

  it("defaults to Config and keeps config, spec, inputs, and paused context one click away", async () => {
    const user = userEvent.setup();
    renderDetail({
      automation: automation({
        status: "paused",
        pausedReasonCode: "release_freeze",
        pausedReasonDetail: "Waiting on base branch",
        specArtifactId: null,
        baseSourcePullRequestJson: null,
      }),
      runs: [run()],
      usage,
    });

    const card = await screen.findByTestId("automation-details-card");
    const configTab = within(card).getByRole("tab", { name: "Config" });
    expect(configTab).toHaveAttribute("aria-selected", "true");
    const configPanel = within(card).getByTestId("automation-config-panel");
    const execution = within(configPanel).getByTestId("automation-config-group-execution");
    expect(within(execution).getByText("Execution")).toBeInTheDocument();
    expect(
      within(execution).getAllByRole("term").map((term) => term.textContent),
    ).toEqual([
      "Mode / model",
      "Setup conversation",
      "Last PR",
      "Base",
      "Branch",
      "Chain mode",
      "Completion signal",
    ]);
    // The config PR row reuses the shared PR link for the newest run with a PR.
    expect(
      within(execution).getByTestId("automation-config-pr-link"),
    ).toHaveTextContent("PR #593");
    const limits = within(configPanel).getByTestId("automation-config-group-limits");
    expect(
      within(limits).getAllByRole("term").map((term) => term.textContent),
    ).toEqual(["Max runs", "Max failures"]);
    const usageGroup = within(configPanel).getByTestId("automation-config-group-usage");
    expect(
      within(usageGroup).getAllByRole("term").map((term) => term.textContent),
    ).toEqual(["Input tokens", "Output tokens", "Cache tokens", "Estimated cost"]);
    const timestamps = within(configPanel).getByTestId("automation-config-timestamps");
    expect(timestamps).toHaveTextContent("Created");
    expect(timestamps).toHaveTextContent("Updated");
    expect(configPanel).toHaveTextContent("Paused: release_freeze - Waiting on base branch");
    expect(within(card).queryByText("No spec linked yet.")).not.toBeInTheDocument();

    await user.click(within(card).getByRole("tab", { name: "Spec" }));
    expect(within(card).getByText("No spec linked yet.")).toBeInTheDocument();
    expect(configPanel).not.toBeVisible();

    await user.click(within(card).getByRole("tab", { name: "Inputs" }));
    expect(
      within(card).getByText("No setup input references are attached to this automation record."),
    ).toBeInTheDocument();
  });

  it("marks populated Spec and Inputs tabs without mounting spec content before selection", async () => {
    useArtifactMock.mockReturnValue({
      data: {
        id: "artifact-spec-1",
        name: "Migration loop spec",
        artifact_type: "specification",
        content_type: "inline",
        content: "## Phase 1\nBuild it.",
        created_at: "2026-07-05T00:00:00Z",
        created_by: "setup-agent",
        version: 1,
        bucket_id: null,
        task_id: null,
        process_id: null,
        derived_from: [],
      },
      isLoading: false,
      isError: false,
    });
    renderDetail({
      automation: automation({ specArtifactId: "artifact-spec-1" }),
      runs: [run()],
      usage,
    });

    const card = await screen.findByTestId("automation-details-card");
    expect(within(card).getByRole("tab", { name: "Spec" })).toHaveTextContent("•");
    expect(within(card).getByRole("tab", { name: "Inputs (1)" })).toBeInTheDocument();
    expect(useArtifactMock).not.toHaveBeenCalled();
  });

  it("calls pause and skip-judge controls through the automation API", async () => {
    const latestRun = run({
      id: "run-2",
      runIndex: 2,
      status: "merged",
      judgeState: "none",
    });
    renderDetail({ automation: automation(), runs: [latestRun], usage });

    await screen.findByTestId("automation-detail-view");

    await userEvent.click(screen.getByLabelText("Pause automation"));
    await waitFor(() =>
      expect(pauseAutomationMock).toHaveBeenCalledWith({
        id: "automation-1",
        reasonCode: "user",
        reasonDetail: "Paused from Automations detail",
      }),
    );

    await userEvent.click(screen.getByLabelText("More automation actions"));
    await userEvent.click(screen.getByText("Skip judge"));

    await waitFor(() =>
      expect(skipJudgeMock).toHaveBeenCalledWith({
        id: "automation-1",
        runId: "run-2",
      }),
    );
  });

  it("renders loading and error states", async () => {
    getAutomationMock.mockReturnValue(new Promise(() => {}));

    renderDetailWithQuery();

    expect(screen.getByTestId("automation-detail-loading")).toBeInTheDocument();

    getAutomationMock.mockReset().mockRejectedValue(new Error("boom"));
    renderDetailWithQuery();

    expect(await screen.findByText("Could not load automation.")).toBeInTheDocument();
  });

  it("resumes paused automation and opens the setup conversation from the action menu", async () => {
    const user = userEvent.setup();
    const onOpenRunConversation = vi.fn();
    renderDetail(
      {
        automation: automation({
          status: "paused",
          pausedReasonCode: "release_freeze",
          pausedReasonDetail: "Waiting on base branch",
          baseSourcePullRequestJson: null,
          goalItemsJson: "not-json",
        }),
        runs: [],
        usage: { ...usage, estimatedUsd: null },
      },
      onOpenRunConversation,
    );

    await screen.findByTestId("automation-detail-view");
    expect(screen.getByText("Paused: release_freeze - Waiting on base branch")).toBeInTheDocument();
    // estimatedUsd is null → the Estimated cost row is omitted instead of
    // rendering a "Not recorded" placeholder.
    expect(screen.queryByText("Estimated cost")).not.toBeInTheDocument();
    expect(screen.queryByText("Not recorded")).not.toBeInTheDocument();
    await user.click(screen.getByRole("tab", { name: "Inputs" }));
    expect(screen.getByText("No setup input references are attached to this automation record.")).toBeInTheDocument();

    await openRunsTab();
    expect(screen.getByText("No runs have been created yet.")).toBeInTheDocument();

    await user.click(screen.getByLabelText("Resume automation"));

    await waitFor(() => expect(resumeAutomationMock).toHaveBeenCalledWith("automation-1"));
    expect(toastSuccessMock).toHaveBeenCalledWith("Automation resumed");

    await user.click(screen.getByLabelText("More automation actions"));
    await user.click(screen.getByText("Edit"));

    expect(onOpenRunConversation).toHaveBeenCalledWith("project-1", "setup-conversation-1");
  });

  it("reports run-now deferred outcomes without scheduling a run", async () => {
    const user = userEvent.setup();
    triggerRunNowMock.mockResolvedValueOnce({
      scheduled: false,
      reason: "run in flight",
    });
    renderDetail({
      automation: automation({
        baseSourcePullRequestJson: "not-json",
        goalItemsJson: null,
      }),
      runs: [run({
        diffStatsJson: null,
        judgeVerdictJson: "not-json",
      })],
      usage,
    });

    await screen.findByTestId("automation-detail-view");
    expect(screen.queryByText("Diff not recorded")).not.toBeInTheDocument();
    await user.click(screen.getByRole("tab", { name: "Inputs" }));
    expect(screen.getByText("No setup input references are attached to this automation record.")).toBeInTheDocument();

    await user.click(screen.getByLabelText("Run now"));

    await waitFor(() => expect(triggerRunNowMock).toHaveBeenCalledWith("automation-1"));
    expect(toastInfoMock).toHaveBeenCalledWith("run in flight");
  });

  it("explains automation cancellation before the destructive mutation", async () => {
    renderDetail({ automation: automation(), runs: [run()], usage });

    await screen.findByTestId("automation-detail-view");
    await userEvent.click(screen.getByLabelText("Cancel automation"));

    expect(await screen.findByText("Cancel automation?")).toBeInTheDocument();
    expect(screen.getByText(/cancels any open run/i)).toBeInTheDocument();
    expect(screen.getByText(/stops automatic scheduling/i)).toBeInTheDocument();
    expect(screen.getByText(/work, artifacts, conversations, branches, and PRs/i)).toBeInTheDocument();
    expect(screen.getByText(/cancelled run cannot be resumed/i)).toBeInTheDocument();
    expect(screen.getByText(/Restart creates a new run/i)).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Cancel automation" }));

    await waitFor(() => expect(stopAutomationMock).toHaveBeenCalledWith("automation-1"));
    expect(toastSuccessMock).toHaveBeenCalledWith("Automation cancelled");
  });

  it("restarts a stopped automation with a fresh run", async () => {
    renderDetail({
      automation: automation({ status: "stopped" }),
      runs: [run({ status: "cancelled", judgeState: "none" })],
      usage,
    });

    await screen.findByTestId("automation-detail-view");
    await userEvent.click(screen.getByRole("button", { name: "Restart automation" }));

    await waitFor(() => expect(restartAutomationMock).toHaveBeenCalledWith("automation-1"));
    expect(toastSuccessMock).toHaveBeenCalledWith("Automation restarted with a new run");
    expect(screen.getByLabelText("Run now")).toBeDisabled();
  });

  it("reports deferred restart and judge-retry outcomes", async () => {
    restartAutomationMock.mockResolvedValueOnce({
      scheduled: false,
      reason: "restart prerequisites changed",
    });
    const restartView = renderDetail({
      automation: automation({ status: "stopped" }),
      runs: [run({ status: "cancelled", judgeState: "none" })],
      usage,
    });

    await userEvent.click(
      await screen.findByRole("button", { name: "Restart automation" }),
    );
    await waitFor(() =>
      expect(toastInfoMock).toHaveBeenCalledWith("restart prerequisites changed"),
    );
    restartView.unmount();

    retryJudgeMock.mockResolvedValueOnce({
      scheduled: false,
      reason: "terminal judge already retried",
    });
    const terminalJudgeView = renderDetail({
      automation: automation(),
      runs: [run({ status: "completed", judgeState: "failed" })],
      usage,
    });

    await screen.findByTestId("automation-detail-view");
    await openRunsTab();
    await userEvent.click(
      await screen.findByRole("button", { name: "Retry terminal judge" }),
    );
    await waitFor(() =>
      expect(toastInfoMock).toHaveBeenCalledWith("terminal judge already retried"),
    );
    terminalJudgeView.unmount();

    retryPlanJudgeMock.mockResolvedValueOnce({
      scheduled: false,
      reason: "plan judge already retried",
    });
    renderDetail({
      automation: automation({ planApprovalMode: "automatic" }),
      runs: [
        run({
          status: "awaiting_plan_approval",
          planJudgeState: "failed",
          planArtifactId: "plan-1",
        }),
      ],
      usage,
    });

    await screen.findByTestId("automation-detail-view");
    await openRunsTab();
    await userEvent.click(
      await screen.findByRole("button", { name: "Retry plan judge" }),
    );
    await waitFor(() =>
      expect(toastInfoMock).toHaveBeenCalledWith("plan judge already retried"),
    );
  });

  it("reports a rejected automation cancellation", async () => {
    stopAutomationMock.mockRejectedValueOnce(new Error("cancel failed"));
    renderDetail({ automation: automation(), runs: [run()], usage });

    await userEvent.click(await screen.findByLabelText("Cancel automation"));
    await userEvent.click(
      await screen.findByRole("button", { name: "Cancel automation" }),
    );

    await waitFor(() =>
      expect(toastErrorMock).toHaveBeenCalledWith("Failed to cancel automation"),
    );
  });

  it.each([
    {
      name: "restart",
      actionName: "Restart automation",
      apiMock: restartAutomationMock,
      errorMessage: "Failed to restart automation",
      detail: {
        automation: automation({ status: "stopped" }),
        runs: [run({ status: "cancelled", judgeState: "none" })],
        usage,
      },
    },
    {
      name: "terminal judge retry",
      actionName: "Retry terminal judge",
      apiMock: retryJudgeMock,
      errorMessage: "Failed to retry terminal judge",
      detail: {
        automation: automation(),
        runs: [run({ status: "completed", judgeState: "failed" })],
        usage,
      },
    },
    {
      name: "plan judge retry",
      actionName: "Retry plan judge",
      apiMock: retryPlanJudgeMock,
      errorMessage: "Failed to retry plan judge",
      detail: {
        automation: automation({ planApprovalMode: "automatic" }),
        runs: [
          run({
            status: "awaiting_plan_approval",
            planJudgeState: "failed",
            planArtifactId: "plan-1",
          }),
        ],
        usage,
      },
    },
  ])("reports a rejected $name action", async ({ actionName, apiMock, detail, errorMessage }) => {
    apiMock.mockRejectedValueOnce(new Error(`${actionName} failed`));
    renderDetail(detail);

    await screen.findByTestId("automation-detail-view");
    if (actionName.startsWith("Retry")) {
      await openRunsTab();
    }
    await userEvent.click(await screen.findByRole("button", { name: actionName }));

    await waitFor(() => expect(toastErrorMock).toHaveBeenCalledWith(errorMessage));
  });

  it("separates plan-judge recovery from terminal-judge recovery", async () => {
    const rendered = renderDetail({
      automation: automation({ planApprovalMode: "automatic" }),
      runs: [
        run({
          status: "awaiting_plan_approval",
          planJudgeState: "failed",
          judgeState: "none",
          planArtifactId: "plan-1",
        }),
      ],
      usage,
    });

    await screen.findByTestId("automation-detail-view");
    await openRunsTab();
    expect(await screen.findAllByText("Plan judge failed")).not.toHaveLength(0);
    await userEvent.click(screen.getByRole("button", { name: "Retry plan judge" }));
    await waitFor(() => expect(retryPlanJudgeMock).toHaveBeenCalledWith("automation-1"));

    rendered.unmount();
    renderDetail({
      automation: automation(),
      runs: [run({ status: "completed", judgeState: "failed" })],
      usage,
    });

    await screen.findByTestId("automation-detail-view");
    await openRunsTab();
    expect(await screen.findAllByText("Terminal judge failed")).not.toHaveLength(0);
    await userEvent.click(screen.getByRole("button", { name: "Retry terminal judge" }));
    await waitFor(() => expect(retryJudgeMock).toHaveBeenCalledWith("automation-1"));
  });

  it("shows pending judge states without offering an ineligible retry", async () => {
    const rendered = renderDetail({
      automation: automation({ planApprovalMode: "automatic" }),
      runs: [
        run({
          status: "awaiting_plan_approval",
          planJudgeState: "none",
          judgeState: "none",
          planArtifactId: "plan-1",
        }),
      ],
      usage,
    });

    expect(await screen.findAllByText("Plan judge pending")).not.toHaveLength(0);
    expect(
      screen.queryByRole("button", { name: "Retry plan judge" }),
    ).not.toBeInTheDocument();

    rendered.unmount();
    renderDetail({
      automation: automation(),
      runs: [run({ status: "completed", judgeState: "none" })],
      usage,
    });

    expect(await screen.findAllByText("Terminal judge pending")).not.toHaveLength(0);
    expect(
      screen.queryByRole("button", { name: "Retry terminal judge" }),
    ).not.toBeInTheDocument();
  });

  it("does not run paused automations when the resume confirmation is canceled", async () => {
    renderDetail({
      automation: automation({ status: "paused" }),
      runs: [run({ status: "published", judgeState: "none" })],
      usage,
    });

    await screen.findByTestId("automation-detail-view");
    await userEvent.click(screen.getByLabelText("Run now"));
    await userEvent.click(await screen.findByRole("button", { name: "Cancel" }));

    expect(triggerRunNowMock).not.toHaveBeenCalled();
  });

  it("confirms paused run-now before scheduling the override", async () => {
    renderDetail({
      automation: automation({ status: "paused" }),
      runs: [run({ status: "published", judgeState: "none" })],
      usage,
    });

    await screen.findByTestId("automation-detail-view");
    await userEvent.click(screen.getByLabelText("Run now"));

    expect(await screen.findByText("Resume and run now?")).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Resume and run" }));

    await waitFor(() => expect(triggerRunNowMock).toHaveBeenCalledWith("automation-1"));
    expect(toastSuccessMock).toHaveBeenCalledWith("Automation run scheduled");
  });

  it("shows the Approve button only for draft automations", async () => {
    const draftView = renderDetail({
      automation: automation({ status: "draft" }),
      runs: [],
      usage,
    });

    await screen.findByTestId("automation-detail-view");
    expect(screen.getByRole("button", { name: "Approve" })).toBeInTheDocument();
    draftView.unmount();

    renderDetail({ automation: automation({ status: "active" }), runs: [], usage });

    await screen.findByTestId("automation-detail-view");
    expect(screen.queryByRole("button", { name: "Approve" })).not.toBeInTheDocument();
  });

  it("approves a draft automation and invalidates the detail query", async () => {
    const queryClient = new QueryClient({
      defaultOptions: {
        queries: { retry: false, gcTime: 0 },
        mutations: { retry: false },
      },
    });
    const invalidateSpy = vi.spyOn(queryClient, "invalidateQueries");
    getAutomationMock.mockResolvedValue({
      automation: automation({ status: "draft" }),
      runs: [],
      usage,
    });

    render(
      <QueryClientProvider client={queryClient}>
        <TooltipProvider>
          <AutomationDetailView
            automationId="automation-1"
            projectId="project-1"
            projectName="Demo Project"
            onBack={vi.fn()}
            onOpenRunConversation={vi.fn()}
          />
        </TooltipProvider>
      </QueryClientProvider>,
    );

    await screen.findByTestId("automation-detail-view");
    await userEvent.click(screen.getByRole("button", { name: "Approve" }));

    await waitFor(() => expect(finalizeAutomationMock).toHaveBeenCalledWith("automation-1"));
    expect(toastSuccessMock).toHaveBeenCalledWith("Automation spec approved");
    await waitFor(() =>
      expect(invalidateSpy).toHaveBeenCalledWith(
        expect.objectContaining({
          queryKey: ["automations", "detail", "automation-1"],
        }),
      ),
    );
  });

  it("surfaces the backend validation message when approval fails", async () => {
    finalizeAutomationMock.mockReset().mockRejectedValue(
      "automation goal_prompt is required before approval",
    );
    renderDetail({
      automation: automation({ status: "draft" }),
      runs: [],
      usage,
    });

    await screen.findByTestId("automation-detail-view");
    await userEvent.click(screen.getByRole("button", { name: "Approve" }));

    await waitFor(() => expect(finalizeAutomationMock).toHaveBeenCalledWith("automation-1"));
    expect(toastErrorMock).toHaveBeenCalledWith(
      "automation goal_prompt is required before approval",
    );
  });

  it("renders the failure reason for a failed run in the timeline", async () => {
    renderDetail({
      automation: automation(),
      runs: [
        run({
          id: "run-failed",
          status: "agent_failed",
          judgeState: "none",
          errorCode: "publish_failed",
          errorDetail: "Publish step exited with code 1",
        }),
      ],
      usage,
    });

    await screen.findByTestId("automation-detail-view");
    await openRunsTab();

    const failure = screen.getByTestId("automation-run-run-failed-failure");
    expect(failure).toHaveTextContent("Publish step exited with code 1");
  });

  it("deletes the latest failed run after confirmation and invalidates automation queries", async () => {
    const queryClient = new QueryClient({
      defaultOptions: {
        queries: { retry: false, gcTime: 0 },
        mutations: { retry: false },
      },
    });
    const invalidateSpy = vi.spyOn(queryClient, "invalidateQueries");
    getAutomationMock.mockResolvedValue({
      automation: automation(),
      runs: [run({ id: "run-failed", runIndex: 7, status: "agent_failed" })],
      usage,
    });
    render(
      <QueryClientProvider client={queryClient}>
        <TooltipProvider>
          <AutomationDetailView
            automationId="automation-1"
            projectId="project-1"
            projectName="Demo Project"
            onBack={vi.fn()}
            onOpenRunConversation={vi.fn()}
          />
        </TooltipProvider>
      </QueryClientProvider>,
    );

    await screen.findByTestId("automation-detail-view");
    await openRunsTab();
    await userEvent.click(screen.getByTestId("automation-run-run-failed-delete"));

    expect(screen.getByText("Delete run?")).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "Delete" }));

    await waitFor(() =>
      expect(deleteRunMock).toHaveBeenCalledWith({
        id: "automation-1",
        runId: "run-failed",
      }),
    );
    expect(toastSuccessMock).toHaveBeenCalledWith("Run deleted");
    await waitFor(() =>
      expect(invalidateSpy).toHaveBeenCalledWith(
        expect.objectContaining({
          queryKey: ["automations", "detail", "automation-1"],
        }),
      ),
    );
  });

  it("does not delete the latest failed run when confirmation is canceled", async () => {
    renderDetail({
      automation: automation(),
      runs: [run({ id: "run-failed", runIndex: 7, status: "agent_failed" })],
      usage,
    });

    await screen.findByTestId("automation-detail-view");
    await openRunsTab();
    await userEvent.click(screen.getByTestId("automation-run-run-failed-delete"));
    await userEvent.click(screen.getByRole("button", { name: "Cancel" }));

    expect(deleteRunMock).not.toHaveBeenCalled();
  });

  it("renders fallback run metadata and deletes terminal automations after confirmation", async () => {
    const user = userEvent.setup();
    const onBack = vi.fn();
    const queryClient = new QueryClient({
      defaultOptions: {
        queries: { retry: false, gcTime: 0 },
        mutations: { retry: false },
      },
    });
    getAutomationMock.mockResolvedValue({
      automation: automation({
        status: "stopped",
        setupConversationId: null,
        baseDisplayName: null,
        baseRef: "",
        baseRefKind: "current_branch",
        baseSourcePullRequestJson: JSON.stringify({ number: 41 }),
        goalItemsJson: JSON.stringify([{ id: "item-fallback" }, "ignored"]),
        createdAt: "not-a-date",
        updatedAt: "not-a-date",
      }),
      runs: [
        run({
          id: "run-fallback",
          status: "cancelled",
          judgeState: "failed",
          conversationId: null,
          promptAuthor: "skip_judge_template",
          baseRefUsed: "",
          baseRefKind: "local_branch",
          prNumber: 41,
          prUrl: null,
          diffStatsJson: JSON.stringify({ files_changed: 2 }),
          agentSummary: null,
          judgeVerdictJson: null,
          finishedAt: null,
          updatedAt: "invalid-date",
        }),
      ],
      usage: {
        inputTokens: 0,
        outputTokens: 0,
        cacheCreationTokens: 0,
        cacheReadTokens: 0,
        estimatedUsd: null,
      },
    });

    render(
      <QueryClientProvider client={queryClient}>
        <TooltipProvider>
          <AutomationDetailView
            automationId="automation-1"
            projectId="project-1"
            projectName={null}
            onBack={onBack}
            onOpenRunConversation={vi.fn()}
          />
        </TooltipProvider>
      </QueryClientProvider>,
    );

    await screen.findByTestId("automation-detail-view");
    expect(screen.getAllByText("Stopped").length).toBeGreaterThanOrEqual(1);
    expect(screen.getByText("current_branch")).toBeInTheDocument();
    expect(screen.getByText("Phase 1")).toBeInTheDocument();
    // "Not recorded" survives only in the intentional Setup conversation slot;
    // run-card facts omit empty fields entirely.
    const configPanel = screen.getByTestId("automation-config-panel");
    expect(within(configPanel).getByText("Not recorded")).toBeInTheDocument();
    await user.click(screen.getByRole("tab", { name: "Inputs (1)" }));
    expect(screen.getByText("PR #41")).toBeInTheDocument();
    expect(screen.getByLabelText("Run now")).toBeDisabled();
    expect(screen.getByLabelText("Cancel automation")).toBeDisabled();

    await openRunsTab();
    expect(screen.getByText("2 files, +0 / -0")).toBeInTheDocument();
    expect(screen.getByText("invalid-date")).toBeInTheDocument();
    expect(screen.getByText("Skip-judge template")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Not started" })).toBeDisabled();
    expect(
      within(screen.getByTestId("automation-run-run-fallback-card")).queryByText("Not recorded"),
    ).not.toBeInTheDocument();

    await user.click(screen.getByLabelText("More automation actions"));
    await user.click(screen.getByText("Delete"));

    expect(await screen.findByText("Delete automation?")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Delete" }));

    await waitFor(() => expect(deleteAutomationMock).toHaveBeenCalledWith("automation-1"));
    expect(toastSuccessMock).toHaveBeenCalledWith("Automation deleted");
    expect(onBack).toHaveBeenCalled();
  });

  it("allows deleting draft automations and lists the archive inventory in the confirm dialog", async () => {
    renderDetail({
      automation: automation({
        status: "draft",
        setupConversationId: "setup-conversation-1",
        specArtifactId: "spec-1",
      }),
      runs: [
        run({
          id: "run-open",
          status: "published",
          judgeState: "none",
          conversationId: "conversation-open",
          prNumber: 777,
          prMergedAt: null,
        }),
      ],
      usage,
    });

    await screen.findByTestId("automation-detail-view");

    await userEvent.click(screen.getByLabelText("More automation actions"));
    const deleteItem = screen.getByText("Delete");
    expect(deleteItem.closest("[role='menuitem']")).not.toHaveAttribute(
      "aria-disabled",
      "true",
    );
    await userEvent.click(deleteItem);

    expect(await screen.findByText("Delete automation?")).toBeInTheDocument();
    expect(
      screen.getByText(/Archives the setup conversation and 1 run conversation\./),
    ).toBeInTheDocument();
    expect(screen.getByText(/Closes 1 open PR\./)).toBeInTheDocument();
    expect(screen.getByText(/Archives the linked spec\./)).toBeInTheDocument();
    expect(
      screen.getByText(/Permanently removes the automation and its run history\./),
    ).toBeInTheDocument();

    await userEvent.click(screen.getByRole("button", { name: "Delete" }));
    await waitFor(() =>
      expect(deleteAutomationMock).toHaveBeenCalledWith("automation-1"),
    );
  });

  it("shows a live run chip and blocks Run now while a run is in progress", async () => {
    renderDetail({
      automation: automation({ status: "active" }),
      runs: [run({ id: "run-live", runIndex: 1, status: "running", judgeState: "none" })],
      usage,
    });

    await screen.findByTestId("automation-detail-view");

    const chip = screen.getByTestId("automation-run-status-chip");
    expect(chip).toHaveTextContent("Run 1 in progress");

    const runNow = screen.getByLabelText("Run now");
    expect(runNow).toBeDisabled();
    expect(runNow).toHaveAttribute(
      "title",
      expect.stringContaining("Run 1 in progress"),
    );

    // Stop stays enabled for an active automation with a live run.
    expect(screen.getByLabelText("Cancel automation")).not.toBeDisabled();
    expect(screen.getByLabelText("Pause automation")).not.toBeDisabled();
  });

  it("shows a judging chip and blocks Run now while the judge is running", async () => {
    renderDetail({
      automation: automation({ status: "active" }),
      runs: [run({ id: "run-judge", runIndex: 2, status: "merged", judgeState: "in_progress" })],
      usage,
    });

    await screen.findByTestId("automation-detail-view");

    expect(screen.getByTestId("automation-run-status-chip")).toHaveTextContent(
      "Terminal judge running",
    );
    expect(screen.getByLabelText("Run now")).toBeDisabled();
  });

  it("enables Run now and hides the live chip for an active automation with no open run", async () => {
    renderDetail({
      automation: automation({ status: "active" }),
      runs: [run({ id: "run-done", runIndex: 1, status: "merged", judgeState: "done" })],
      usage,
    });

    await screen.findByTestId("automation-detail-view");

    expect(screen.queryByTestId("automation-run-status-chip")).not.toBeInTheDocument();
    expect(screen.getByLabelText("Run now")).not.toBeDisabled();
  });

  it("surfaces an idle-after-cancelled banner and runs the automation from its CTA", async () => {
    renderDetail({
      automation: automation({ status: "active" }),
      runs: [run({ id: "run-cancelled", status: "cancelled", judgeState: "none" })],
      usage,
    });

    await screen.findByTestId("automation-detail-view");
    await openRunsTab();
    const banner = await screen.findByTestId("automation-idle-after-cancelled");
    expect(banner).toHaveTextContent(
      "The last run was cancelled. Run now starts a new run from that run's prompt; it does not resume the cancelled run.",
    );

    await userEvent.click(within(banner).getByRole("button", { name: "Run now" }));

    await waitFor(() => expect(triggerRunNowMock).toHaveBeenCalledWith("automation-1"));
  });

  it("hides the idle-after-cancelled banner for paused and running automations", async () => {
    const pausedView = renderDetail({
      automation: automation({ status: "paused" }),
      runs: [run({ id: "run-paused-cancelled", status: "cancelled", judgeState: "none" })],
      usage,
    });

    await screen.findByTestId("automation-detail-view");
    await openRunsTab();
    expect(screen.queryByTestId("automation-idle-after-cancelled")).not.toBeInTheDocument();
    pausedView.unmount();

    renderDetail({
      automation: automation({ status: "active" }),
      runs: [run({ id: "run-running", status: "running", judgeState: "none" })],
      usage,
    });

    await screen.findByTestId("automation-detail-view");
    await openRunsTab();
    expect(screen.queryByTestId("automation-idle-after-cancelled")).not.toBeInTheDocument();
  });

  it("disables delete for active automations", async () => {
    renderDetail({
      automation: automation({ status: "active" }),
      runs: [run()],
      usage,
    });

    await screen.findByTestId("automation-detail-view");

    await userEvent.click(screen.getByLabelText("More automation actions"));
    expect(screen.getByText("Delete").closest("[role='menuitem']")).toHaveAttribute(
      "aria-disabled",
      "true",
    );
  });

  describe("run card progressive disclosure", () => {
    it("orders judge verdict and agent summary before the run prompt toggle", async () => {
      renderDetail({ automation: automation(), runs: [run()], usage });

      await screen.findByTestId("automation-detail-view");
    await openRunsTab();

      const body = screen.getByTestId("automation-run-run-1-body");
      const judge = within(body).getByTestId("automation-run-run-1-judge");
      const summary = within(body).getByTestId("automation-run-run-1-summary");
      const promptToggle = within(body).getByTestId("automation-run-run-1-prompt-toggle");
      expect(
        judge.compareDocumentPosition(summary) & Node.DOCUMENT_POSITION_FOLLOWING,
      ).toBeTruthy();
      expect(
        summary.compareDocumentPosition(promptToggle) & Node.DOCUMENT_POSITION_FOLLOWING,
      ).toBeTruthy();
      // The teaser is a collapsed-card affordance only.
      expect(
        screen.queryByTestId("automation-run-run-1-summary-teaser"),
      ).not.toBeInTheDocument();
    });

    it("keeps the run prompt fully closed until toggled", async () => {
      renderDetail({ automation: automation(), runs: [run()], usage });

      await screen.findByTestId("automation-detail-view");
    await openRunsTab();

      expect(screen.queryByText(/Prompt line 1/)).not.toBeInTheDocument();

      await userEvent.click(screen.getByTestId("automation-run-run-1-prompt-toggle"));
      expect(screen.getByText(/Prompt line 1/)).toBeInTheDocument();

      await userEvent.click(screen.getByTestId("automation-run-run-1-prompt-toggle"));
      expect(screen.queryByText(/Prompt line 1/)).not.toBeInTheDocument();
    });

    it("omits placeholder cells for empty run facts", async () => {
      renderDetail({
        automation: automation(),
        runs: [
          run({
            id: "run-empty",
            prNumber: null,
            prUrl: null,
            prMergedAt: null,
            mergeCommitSha: null,
            diffStatsJson: null,
            finishedAt: null,
          }),
        ],
        usage,
      });

      await screen.findByTestId("automation-detail-view");
    await openRunsTab();

      const card = screen.getByTestId("automation-run-run-empty-card");
      expect(within(card).queryByText("Not recorded")).not.toBeInTheDocument();
      expect(within(card).queryByText("Not published")).not.toBeInTheDocument();
      expect(within(card).queryByText("Diff not recorded")).not.toBeInTheDocument();
      expect(
        within(card).queryByTestId("automation-run-run-empty-pr-state"),
      ).not.toBeInTheDocument();
    });

    it("keeps run facts reachable in the compact facts row", async () => {
      renderDetail({ automation: automation(), runs: [run()], usage });

      await screen.findByTestId("automation-detail-view");
    await openRunsTab();

      const body = screen.getByTestId("automation-run-run-1-body");
      expect(within(body).getByTestId("automation-run-run-1-pr-state")).toHaveTextContent(
        "PR #593",
      );
      expect(within(body).getByText("3 files, +12 / -4")).toBeInTheDocument();
      expect(within(body).getByText("Setup agent")).toBeInTheDocument();
      expect(
        within(body).getByRole("button", { name: "Open conversation" }),
      ).toBeInTheDocument();
    });

    it("shows a failed run's reason and summary teaser while collapsed", async () => {
      renderDetail({
        automation: automation(),
        runs: [
          run({ id: "run-latest", runIndex: 2 }),
          run({
            id: "run-old-failed",
            runIndex: 1,
            status: "agent_failed",
            judgeState: "done",
            errorCode: "publish_failed",
            errorDetail: "Publish step exited with code 1",
            agentSummary: "Attempted the migration but publish failed.",
          }),
        ],
        usage,
      });

      await screen.findByTestId("automation-detail-view");
    await openRunsTab();

      expect(
        screen.queryByTestId("automation-run-run-old-failed-body"),
      ).not.toBeInTheDocument();
      expect(screen.getByTestId("automation-run-run-old-failed-failure")).toHaveTextContent(
        "Publish step exited with code 1",
      );
      expect(
        screen.getByTestId("automation-run-run-old-failed-summary-teaser"),
      ).toHaveTextContent("Attempted the migration but publish failed.");
    });

    it("suppresses the summary teaser while a run is open", async () => {
      renderDetail({
        automation: automation({ status: "active" }),
        runs: [
          run({
            id: "run-live",
            status: "running",
            judgeState: "none",
            finishedAt: null,
            agentSummary: "Partial work so far.",
          }),
        ],
        usage,
      });

      await screen.findByTestId("automation-detail-view");
    await openRunsTab();
      await userEvent.click(screen.getByRole("button", { name: "Collapse run 1" }));

      expect(
        screen.queryByTestId("automation-run-run-live-summary-teaser"),
      ).not.toBeInTheDocument();
    });
  });
});
