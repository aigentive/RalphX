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
  stopAutomationMock,
  triggerRunNowMock,
  skipJudgeMock,
  deleteAutomationMock,
  toastSuccessMock,
  toastErrorMock,
  toastInfoMock,
} = vi.hoisted(() => ({
  getAutomationMock: vi.fn(),
  pauseAutomationMock: vi.fn(),
  resumeAutomationMock: vi.fn(),
  stopAutomationMock: vi.fn(),
  triggerRunNowMock: vi.fn(),
  skipJudgeMock: vi.fn(),
  deleteAutomationMock: vi.fn(),
  toastSuccessMock: vi.fn(),
  toastErrorMock: vi.fn(),
  toastInfoMock: vi.fn(),
}));

vi.mock("sonner", () => ({
  toast: {
    success: toastSuccessMock,
    error: toastErrorMock,
    info: toastInfoMock,
  },
}));

vi.mock("@/api/automations", () => ({
  automationsApi: {
    get: getAutomationMock,
    pause: pauseAutomationMock,
    resume: resumeAutomationMock,
    stop: stopAutomationMock,
    triggerRunNow: triggerRunNowMock,
    skipJudge: skipJudgeMock,
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
    conversationId: "conversation-1",
    runPrompt: Array.from({ length: 12 }, (_, index) => `Prompt line ${index + 1}`).join("\n"),
    promptAuthor: "setup_agent",
    baseRefKind: "project_default",
    baseRefUsed: "main",
    baseFromRunId: null,
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

function renderDetail(detail: AutomationDetail, onOpenRunConversation = vi.fn()) {
  const queryClient = new QueryClient({
    defaultOptions: {
      queries: { retry: false, gcTime: 0 },
      mutations: { retry: false },
    },
  });
  getAutomationMock.mockResolvedValue(detail);
  return {
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
          />
        </TooltipProvider>
      </QueryClientProvider>,
    ),
  };
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
    stopAutomationMock.mockReset().mockResolvedValue(automation({ status: "stopped" }));
    triggerRunNowMock.mockReset().mockResolvedValue({ scheduled: true, reason: null });
    skipJudgeMock.mockReset().mockResolvedValue({ scheduled: true, reason: null });
    deleteAutomationMock.mockReset().mockResolvedValue(undefined);
    toastSuccessMock.mockReset();
    toastErrorMock.mockReset();
    toastInfoMock.mockReset();
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

    renderDetail({ automation: automation(), runs: [olderRun, latestRun] });

    expect(await screen.findByTestId("automation-detail-view")).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Ship migration loop" })).toBeInTheDocument();
    expect(screen.getByLabelText("Pause automation")).toBeInTheDocument();
    expect(screen.getByLabelText("Run now")).toBeInTheDocument();
    expect(screen.getByLabelText("Stop automation")).toBeInTheDocument();

    const runTwo = screen.getByTestId("automation-run-run-2");
    const runOne = screen.getByTestId("automation-run-run-1");
    expect(runTwo.compareDocumentPosition(runOne)).toBe(Node.DOCUMENT_POSITION_FOLLOWING);
    expect(within(runTwo).getByText("Run 2")).toBeInTheDocument();
    const goalCard = screen.getByTestId("automation-goal-card");
    expect(goalCard).toHaveTextContent("Goal line 10");
    expect(goalCard).not.toHaveTextContent("Goal line 11");

    await userEvent.click(screen.getAllByRole("button", { name: "Expand" })[0]);

    expect(goalCard).toHaveTextContent("Goal line 11");
  });

  it("calls pause and skip-judge controls through the automation API", async () => {
    const latestRun = run({
      id: "run-2",
      runIndex: 2,
      status: "merged",
      judgeState: "none",
    });
    renderDetail({ automation: automation(), runs: [latestRun] });

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
});
