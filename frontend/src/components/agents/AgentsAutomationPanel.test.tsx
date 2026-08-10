import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import {
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import type {
  Automation,
  AutomationDetail,
  AutomationRun,
} from "@/api/automations";
import { TooltipProvider } from "@/components/ui/tooltip";
import { AGENT_CONTROL_DISABLED_HINT } from "@/lib/remote/agent-gate";
import {
  LOCAL_ENVIRONMENT_ID,
  useEnvironmentStore,
} from "@/stores/environmentStore";
import { AgentsAutomationPanel } from "./AgentsAutomationPanel";

const {
  useAutomationDetailMock,
  useAutomationEventsMock,
  pauseAutomationMock,
  resumeAutomationMock,
  stopAutomationMock,
  restartAutomationMock,
  triggerRunNowMock,
  retryJudgeMock,
  retryPlanJudgeMock,
  deleteAutomationMock,
  cancelRunMock,
  updateSettingsMock,
  updateAutomationSetupMock,
  sendAgentMessageMock,
  useAskUserQuestionMock,
  submitAutomationSetupAnswerMock,
  useArtifactMock,
  openExternalUrlMock,
  toastSuccessMock,
  toastErrorMock,
  toastInfoMock,
} = vi.hoisted(() => ({
  useAutomationDetailMock: vi.fn(),
  useAutomationEventsMock: vi.fn(),
  pauseAutomationMock: vi.fn(),
  resumeAutomationMock: vi.fn(),
  stopAutomationMock: vi.fn(),
  restartAutomationMock: vi.fn(),
  triggerRunNowMock: vi.fn(),
  retryJudgeMock: vi.fn(),
  retryPlanJudgeMock: vi.fn(),
  deleteAutomationMock: vi.fn(),
  cancelRunMock: vi.fn(),
  updateSettingsMock: vi.fn(),
  updateAutomationSetupMock: vi.fn(),
  sendAgentMessageMock: vi.fn(),
  useAskUserQuestionMock: vi.fn(),
  submitAutomationSetupAnswerMock: vi.fn(),
  useArtifactMock: vi.fn(),
  openExternalUrlMock: vi.fn(),
  toastSuccessMock: vi.fn(),
  toastErrorMock: vi.fn(),
  toastInfoMock: vi.fn(),
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

vi.mock("@/components/agents/agentDeferredFrame", () => ({
  useAfterPaintMounted: () => true,
}));

vi.mock("@/hooks/useAgentModels", () => ({
  useAgentModels: () => ({
    registry: {
      claude: [
        {
          id: "sonnet",
          label: "sonnet",
          menuLabel: "sonnet",
          defaultEffort: "medium",
          supportedEfforts: ["low", "medium", "high", "max"],
        },
      ],
      codex: [
        {
          id: "gpt-5.5",
          label: "gpt-5.5",
          menuLabel: "gpt-5.5",
          defaultEffort: "xhigh",
          supportedEfforts: ["low", "medium", "high", "xhigh"],
        },
        {
          id: "gpt-5.4",
          label: "gpt-5.4",
          menuLabel: "gpt-5.4",
          defaultEffort: "xhigh",
          supportedEfforts: ["low", "medium", "high", "xhigh"],
        },
      ],
    },
  }),
}));

vi.mock("@/hooks/useHarnessProviders", () => ({
  useHarnessProviders: () => ({
    providers: [
      {
        provider: "claude",
        enabled: true,
        available: true,
        cliVersion: "2.1.197",
        status: "ready",
        error: null,
        missingCoreExecFeatures: [],
        supportedModelAliases: ["sonnet"],
        supportedEfforts: ["low", "medium", "high", "max"],
      },
      {
        provider: "codex",
        enabled: true,
        available: true,
        cliVersion: "1.2.3",
        status: "ready",
        error: null,
        missingCoreExecFeatures: [],
        supportedModelAliases: ["gpt-5.5", "gpt-5.4"],
        supportedEfforts: ["low", "medium", "high", "xhigh"],
      },
    ],
    isLoading: false,
    isPlaceholderData: false,
  }),
}));

vi.mock("@/hooks/useAutomations", () => ({
  invalidateAutomationQueries: vi.fn(),
  evictDeletedAutomation: vi.fn(),
  useAutomationDetail: (...args: unknown[]) => useAutomationDetailMock(...args),
  useAutomationEvents: (...args: unknown[]) => useAutomationEventsMock(...args),
}));

vi.mock("@/hooks/useAskUserQuestion", () => ({
  useAskUserQuestion: (...args: unknown[]) => useAskUserQuestionMock(...args),
}));

vi.mock("@/api/chat", () => ({
  sendAgentMessage: (...args: unknown[]) => sendAgentMessageMock(...args),
}));

vi.mock("@/api/automations", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/api/automations")>();
  return {
    ...actual,
    automationsApi: {
      ...actual.automationsApi,
      pause: (...args: unknown[]) => pauseAutomationMock(...args),
      resume: (...args: unknown[]) => resumeAutomationMock(...args),
      stop: (...args: unknown[]) => stopAutomationMock(...args),
      restart: (...args: unknown[]) => restartAutomationMock(...args),
      triggerRunNow: (...args: unknown[]) => triggerRunNowMock(...args),
      retryJudge: (...args: unknown[]) => retryJudgeMock(...args),
      retryPlanJudge: (...args: unknown[]) => retryPlanJudgeMock(...args),
      delete: (...args: unknown[]) => deleteAutomationMock(...args),
      cancelRun: (...args: unknown[]) => cancelRunMock(...args),
      updateSettings: (...args: unknown[]) => updateSettingsMock(...args),
      setupAgent: {
        ...actual.automationsApi.setupAgent,
        updateAutomation: (...args: unknown[]) =>
          updateAutomationSetupMock(...args),
      },
    },
  };
});

const automationFixture = (
  overrides: Partial<Automation> = {},
): Automation => ({
  id: "automation-1",
  projectId: "project-1",
  name: "Release automation",
  status: "active",
  pausedReasonCode: null,
  pausedReasonDetail: null,
  goalPrompt: "Ship the remaining release tasks.",
  setupConversationId: "conversation-setup",
  specArtifactId: null,
  providerHarness: "codex",
  modelId: "gpt-5.4",
  logicalEffort: "medium",
  runMode: "edit",
  baseRefKind: "project_default",
  baseRef: "",
  baseDisplayName: "Project default (main)",
  baseSourcePullRequestJson: null,
  goalItemsJson:
    '[{"id":"phase-1","title":"Build shared context model","status":"pending"}]',
  chainMode: "merged_base",
  completionSignal: "pr_merged",
  planApprovalMode: "manual",
  prMergeMode: "manual",
  planDeepVerification: false,
  maxRuns: 25,
  maxConsecutiveFailures: 3,
  firstRunPrompt: "Build the shared context model in a scoped PR.",
  setupAnalysisSummary: "Configure selected artifact context for chat.",
  createdAt: "2026-07-05T10:00:00Z",
  updatedAt: "2026-07-05T10:00:00Z",
  ...overrides,
});

const automationRunFixture = (
  overrides: Partial<AutomationRun> = {},
): AutomationRun => ({
  id: "run-1",
  automationId: "automation-1",
  runIndex: 3,
  status: "published",
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
  runPrompt: "Continue the release automation.",
  promptAuthor: "judge",
  baseRefKind: "project_default",
  baseRefUsed: "main",
  baseFromRunId: "run-0",
  goalItemId: null,
  branchName: "ralphx/release/agent-1",
  prNumber: 593,
  prUrl: "https://github.com/aigentive/ralphx.app/pull/593",
  prTitle: "Release automation task",
  prHeadRefName: "ralphx/release/agent-1",
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
  startedAt: "2026-07-05T10:00:00Z",
  finishedAt: null,
  createdAt: "2026-07-05T10:00:00Z",
  updatedAt: "2026-07-05T10:00:00Z",
  ...overrides,
});

const automationDetailFixture = (
  overrides: Partial<AutomationDetail> = {},
): AutomationDetail => ({
  automation: automationFixture(),
  runs: [automationRunFixture()],
  usage: {
    inputTokens: 0,
    outputTokens: 0,
    cacheCreationTokens: 0,
    cacheReadTokens: 0,
    estimatedUsd: null,
  },
  ...overrides,
});

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

function renderPanel({
  onOpenAutomation = vi.fn(),
  onFocusAutomationRun,
}: {
  onOpenAutomation?: ((automationId: string) => void) | null;
  onFocusAutomationRun?: (
    automationId: string,
    runId: string,
    conversationId: string,
  ) => void;
} = {}) {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });

  const rendered = render(
    <QueryClientProvider client={queryClient}>
      <TooltipProvider>
        <AgentsAutomationPanel
          automationId="automation-1"
          {...(onOpenAutomation ? { onOpenAutomation } : {})}
          {...(onFocusAutomationRun ? { onFocusAutomationRun } : {})}
        />
      </TooltipProvider>
    </QueryClientProvider>,
  );

  return { onOpenAutomation, unmount: rendered.unmount };
}

describe("AgentsAutomationPanel", () => {
  beforeEach(() => {
    useEnvironmentStore.setState({
      activeEnvironmentId: LOCAL_ENVIRONMENT_ID,
      environments: [
        { id: LOCAL_ENVIRONMENT_ID, name: "This Mac", kind: "local" },
      ],
      effectiveScopes: {},
      connectionPresentations: {},
    });
    vi.clearAllMocks();
    useArtifactMock.mockReturnValue({
      data: null,
      isLoading: false,
      isError: false,
    });
    pauseAutomationMock.mockResolvedValue(
      automationFixture({ status: "paused" }),
    );
    resumeAutomationMock.mockResolvedValue(
      automationFixture({ status: "active" }),
    );
    stopAutomationMock.mockResolvedValue(
      automationFixture({ status: "stopped" }),
    );
    restartAutomationMock.mockResolvedValue({ scheduled: true, reason: null });
    triggerRunNowMock.mockResolvedValue({ scheduled: true, reason: null });
    retryJudgeMock.mockResolvedValue({ scheduled: true, reason: null });
    retryPlanJudgeMock.mockResolvedValue({ scheduled: true, reason: null });
    deleteAutomationMock.mockResolvedValue(undefined);
    cancelRunMock.mockResolvedValue(
      automationRunFixture({ status: "cancelled" }),
    );
    updateSettingsMock.mockResolvedValue(automationFixture({ maxRuns: 8 }));
    updateAutomationSetupMock.mockResolvedValue(
      automationFixture({ status: "draft" }),
    );
    sendAgentMessageMock.mockResolvedValue({
      conversationId: "conversation-setup",
      agentRunId: "run-setup",
      queued: false,
    });
    submitAutomationSetupAnswerMock.mockResolvedValue({
      success: true,
      deliveredToWaitingAgent: true,
    });
    useAskUserQuestionMock.mockReturnValue({
      activeQuestion: null,
      answeredQuestion: undefined,
      submitAnswer: submitAutomationSetupAnswerMock,
      dismissQuestion: vi.fn(),
      clearAnswered: vi.fn(),
      isLoading: false,
    });
    useAutomationDetailMock.mockReturnValue({
      data: automationDetailFixture(),
      isLoading: false,
      isError: false,
    });
    openExternalUrlMock.mockReset().mockResolvedValue(undefined);
  });

  it("shows automation summary controls and opens the automation detail", () => {
    const { onOpenAutomation } = renderPanel();

    expect(screen.getByTestId("agents-automation-panel")).toBeInTheDocument();
    expect(screen.getByText("Release automation")).toBeInTheDocument();
    expect(screen.getByText("Approved")).toBeInTheDocument();
    expect(screen.getByText("3 of 25")).toBeInTheDocument();
    expect(screen.getByText("Current PR #593")).toBeInTheDocument();
    expect(screen.getByTestId("agents-automation-goal")).toHaveTextContent(
      "Ship the remaining release tasks.",
    );
    expect(screen.getByTestId("agents-automation-phases")).toHaveTextContent(
      "Build shared context model",
    );
    expect(screen.getByTestId("agents-automation-spec")).toHaveTextContent(
      "No spec linked yet.",
    );
    expect(
      screen.getByTestId("agents-automation-setup-summary"),
    ).toHaveTextContent("Configure selected artifact context for chat.");
    expect(screen.getByTestId("agents-automation-first-run")).toHaveTextContent(
      "Build the shared context model in a scoped PR.",
    );
    expect(screen.getByTestId("agents-automation-stage")).toHaveTextContent(
      "Waiting for PR #593 to merge",
    );
    expect(
      screen.queryByTestId("agents-automation-failure"),
    ).not.toBeInTheDocument();
    expect(screen.getByTestId("agents-automation-pause")).toBeInTheDocument();
    expect(screen.getByTestId("agents-automation-stop")).toBeInTheDocument();

    fireEvent.click(screen.getByTestId("agents-automation-open"));

    expect(onOpenAutomation).toHaveBeenCalledWith("automation-1");
    expect(useAutomationEventsMock).not.toHaveBeenCalled();
  });

  it("enables registered Run now while Resume follows agent-control scope", () => {
    useEnvironmentStore.setState({
      activeEnvironmentId: "remote-1",
      environments: [{ id: "remote-1", name: "Studio", kind: "remote" }],
      effectiveScopes: { "remote-1": ["ui:read", "ui:operate", "ui:agent"] },
      connectionPresentations: {
        "remote-1": {
          presentation: "connected",
          blockedFailure: null,
          blockedMessage: null,
        },
      },
    });
    // "Run now" renders only via isIdleAfterCancelledRun (active automation + cancelled
    // run); the Resume affordance renders only for a paused automation. The two states are
    // mutually exclusive, so each gets its own render.
    useAutomationDetailMock.mockReturnValue({
      data: automationDetailFixture({
        automation: automationFixture({ status: "active" }),
        runs: [
          automationRunFixture({
            status: "cancelled",
            finishedAt: "2026-07-05T11:00:00Z",
          }),
        ],
      }),
      isLoading: false,
      isError: false,
    });

    const { unmount } = renderPanel();

    // Wave B4 registered request_remote_automation_run, so Run now is reachable with ui:agent.
    expect(screen.getByRole("button", { name: "Run now" })).toBeEnabled();
    expect(screen.getByRole("button", { name: "Run now" })).not.toHaveAttribute(
      "title",
    );

    unmount();
    useAutomationDetailMock.mockReturnValue({
      data: automationDetailFixture({
        automation: automationFixture({ status: "paused" }),
        runs: [
          automationRunFixture({
            status: "cancelled",
            finishedAt: "2026-07-05T11:00:00Z",
          }),
        ],
      }),
      isLoading: false,
      isError: false,
    });
    const withAgentScope = renderPanel();

    expect(screen.getByTestId("agents-automation-resume")).toBeEnabled();

    withAgentScope.unmount();
    useEnvironmentStore.setState({
      effectiveScopes: { "remote-1": ["ui:read", "ui:operate"] },
    });
    renderPanel();
    expect(screen.getByTestId("agents-automation-resume")).toBeDisabled();
    expect(screen.getByTestId("agents-automation-resume")).toHaveAttribute(
      "title",
      AGENT_CONTROL_DISABLED_HINT,
    );
  });

  it("lists every run with its status, newest first", () => {
    useAutomationDetailMock.mockReturnValue({
      data: automationDetailFixture({
        runs: [
          automationRunFixture({
            id: "run-1",
            runIndex: 1,
            status: "merged",
            prNumber: 100,
            prUrl: "https://github.com/aigentive/ralphx.app/pull/100",
          }),
          automationRunFixture({
            id: "run-2",
            runIndex: 2,
            status: "agent_failed",
            prNumber: null,
            prUrl: null,
            errorCode: "timeout",
          }),
          automationRunFixture({
            id: "run-3",
            runIndex: 3,
            status: "running",
            prNumber: null,
            prUrl: null,
          }),
        ],
      }),
      isLoading: false,
      isError: false,
    });

    renderPanel();

    const list = screen.getByTestId("agents-automation-runs-list");
    expect(list).toBeInTheDocument();
    // Newest run (#3) renders first.
    const rows = within(list).getAllByRole("listitem");
    expect(rows[0]).toHaveAttribute("data-testid", "agents-automation-run-3");
    expect(rows[2]).toHaveAttribute("data-testid", "agents-automation-run-1");
    // Each run shows its status label; the failed run surfaces its error code.
    expect(within(rows[0]!).getByText("Running")).toBeInTheDocument();
    expect(within(rows[1]!).getByText("Agent failed")).toBeInTheDocument();
    expect(within(rows[1]!).getByText("Failed: timeout")).toBeInTheDocument();
    expect(within(rows[2]!).getByText("Merged")).toBeInTheDocument();
    const prLink = within(rows[2]!).getByRole("button", {
      name: "Open PR #100 in browser",
    });
    expect(prLink).toHaveTextContent("PR #100");
    expect(
      within(rows[0]!).queryByRole("button", { name: /Open PR #/ }),
    ).not.toBeInTheDocument();
    expect(
      within(rows[1]!).queryByRole("button", { name: /Open PR #/ }),
    ).not.toBeInTheDocument();

    fireEvent.click(prLink);

    expect(openExternalUrlMock).toHaveBeenCalledWith(
      "https://github.com/aigentive/ralphx.app/pull/100",
    );
  });

  it("tints each run row by status, matching the runs-timeline cards", () => {
    useAutomationDetailMock.mockReturnValue({
      data: automationDetailFixture({
        runs: [
          automationRunFixture({ id: "run-1", runIndex: 1, status: "merged" }),
          automationRunFixture({
            id: "run-2",
            runIndex: 2,
            status: "agent_failed",
            prNumber: null,
            prUrl: null,
            errorCode: "timeout",
          }),
          automationRunFixture({
            id: "run-3",
            runIndex: 3,
            status: "running",
            prNumber: null,
            prUrl: null,
          }),
        ],
      }),
      isLoading: false,
      isError: false,
    });

    renderPanel();

    const rows = within(
      screen.getByTestId("agents-automation-runs-list"),
    ).getAllByRole("listitem");
    const [running, failed, merged] = rows as [
      HTMLElement,
      HTMLElement,
      HTMLElement,
    ];

    // Running/open → soft accent (orange).
    expect(running.style.backgroundColor).toContain("--accent-muted");
    expect(running.style.borderColor).toContain("--accent-border");
    // Failed → soft darker surface.
    expect(failed.style.backgroundColor).toContain("--bg-surface");
    expect(failed.style.borderColor).toContain("--border-default");
    // Merged → soft green.
    expect(merged.style.backgroundColor).toContain("--status-success-muted");
    expect(merged.style.borderColor).toContain("--status-success-border");
    // Every row carries an explicit 1px solid edge (WKWebView-safe longhands).
    for (const row of [running, failed, merged]) {
      expect(row.style.borderStyle).toBe("solid");
      expect(row.style.borderWidth).toBe("1px");
    }
  });

  it("renders URL-only run pull request links", () => {
    useAutomationDetailMock.mockReturnValue({
      data: automationDetailFixture({
        runs: [
          automationRunFixture({
            id: "run-url-only",
            runIndex: 1,
            prNumber: null,
            prUrl: "https://github.com/aigentive/ralphx.app/pull/preview",
          }),
        ],
      }),
      isLoading: false,
      isError: false,
    });

    renderPanel();

    const prLink = screen.getByRole("button", {
      name: "Open PR in browser",
    });
    expect(prLink).toHaveTextContent("PR");

    fireEvent.click(prLink);

    expect(openExternalUrlMock).toHaveBeenCalledWith(
      "https://github.com/aigentive/ralphx.app/pull/preview",
    );
  });

  it("shows the current phase chip on open run rows only", () => {
    useAutomationDetailMock.mockReturnValue({
      data: automationDetailFixture({
        automation: automationFixture({
          goalItemsJson: JSON.stringify([
            {
              id: "phase-1",
              title: "Finish the scheduler handoff",
              status: "in_progress",
            },
            { id: "phase-2", title: "Document rollout", status: "pending" },
          ]),
        }),
        runs: [
          automationRunFixture({
            id: "run-open",
            runIndex: 4,
            status: "running",
            prNumber: null,
          }),
          automationRunFixture({
            id: "run-terminal",
            runIndex: 3,
            status: "merged",
            judgeState: "done",
            prNumber: 103,
          }),
        ],
      }),
      isLoading: false,
      isError: false,
    });

    renderPanel();

    const openRun = screen.getByTestId("agents-automation-run-4");
    expect(
      within(openRun).getByTestId("agents-automation-run-4-phase"),
    ).toHaveTextContent("Finish the scheduler handoff");

    const terminalRun = screen.getByTestId("agents-automation-run-3");
    expect(
      within(terminalRun).queryByTestId("agents-automation-run-3-phase"),
    ).not.toBeInTheDocument();
  });

  it("does not show a run-row phase chip when no goal item is in progress", () => {
    useAutomationDetailMock.mockReturnValue({
      data: automationDetailFixture({
        automation: automationFixture({
          goalItemsJson: JSON.stringify([
            { id: "phase-1", title: "Pending work", status: "pending" },
          ]),
        }),
        runs: [
          automationRunFixture({
            id: "run-open",
            runIndex: 4,
            status: "running",
            prNumber: null,
          }),
        ],
      }),
      isLoading: false,
      isError: false,
    });

    renderPanel();

    expect(
      within(screen.getByTestId("agents-automation-run-4")).queryByTestId(
        "agents-automation-run-4-phase",
      ),
    ).not.toBeInTheDocument();
  });

  it("surfaces automatic merge enable warnings on published run rows", () => {
    useAutomationDetailMock.mockReturnValue({
      data: automationDetailFixture({
        runs: [
          automationRunFixture({
            id: "run-4",
            runIndex: 4,
            status: "published",
            prNumber: 104,
            errorCode: "auto_merge_enable_failed",
            errorDetail:
              "GitHub auto-merge could not be enabled yet: branch protection blocks it",
          }),
        ],
      }),
      isLoading: false,
      isError: false,
    });

    renderPanel();

    expect(
      screen.getByTestId("agents-automation-run-4-warning"),
    ).toHaveTextContent("branch protection blocks it");
  });

  it("cancels an open run from the runs list, leaving terminal runs uncancellable", async () => {
    useAutomationDetailMock.mockReturnValue({
      data: automationDetailFixture({
        runs: [
          automationRunFixture({
            id: "run-2",
            runIndex: 2,
            status: "agent_failed",
            prNumber: null,
            errorCode: "timeout",
          }),
          automationRunFixture({
            id: "run-4",
            runIndex: 4,
            status: "running",
            prNumber: null,
          }),
        ],
      }),
      isLoading: false,
      isError: false,
    });

    renderPanel();

    // Terminal (failed) run has no Cancel action.
    expect(
      screen.queryByTestId("agents-automation-run-2-cancel"),
    ).not.toBeInTheDocument();
    // Open (running) run can be canceled.
    const cancelButton = screen.getByTestId("agents-automation-run-4-cancel");
    fireEvent.click(cancelButton);

    await waitFor(() =>
      expect(cancelRunMock).toHaveBeenCalledWith({
        id: "automation-1",
        runId: "run-4",
      }),
    );
    await waitFor(() =>
      expect(toastSuccessMock).toHaveBeenCalledWith("Run canceled"),
    );
  });

  it("shows an empty runs state when no runs exist", () => {
    useAutomationDetailMock.mockReturnValue({
      data: automationDetailFixture({ runs: [] }),
      isLoading: false,
      isError: false,
    });

    renderPanel();

    expect(screen.getByTestId("agents-automation-runs")).toHaveTextContent(
      "No runs yet.",
    );
    expect(
      screen.queryByTestId("agents-automation-runs-list"),
    ).not.toBeInTheDocument();
  });

  it("extends the run budget from a paused, budget-exhausted automation", async () => {
    // 4/4 runs used, paused because the budget is exhausted.
    useAutomationDetailMock.mockReturnValue({
      data: automationDetailFixture({
        automation: automationFixture({
          status: "paused",
          pausedReasonCode: "judge_stopped_unmet",
          maxRuns: 4,
        }),
        runs: [
          automationRunFixture({ id: "run-1", runIndex: 1, status: "merged" }),
          automationRunFixture({
            id: "run-2",
            runIndex: 2,
            status: "agent_failed",
          }),
          automationRunFixture({
            id: "run-3",
            runIndex: 3,
            status: "agent_failed",
          }),
          automationRunFixture({
            id: "run-4",
            runIndex: 4,
            status: "agent_failed",
          }),
        ],
      }),
      isLoading: false,
      isError: false,
    });

    renderPanel();

    const input = screen.getByLabelText("Max runs");
    expect(input).toHaveValue(4);
    // Cannot save the unchanged value.
    expect(
      screen.getByTestId("agents-automation-max-runs-save"),
    ).toBeDisabled();

    fireEvent.change(input, { target: { value: "8" } });
    fireEvent.click(screen.getByTestId("agents-automation-max-runs-save"));

    await waitFor(() =>
      expect(updateSettingsMock).toHaveBeenCalledWith({
        id: "automation-1",
        maxRuns: 8,
      }),
    );
    await waitFor(() =>
      expect(toastSuccessMock).toHaveBeenCalledWith("Max runs updated"),
    );
  });

  it("updates plan approval, PR merge, and deep verification settings", async () => {
    updateSettingsMock.mockResolvedValue(
      automationFixture({
        planApprovalMode: "automatic",
        prMergeMode: "automatic",
        planDeepVerification: true,
      }),
    );

    renderPanel();

    expect(
      screen.getByRole("option", { name: "Automatic (judge)" }),
    ).toBeInTheDocument();
    expect(screen.getByTestId("agents-automation-settings")).toHaveTextContent(
      "Adversarially verify each run plan before it can be approved.",
    );

    fireEvent.change(
      screen.getByTestId("agents-automation-plan-approval-mode"),
      {
        target: { value: "automatic" },
      },
    );
    await waitFor(() =>
      expect(updateSettingsMock).toHaveBeenCalledWith({
        id: "automation-1",
        planApprovalMode: "automatic",
      }),
    );

    fireEvent.change(screen.getByTestId("agents-automation-pr-merge-mode"), {
      target: { value: "automatic" },
    });
    await waitFor(() =>
      expect(updateSettingsMock).toHaveBeenCalledWith({
        id: "automation-1",
        prMergeMode: "automatic",
      }),
    );

    fireEvent.click(
      screen.getByTestId("agents-automation-plan-deep-verification"),
    );
    await waitFor(() =>
      expect(updateSettingsMock).toHaveBeenCalledWith({
        id: "automation-1",
        planDeepVerification: true,
      }),
    );
  });

  it("keeps changed settings visible while the save is still pending", async () => {
    const pendingUpdate = deferred<Automation>();
    updateSettingsMock.mockReturnValue(pendingUpdate.promise);

    renderPanel();

    const planApprovalSelect = screen.getByTestId(
      "agents-automation-plan-approval-mode",
    ) as HTMLSelectElement;
    const prMergeSelect = screen.getByTestId(
      "agents-automation-pr-merge-mode",
    ) as HTMLSelectElement;
    const deepVerificationSwitch = screen.getByTestId(
      "agents-automation-plan-deep-verification",
    );

    expect(planApprovalSelect).toHaveValue("manual");

    fireEvent.change(planApprovalSelect, {
      target: { value: "automatic" },
    });

    expect(planApprovalSelect).toHaveValue("automatic");
    expect(planApprovalSelect).toBeDisabled();
    expect(prMergeSelect).not.toBeDisabled();
    expect(deepVerificationSwitch).not.toBeDisabled();

    pendingUpdate.resolve(automationFixture({ planApprovalMode: "automatic" }));
    await waitFor(() =>
      expect(toastSuccessMock).toHaveBeenCalledWith(
        "Automation settings updated",
      ),
    );
  });

  it("shows the stacked-chain merge-mode failure reason", async () => {
    updateSettingsMock.mockRejectedValue(
      new Error(
        "automation_stacked_auto_merge_unsupported: automatic PR merge is not supported for stacked PR chains",
      ),
    );

    renderPanel();

    fireEvent.change(screen.getByTestId("agents-automation-pr-merge-mode"), {
      target: { value: "automatic" },
    });

    await waitFor(() =>
      expect(toastErrorMock).toHaveBeenCalledWith(
        "Stacked PR chains require manual merge.",
      ),
    );
  });

  it("disables automatic PR merge for stacked-chain automations", () => {
    useAutomationDetailMock.mockReturnValue({
      data: automationDetailFixture({
        automation: automationFixture({
          chainMode: "pr_head_stacked",
          prMergeMode: "manual",
        }),
      }),
      isLoading: false,
      isError: false,
    });

    renderPanel();

    expect(
      screen.getByTestId("agents-automation-pr-merge-mode"),
    ).toBeDisabled();
    expect(screen.getByTestId("agents-automation-settings")).toHaveTextContent(
      "Stacked PR chains require manual merge",
    );
  });

  it("shows the parked run pill, allows cancel, and opens the run conversation synchronously", () => {
    const onFocusAutomationRun = vi.fn();
    useAutomationDetailMock.mockReturnValue({
      data: automationDetailFixture({
        runs: [
          automationRunFixture({
            id: "run-3",
            runIndex: 3,
            status: "awaiting_plan_approval",
            planJudgeState: "in_progress",
            planArtifactId: "plan-artifact-1",
            conversationId: "conversation-run-3",
            prNumber: null,
            prUrl: null,
          }),
        ],
      }),
      isLoading: false,
      isError: false,
    });

    renderPanel({ onFocusAutomationRun });

    expect(screen.getByTestId("agents-automation-stage")).toHaveTextContent(
      "Plan judge running",
    );
    expect(
      within(screen.getByTestId("agents-automation-run-3-status")).getByText(
        "Plan judge running",
      ),
    ).toBeInTheDocument();
    expect(screen.getByText("Awaiting plan approval")).toBeInTheDocument();
    expect(
      screen.getByTestId("agents-automation-run-3-cancel"),
    ).toBeInTheDocument();
    expect(screen.getByLabelText("Open run conversation")).toHaveClass(
      "cursor-pointer",
    );

    fireEvent.click(screen.getByTestId("agents-automation-run-3-status"));

    expect(onFocusAutomationRun).toHaveBeenCalledWith(
      "automation-1",
      "run-3",
      "conversation-run-3",
      expect.objectContaining({
        runStatus: "awaiting_plan_approval",
        hasPlanArtifact: true,
        hasPullRequest: false,
      }),
    );
  });

  it("makes run status pills clickable for any status with a conversation and inert without one", () => {
    const onFocusAutomationRun = vi.fn();
    useAutomationDetailMock.mockReturnValue({
      data: automationDetailFixture({
        runs: [
          automationRunFixture({
            id: "run-running",
            runIndex: 4,
            status: "running",
            conversationId: "conversation-running",
            prNumber: null,
            prUrl: null,
          }),
          automationRunFixture({
            id: "run-agent-failed",
            runIndex: 3,
            status: "agent_failed",
            conversationId: "conversation-agent-failed",
            prNumber: null,
            prUrl: null,
          }),
          automationRunFixture({
            id: "run-terminal",
            runIndex: 2,
            status: "merged",
            judgeState: "done",
            conversationId: "conversation-terminal",
          }),
          automationRunFixture({
            id: "run-without-conversation",
            runIndex: 1,
            status: "running",
            conversationId: null,
            prNumber: null,
            prUrl: null,
          }),
        ],
      }),
      isLoading: false,
      isError: false,
    });

    renderPanel({ onFocusAutomationRun });

    for (const runIndex of [4, 3, 2]) {
      const row = screen.getByTestId(`agents-automation-run-${runIndex}`);
      expect(
        within(row).getByRole("button", { name: "Open run conversation" }),
      ).toBeInTheDocument();
      fireEvent.click(
        screen.getByTestId(`agents-automation-run-${runIndex}-status`),
      );
    }

    expect(onFocusAutomationRun).toHaveBeenNthCalledWith(
      1,
      "automation-1",
      "run-running",
      "conversation-running",
      expect.objectContaining({
        runStatus: "running",
        hasPlanArtifact: false,
        hasPullRequest: false,
      }),
    );
    expect(onFocusAutomationRun).toHaveBeenNthCalledWith(
      2,
      "automation-1",
      "run-agent-failed",
      "conversation-agent-failed",
      expect.objectContaining({
        runStatus: "agent_failed",
        hasPlanArtifact: false,
        hasPullRequest: false,
      }),
    );
    expect(onFocusAutomationRun).toHaveBeenNthCalledWith(
      3,
      "automation-1",
      "run-terminal",
      "conversation-terminal",
      expect.objectContaining({
        runStatus: "merged",
        hasPlanArtifact: false,
        hasPullRequest: true,
      }),
    );

    const inertRow = screen.getByTestId("agents-automation-run-1");
    expect(
      within(inertRow).queryByRole("button", { name: "Open run conversation" }),
    ).not.toBeInTheDocument();
    fireEvent.click(screen.getByTestId("agents-automation-run-1-status"));
    expect(onFocusAutomationRun).toHaveBeenCalledTimes(3);
  });

  it("labels parked runs with pending judge revisions", () => {
    useAutomationDetailMock.mockReturnValue({
      data: automationDetailFixture({
        runs: [
          automationRunFixture({
            id: "run-3",
            runIndex: 3,
            status: "awaiting_plan_approval",
            planRevisionPending: true,
            conversationId: "conversation-run-3",
            prNumber: null,
          }),
        ],
      }),
      isLoading: false,
      isError: false,
    });

    renderPanel();

    expect(screen.getByText("Revision pending")).toBeInTheDocument();
  });

  it.each([
    [
      "plan_judge_failed",
      "Plan judge failed — review and approve the plan to resume this automation.",
    ],
    [
      "plan_revision_exhausted",
      "Plan revision limit reached — review and approve the plan to resume this automation.",
    ],
  ])(
    "deep-links %s plan-gate pause banners to the parked run conversation",
    (pausedReasonCode, pausedCopy) => {
      const onFocusAutomationRun = vi.fn();
      useAutomationDetailMock.mockReturnValue({
        data: automationDetailFixture({
          automation: automationFixture({
            status: "paused",
            pausedReasonCode,
            pausedReasonDetail: "Judge could not parse the verdict.",
          }),
          runs: [
            automationRunFixture({
              id: "run-3",
              runIndex: 3,
              status: "awaiting_plan_approval",
              planArtifactId: "plan-artifact-1",
              conversationId: "conversation-run-3",
              prNumber: null,
              prUrl: null,
            }),
          ],
        }),
        isLoading: false,
        isError: false,
      });

      renderPanel({ onFocusAutomationRun });

      expect(
        screen.getByTestId("agents-automation-plan-gate-paused"),
      ).toHaveTextContent(`${pausedCopy} Judge could not parse the verdict.`);

      fireEvent.click(screen.getByTestId("agents-automation-plan-gate-open"));

      expect(onFocusAutomationRun).toHaveBeenCalledWith(
        "automation-1",
        "run-3",
        "conversation-run-3",
        expect.objectContaining({
          runStatus: "awaiting_plan_approval",
          hasPlanArtifact: true,
          hasPullRequest: false,
        }),
      );
    },
  );

  it("hides the max runs editor while the automation is active", () => {
    useAutomationDetailMock.mockReturnValue({
      data: automationDetailFixture({
        automation: automationFixture({ status: "active" }),
      }),
      isLoading: false,
      isError: false,
    });

    renderPanel();

    expect(
      screen.queryByTestId("agents-automation-max-runs"),
    ).not.toBeInTheDocument();
  });

  it("renders the linked spec name and preview in the Spec section", () => {
    useArtifactMock.mockReturnValue({
      data: {
        id: "artifact-spec-1",
        name: "Release automation spec",
        artifact_type: "specification",
        content_type: "inline",
        content: "## Phase 1\nBuild the shared context model in a scoped PR.",
        created_at: "2026-07-05T10:00:00Z",
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
    useAutomationDetailMock.mockReturnValue({
      data: automationDetailFixture({
        automation: automationFixture({ specArtifactId: "artifact-spec-1" }),
      }),
      isLoading: false,
      isError: false,
    });

    renderPanel();

    const spec = screen.getByTestId("agents-automation-spec");
    expect(spec).toHaveTextContent("Release automation spec");
    expect(spec).toHaveTextContent(
      "Build the shared context model in a scoped PR.",
    );
    expect(useArtifactMock).toHaveBeenCalledWith("artifact-spec-1");
  });

  it("shows phase progress and a collapsed expandable spec", () => {
    useArtifactMock.mockReturnValue({
      data: {
        id: "artifact-spec-1",
        name: "Release automation spec",
        artifact_type: "specification",
        content_type: "inline",
        content: "## Phase 1\nBuild the shared context model in a scoped PR.",
        created_at: "2026-07-05T10:00:00Z",
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
    useAutomationDetailMock.mockReturnValue({
      data: automationDetailFixture({
        automation: automationFixture({
          specArtifactId: "artifact-spec-1",
          goalItemsJson: JSON.stringify([
            { id: "p1", title: "Build shared context model", status: "done" },
            { id: "p2", title: "Wire the scheduler", status: "in_progress" },
          ]),
        }),
      }),
      isLoading: false,
      isError: false,
    });

    renderPanel();

    const phases = screen.getByTestId("agents-automation-phases");
    expect(phases).toHaveTextContent("1/2 done");
    expect(within(phases).getByRole("progressbar")).toHaveAttribute(
      "aria-valuenow",
      "1",
    );
    expect(within(phases).getByText("In progress")).toBeInTheDocument();

    const spec = screen.getByTestId("agents-automation-spec");
    expect(
      within(spec).getByTestId("automation-spec-toggle"),
    ).toHaveTextContent("Show full spec");
    expect(
      within(spec).queryByTestId("automation-spec-markdown"),
    ).not.toBeInTheDocument();
  });

  it("updates draft setup settings from the automation artifact panel", async () => {
    useAutomationDetailMock.mockReturnValue({
      data: automationDetailFixture({
        automation: automationFixture({
          status: "draft",
          providerHarness: "codex",
          modelId: "gpt-5.5",
          logicalEffort: "xhigh",
          runMode: "edit",
        }),
        runs: [],
      }),
      isLoading: false,
      isError: false,
    });

    renderPanel();

    expect(screen.getByTestId("agents-automation-setup")).toHaveTextContent(
      "Automation setup",
    );
    expect(
      screen.getByTestId("agents-automation-runtime-selector"),
    ).toBeInTheDocument();
    expect(
      (screen.getByTestId("agents-automation-provider") as HTMLSelectElement)
        .value,
    ).toBe("codex");
    expect(
      (screen.getByTestId("agents-automation-model") as HTMLSelectElement)
        .value,
    ).toBe("gpt-5.5");
    expect(
      (screen.getByTestId("agents-automation-effort") as HTMLSelectElement)
        .value,
    ).toBe("xhigh");

    fireEvent.click(screen.getByTestId("agents-automation-run-mode-plan"));

    // Wave D1 routes draft edits through update_automation_config with the current config envelope.
    await waitFor(() =>
      expect(updateAutomationSetupMock).toHaveBeenCalledWith(
        "conversation-setup",
        expect.objectContaining({
          id: "automation-1",
          setupConversationId: "conversation-setup",
        }),
        {
          runMode: "plan",
          completionSignal: "agent_completed",
        },
      ),
    );
    await waitFor(() =>
      expect(toastSuccessMock).toHaveBeenCalledWith(
        "Automation will run as Plan",
      ),
    );

    updateAutomationSetupMock.mockClear();
    toastSuccessMock.mockClear();
    fireEvent.change(screen.getByTestId("agents-automation-provider"), {
      target: { value: "claude" },
    });

    await waitFor(() =>
      expect(updateAutomationSetupMock).toHaveBeenCalledWith(
        "conversation-setup",
        expect.any(Object),
        {
          providerHarness: "claude",
          modelId: "sonnet",
          logicalEffort: "medium",
        },
      ),
    );
    await waitFor(() =>
      expect(toastSuccessMock).toHaveBeenCalledWith(
        "Automation run agent updated",
      ),
    );

    updateAutomationSetupMock.mockClear();
    toastSuccessMock.mockClear();
    fireEvent.change(screen.getByTestId("agents-automation-effort"), {
      target: { value: "high" },
    });

    await waitFor(() =>
      expect(updateAutomationSetupMock).toHaveBeenCalledWith(
        "conversation-setup",
        expect.any(Object),
        {
          providerHarness: "codex",
          modelId: "gpt-5.5",
          logicalEffort: "high",
        },
      ),
    );
    await waitFor(() =>
      expect(toastSuccessMock).toHaveBeenCalledWith(
        "Automation run agent updated",
      ),
    );
  });

  it("shows an artifact-side update action for pending automation spec proposals", async () => {
    useAutomationDetailMock.mockReturnValue({
      data: automationDetailFixture({
        automation: automationFixture({
          status: "draft",
          goalPrompt: "",
          goalItemsJson: null,
        }),
        runs: [],
      }),
      isLoading: false,
      isError: false,
    });
    useAskUserQuestionMock.mockReturnValue({
      activeQuestion: {
        requestId: "question-1",
        sessionId: "conversation-setup",
        header: "Update automation?",
        question: "Apply the proposed goal and phases to this automation?",
        options: [
          {
            label: "Update automation",
            value: "apply_automation_proposal",
          },
        ],
        multiSelect: false,
        allowSkip: true,
        metadata: { kind: "automation_setup_proposal" },
      },
      answeredQuestion: undefined,
      submitAnswer: submitAutomationSetupAnswerMock,
      dismissQuestion: vi.fn(),
      clearAnswered: vi.fn(),
      isLoading: false,
    });

    renderPanel();

    expect(useAskUserQuestionMock).toHaveBeenCalledWith("conversation-setup");
    expect(
      screen.getByTestId("agents-automation-proposal-cta"),
    ).toHaveTextContent("Apply the proposed goal and phases");

    fireEvent.click(screen.getByTestId("agents-automation-proposal-update"));

    await waitFor(() =>
      expect(submitAutomationSetupAnswerMock).toHaveBeenCalledWith({
        requestId: "question-1",
        selectedOptions: ["apply_automation_proposal"],
      }),
    );
    expect(toastSuccessMock).toHaveBeenCalledWith("Automation update accepted");
  });

  it("can request an automation update from the latest plain proposal when goal and phases are not saved", async () => {
    useAutomationDetailMock.mockReturnValue({
      data: automationDetailFixture({
        automation: automationFixture({
          status: "draft",
          goalPrompt: "",
          goalItemsJson: null,
          providerHarness: "codex",
          modelId: "gpt-5.5",
          logicalEffort: "xhigh",
        }),
        runs: [],
      }),
      isLoading: false,
      isError: false,
    });

    renderPanel();

    expect(
      screen.getByTestId("agents-automation-proposal-cta"),
    ).toHaveTextContent("Save the latest proposed goal and phases");

    fireEvent.click(screen.getByTestId("agents-automation-proposal-update"));

    await waitFor(() =>
      expect(sendAgentMessageMock).toHaveBeenCalledWith(
        "project",
        "project-1",
        expect.stringContaining("Update the bound draft automation now"),
        undefined,
        {
          conversationId: "conversation-setup",
          providerHarness: "codex",
          modelId: "gpt-5.5",
          logicalEffort: "xhigh",
        },
      ),
    );
    expect(toastSuccessMock).toHaveBeenCalledWith(
      "Automation update requested",
    );
  });

  it("renders loading and error states without action controls", () => {
    useAutomationDetailMock.mockReturnValueOnce({
      data: undefined,
      isLoading: true,
      isError: false,
    });

    renderPanel();

    expect(
      screen.getByTestId("agents-automation-panel-loading"),
    ).toBeInTheDocument();
    expect(
      screen.queryByTestId("agents-automation-pause"),
    ).not.toBeInTheDocument();

    useAutomationDetailMock.mockReturnValueOnce({
      data: undefined,
      isLoading: false,
      isError: true,
    });

    renderPanel();

    expect(screen.getByText("Could not load automation.")).toBeInTheDocument();
    expect(
      screen.queryByTestId("agents-automation-open"),
    ).not.toBeInTheDocument();
  });

  it("resumes paused automations and summarizes runs without PRs", async () => {
    useAutomationDetailMock.mockReturnValue({
      data: automationDetailFixture({
        automation: automationFixture({
          status: "paused",
          pausedReasonCode: "release_freeze",
        }),
        runs: [automationRunFixture({ prNumber: null, status: "running" })],
      }),
      isLoading: false,
      isError: false,
    });

    renderPanel();

    expect(screen.getByText("Paused")).toBeInTheDocument();
    // "Running" appears both in the Current PR summary and the runs list row.
    expect(screen.getAllByText("Running").length).toBeGreaterThan(0);
    expect(
      screen.queryByTestId("agents-automation-pause"),
    ).not.toBeInTheDocument();
    expect(screen.getByTestId("agents-automation-resume")).toBeInTheDocument();

    fireEvent.click(screen.getByTestId("agents-automation-resume"));

    await waitFor(() =>
      expect(resumeAutomationMock).toHaveBeenCalledWith("automation-1"),
    );
    expect(toastSuccessMock).toHaveBeenCalledWith("Automation resumed");
  });

  it("pauses active automations and explains cancellation consequences", async () => {
    renderPanel();

    fireEvent.click(screen.getByTestId("agents-automation-pause"));
    await waitFor(() =>
      expect(pauseAutomationMock).toHaveBeenCalledWith({
        id: "automation-1",
        reasonCode: "user",
        reasonDetail: "Paused from Agents automation panel",
      }),
    );
    expect(toastSuccessMock).toHaveBeenCalledWith("Automation paused");

    fireEvent.click(screen.getByTestId("agents-automation-stop"));
    expect(await screen.findByText("Cancel automation?")).toBeInTheDocument();
    expect(
      screen.getByText(/cancelled run cannot be resumed/i),
    ).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Cancel automation" }));

    await waitFor(() =>
      expect(stopAutomationMock).toHaveBeenCalledWith("automation-1"),
    );
    expect(toastSuccessMock).toHaveBeenCalledWith("Automation cancelled");
  });

  it("surfaces the latest run failure reason as an error line", () => {
    useAutomationDetailMock.mockReturnValue({
      data: automationDetailFixture({
        runs: [
          automationRunFixture({
            status: "agent_failed",
            judgeState: "none",
            prNumber: null,
            errorCode: "publish_failed",
            errorDetail: "Publish step exited with code 1",
          }),
        ],
      }),
      isLoading: false,
      isError: false,
    });

    renderPanel();

    const failure = screen.getByTestId("agents-automation-failure");
    expect(failure).toHaveTextContent("Publish step exited with code 1");
    expect(
      screen.queryByTestId("agents-automation-paused"),
    ).not.toBeInTheDocument();
  });

  it("shows the paused reason when paused without a failed run", () => {
    useAutomationDetailMock.mockReturnValue({
      data: automationDetailFixture({
        automation: automationFixture({
          status: "paused",
          pausedReasonCode: "release_freeze",
          pausedReasonDetail: "Waiting on base branch",
        }),
        runs: [
          automationRunFixture({
            status: "running",
            judgeState: "none",
            prNumber: null,
          }),
        ],
      }),
      isLoading: false,
      isError: false,
    });

    renderPanel();

    expect(screen.getByTestId("agents-automation-paused")).toHaveTextContent(
      "Paused: release_freeze - Waiting on base branch",
    );
    expect(
      screen.queryByTestId("agents-automation-failure"),
    ).not.toBeInTheDocument();
  });

  it("does not render a failure or paused line for a healthy running automation", () => {
    useAutomationDetailMock.mockReturnValue({
      data: automationDetailFixture({
        runs: [
          automationRunFixture({
            status: "running",
            judgeState: "none",
            prNumber: null,
          }),
        ],
      }),
      isLoading: false,
      isError: false,
    });

    renderPanel();

    expect(screen.getByTestId("agents-automation-stage")).toHaveTextContent(
      "Run 3 in progress",
    );
    expect(
      screen.queryByTestId("agents-automation-failure"),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByTestId("agents-automation-paused"),
    ).not.toBeInTheDocument();
  });

  it("surfaces an idle-after-cancelled banner only for active automations", () => {
    useAutomationDetailMock.mockReturnValue({
      data: automationDetailFixture({
        runs: [
          automationRunFixture({ status: "cancelled", judgeState: "none" }),
        ],
      }),
      isLoading: false,
      isError: false,
    });

    const activeView = renderPanel();

    expect(
      screen.getByTestId("agents-automation-idle-after-cancelled"),
    ).toHaveTextContent(
      "The last run was cancelled. Run now starts a new run from that run's prompt; it does not resume the cancelled run.",
    );
    activeView.unmount();

    useAutomationDetailMock.mockReturnValue({
      data: automationDetailFixture({
        automation: automationFixture({ status: "paused" }),
        runs: [
          automationRunFixture({ status: "cancelled", judgeState: "none" }),
        ],
      }),
      isLoading: false,
      isError: false,
    });

    const pausedView = renderPanel();

    expect(
      screen.queryByTestId("agents-automation-idle-after-cancelled"),
    ).not.toBeInTheDocument();
    pausedView.unmount();

    useAutomationDetailMock.mockReturnValue({
      data: automationDetailFixture({
        runs: [automationRunFixture({ status: "running", judgeState: "none" })],
      }),
      isLoading: false,
      isError: false,
    });

    renderPanel();

    expect(
      screen.queryByTestId("agents-automation-idle-after-cancelled"),
    ).not.toBeInTheDocument();
  });

  it("starts a fresh run from the active cancelled-run notice", async () => {
    useAutomationDetailMock.mockReturnValue({
      data: automationDetailFixture({
        runs: [
          automationRunFixture({ status: "cancelled", judgeState: "none" }),
        ],
      }),
      isLoading: false,
      isError: false,
    });

    renderPanel();
    fireEvent.click(
      within(
        screen.getByTestId("agents-automation-idle-after-cancelled"),
      ).getByRole("button", { name: "Run now" }),
    );

    await waitFor(() =>
      expect(triggerRunNowMock).toHaveBeenCalledWith("automation-1"),
    );
  });

  it("restarts a stopped automation as a new run", async () => {
    useAutomationDetailMock.mockReturnValue({
      data: automationDetailFixture({
        automation: automationFixture({ status: "stopped" }),
        runs: [
          automationRunFixture({ status: "cancelled", judgeState: "none" }),
        ],
      }),
      isLoading: false,
      isError: false,
    });

    renderPanel();
    fireEvent.click(screen.getByTestId("agents-automation-restart"));

    await waitFor(() =>
      expect(restartAutomationMock).toHaveBeenCalledWith("automation-1"),
    );
    expect(toastSuccessMock).toHaveBeenCalledWith(
      "Automation restarted with a new run",
    );
  });

  it("reports deferred restart, run-now, and judge-retry outcomes", async () => {
    restartAutomationMock.mockResolvedValueOnce({
      scheduled: false,
      reason: "restart prerequisites changed",
    });
    useAutomationDetailMock.mockReturnValue({
      data: automationDetailFixture({
        automation: automationFixture({ status: "stopped" }),
        runs: [
          automationRunFixture({ status: "cancelled", judgeState: "none" }),
        ],
      }),
      isLoading: false,
      isError: false,
    });

    const restartView = renderPanel();
    fireEvent.click(screen.getByTestId("agents-automation-restart"));
    await waitFor(() =>
      expect(toastInfoMock).toHaveBeenCalledWith(
        "restart prerequisites changed",
      ),
    );
    restartView.unmount();

    triggerRunNowMock.mockResolvedValueOnce({
      scheduled: false,
      reason: "run in flight",
    });
    useAutomationDetailMock.mockReturnValue({
      data: automationDetailFixture({
        runs: [
          automationRunFixture({ status: "cancelled", judgeState: "none" }),
        ],
      }),
      isLoading: false,
      isError: false,
    });

    const runNowView = renderPanel();
    fireEvent.click(screen.getByRole("button", { name: "Run now" }));
    await waitFor(() =>
      expect(toastInfoMock).toHaveBeenCalledWith("run in flight"),
    );
    runNowView.unmount();

    retryJudgeMock.mockResolvedValueOnce({
      scheduled: false,
      reason: "terminal judge already retried",
    });
    useAutomationDetailMock.mockReturnValue({
      data: automationDetailFixture({
        runs: [
          automationRunFixture({ status: "completed", judgeState: "failed" }),
        ],
      }),
      isLoading: false,
      isError: false,
    });

    const terminalJudgeView = renderPanel();
    fireEvent.click(
      screen.getByRole("button", { name: "Retry terminal judge" }),
    );
    await waitFor(() =>
      expect(toastInfoMock).toHaveBeenCalledWith(
        "terminal judge already retried",
      ),
    );
    terminalJudgeView.unmount();

    retryPlanJudgeMock.mockResolvedValueOnce({
      scheduled: false,
      reason: "plan judge already retried",
    });
    useAutomationDetailMock.mockReturnValue({
      data: automationDetailFixture({
        automation: automationFixture({ planApprovalMode: "automatic" }),
        runs: [
          automationRunFixture({
            status: "awaiting_plan_approval",
            planJudgeState: "failed",
            planArtifactId: "plan-artifact-1",
          }),
        ],
      }),
      isLoading: false,
      isError: false,
    });

    renderPanel();
    fireEvent.click(screen.getByRole("button", { name: "Retry plan judge" }));
    await waitFor(() =>
      expect(toastInfoMock).toHaveBeenCalledWith("plan judge already retried"),
    );
  });

  it("reports a rejected automation cancellation", async () => {
    stopAutomationMock.mockRejectedValueOnce(new Error("cancel failed"));
    renderPanel();

    fireEvent.click(screen.getByTestId("agents-automation-stop"));
    fireEvent.click(
      await screen.findByRole("button", { name: "Cancel automation" }),
    );

    await waitFor(() =>
      expect(toastErrorMock).toHaveBeenCalledWith(
        "Failed to cancel automation",
      ),
    );
  });

  it.each([
    {
      name: "restart",
      actionName: "Restart automation",
      apiMock: restartAutomationMock,
      errorMessage: "Failed to restart automation",
      detail: automationDetailFixture({
        automation: automationFixture({ status: "stopped" }),
        runs: [
          automationRunFixture({ status: "cancelled", judgeState: "none" }),
        ],
      }),
    },
    {
      name: "run now",
      actionName: "Run now",
      apiMock: triggerRunNowMock,
      errorMessage: "Failed to run automation",
      detail: automationDetailFixture({
        runs: [
          automationRunFixture({ status: "cancelled", judgeState: "none" }),
        ],
      }),
    },
    {
      name: "terminal judge retry",
      actionName: "Retry terminal judge",
      apiMock: retryJudgeMock,
      errorMessage: "Failed to retry terminal judge",
      detail: automationDetailFixture({
        runs: [
          automationRunFixture({ status: "completed", judgeState: "failed" }),
        ],
      }),
    },
    {
      name: "plan judge retry",
      actionName: "Retry plan judge",
      apiMock: retryPlanJudgeMock,
      errorMessage: "Failed to retry plan judge",
      detail: automationDetailFixture({
        automation: automationFixture({ planApprovalMode: "automatic" }),
        runs: [
          automationRunFixture({
            status: "awaiting_plan_approval",
            planJudgeState: "failed",
            planArtifactId: "plan-artifact-1",
          }),
        ],
      }),
    },
  ])(
    "reports a rejected $name action",
    async ({ actionName, apiMock, detail, errorMessage }) => {
      apiMock.mockRejectedValueOnce(new Error(`${actionName} failed`));
      useAutomationDetailMock.mockReturnValue({
        data: detail,
        isLoading: false,
        isError: false,
      });
      renderPanel();

      fireEvent.click(screen.getByRole("button", { name: actionName }));

      await waitFor(() =>
        expect(toastErrorMock).toHaveBeenCalledWith(errorMessage),
      );
    },
  );

  it("renders terminal automations without mutation controls", () => {
    useAutomationDetailMock.mockReturnValue({
      data: automationDetailFixture({
        automation: automationFixture({
          status: "completed",
          maxRuns: 3,
        }),
        runs: [],
      }),
      isLoading: false,
      isError: false,
    });

    renderPanel({ onOpenAutomation: null });

    expect(screen.getByText("Completed")).toBeInTheDocument();
    expect(screen.getByText("0 of 3")).toBeInTheDocument();
    expect(screen.getByText("No PR yet")).toBeInTheDocument();
    expect(
      screen.queryByTestId("agents-automation-pause"),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByTestId("agents-automation-resume"),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByTestId("agents-automation-stop"),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByTestId("agents-automation-delete"),
    ).not.toBeInTheDocument();
    expect(screen.getByTestId("agents-automation-open")).toBeDisabled();
  });

  it("deletes draft automations after confirming the archive inventory", async () => {
    useAutomationDetailMock.mockReturnValue({
      data: automationDetailFixture({
        automation: automationFixture({
          status: "draft",
          setupConversationId: "conversation-setup",
          specArtifactId: "spec-1",
        }),
        runs: [],
      }),
      isLoading: false,
      isError: false,
    });

    renderPanel();

    const deleteButton = screen.getByTestId("agents-automation-delete");
    expect(deleteButton).toHaveTextContent("Delete draft");

    fireEvent.click(deleteButton);

    expect(
      await screen.findByText("Delete draft automation?"),
    ).toBeInTheDocument();
    expect(
      screen.getByText(
        /Archives the setup conversation and 0 run conversations\./,
      ),
    ).toBeInTheDocument();
    expect(screen.getByText(/Archives the linked spec\./)).toBeInTheDocument();
    expect(
      screen.getByText(
        /Permanently removes the automation and its run history\./,
      ),
    ).toBeInTheDocument();

    const dialog = screen.getByRole("alertdialog");
    fireEvent.click(
      within(dialog).getByRole("button", { name: "Delete draft" }),
    );

    await waitFor(() =>
      expect(deleteAutomationMock).toHaveBeenCalledWith("automation-1"),
    );
    await waitFor(() =>
      expect(toastSuccessMock).toHaveBeenCalledWith("Automation deleted"),
    );
  });

  it("does not show a delete action for non-draft automations", () => {
    renderPanel();

    expect(
      screen.queryByTestId("agents-automation-delete"),
    ).not.toBeInTheDocument();
  });
});
