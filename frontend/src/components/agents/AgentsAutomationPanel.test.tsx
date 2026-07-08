import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import type { Automation, AutomationDetail, AutomationRun } from "@/api/automations";
import { AgentsAutomationPanel } from "./AgentsAutomationPanel";

const {
  useAutomationDetailMock,
  useAutomationEventsMock,
  pauseAutomationMock,
  resumeAutomationMock,
  stopAutomationMock,
  deleteAutomationMock,
  cancelRunMock,
  updateSettingsMock,
  updateAutomationSetupMock,
  sendAgentMessageMock,
  useAskUserQuestionMock,
  submitAutomationSetupAnswerMock,
  useArtifactMock,
  toastSuccessMock,
  toastErrorMock,
} = vi.hoisted(() => ({
  useAutomationDetailMock: vi.fn(),
  useAutomationEventsMock: vi.fn(),
  pauseAutomationMock: vi.fn(),
  resumeAutomationMock: vi.fn(),
  stopAutomationMock: vi.fn(),
  deleteAutomationMock: vi.fn(),
  cancelRunMock: vi.fn(),
  updateSettingsMock: vi.fn(),
  updateAutomationSetupMock: vi.fn(),
  sendAgentMessageMock: vi.fn(),
  useAskUserQuestionMock: vi.fn(),
  submitAutomationSetupAnswerMock: vi.fn(),
  useArtifactMock: vi.fn(),
  toastSuccessMock: vi.fn(),
  toastErrorMock: vi.fn(),
}));

vi.mock("@/hooks/useArtifacts", () => ({
  useArtifact: (...args: unknown[]) => useArtifactMock(...args),
}));

vi.mock("sonner", () => ({
  toast: {
    success: toastSuccessMock,
    error: toastErrorMock,
  },
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
  conversationId: "conversation-1",
  runPrompt: "Continue the release automation.",
  promptAuthor: "judge",
  baseRefKind: "project_default",
  baseRefUsed: "main",
  baseFromRunId: "run-0",
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

function renderPanel(onOpenAutomation: ((automationId: string) => void) | null = vi.fn()) {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });

  render(
    <QueryClientProvider client={queryClient}>
      <AgentsAutomationPanel
        automationId="automation-1"
        {...(onOpenAutomation ? { onOpenAutomation } : {})}
      />
    </QueryClientProvider>,
  );

  return { onOpenAutomation };
}

describe("AgentsAutomationPanel", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    useArtifactMock.mockReturnValue({
      data: null,
      isLoading: false,
      isError: false,
    });
    pauseAutomationMock.mockResolvedValue(automationFixture({ status: "paused" }));
    resumeAutomationMock.mockResolvedValue(automationFixture({ status: "active" }));
    stopAutomationMock.mockResolvedValue(automationFixture({ status: "stopped" }));
    deleteAutomationMock.mockResolvedValue(undefined);
    cancelRunMock.mockResolvedValue(automationRunFixture({ status: "cancelled" }));
    updateSettingsMock.mockResolvedValue(automationFixture({ maxRuns: 8 }));
    updateAutomationSetupMock.mockResolvedValue(automationFixture({ status: "draft" }));
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
  });

  it("shows automation summary controls and opens the automation detail", () => {
    const { onOpenAutomation } = renderPanel();

    expect(screen.getByTestId("agents-automation-panel")).toBeInTheDocument();
    expect(screen.getByText("Release automation")).toBeInTheDocument();
    expect(screen.getByText("Approved")).toBeInTheDocument();
    expect(screen.getByText("3 of 25")).toBeInTheDocument();
    expect(screen.getByText("PR #593 · Running")).toBeInTheDocument();
    expect(screen.getByTestId("agents-automation-goal")).toHaveTextContent(
      "Ship the remaining release tasks.",
    );
    expect(screen.getByTestId("agents-automation-phases")).toHaveTextContent(
      "Build shared context model",
    );
    expect(screen.getByTestId("agents-automation-spec")).toHaveTextContent(
      "No spec linked yet.",
    );
    expect(screen.getByTestId("agents-automation-setup-summary")).toHaveTextContent(
      "Configure selected artifact context for chat.",
    );
    expect(screen.getByTestId("agents-automation-first-run")).toHaveTextContent(
      "Build the shared context model in a scoped PR.",
    );
    expect(screen.getByTestId("agents-automation-stage")).toHaveTextContent(
      "Waiting for PR #593 to merge",
    );
    expect(screen.queryByTestId("agents-automation-failure")).not.toBeInTheDocument();
    expect(screen.getByTestId("agents-automation-pause")).toBeInTheDocument();
    expect(screen.getByTestId("agents-automation-stop")).toBeInTheDocument();

    fireEvent.click(screen.getByTestId("agents-automation-open"));

    expect(onOpenAutomation).toHaveBeenCalledWith("automation-1");
    expect(useAutomationEventsMock).toHaveBeenCalledWith("automation-1");
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
          }),
          automationRunFixture({
            id: "run-2",
            runIndex: 2,
            status: "agent_failed",
            prNumber: null,
            errorCode: "timeout",
          }),
          automationRunFixture({
            id: "run-3",
            runIndex: 3,
            status: "running",
            prNumber: null,
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
    expect(within(rows[2]!).getByText("PR #100")).toBeInTheDocument();
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
          automationRunFixture({ id: "run-2", runIndex: 2, status: "agent_failed" }),
          automationRunFixture({ id: "run-3", runIndex: 3, status: "agent_failed" }),
          automationRunFixture({ id: "run-4", runIndex: 4, status: "agent_failed" }),
        ],
      }),
      isLoading: false,
      isError: false,
    });

    renderPanel();

    const input = screen.getByLabelText("Max runs");
    expect(input).toHaveValue(4);
    // Cannot save the unchanged value.
    expect(screen.getByTestId("agents-automation-max-runs-save")).toBeDisabled();

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
    expect(spec).toHaveTextContent("Build the shared context model in a scoped PR.");
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
    expect(within(spec).getByTestId("automation-spec-toggle")).toHaveTextContent(
      "Show full spec",
    );
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
    expect(screen.getByTestId("agents-automation-runtime-selector")).toBeInTheDocument();
    expect(
      (screen.getByTestId("agents-automation-provider") as HTMLSelectElement).value,
    ).toBe("codex");
    expect(
      (screen.getByTestId("agents-automation-model") as HTMLSelectElement).value,
    ).toBe("gpt-5.5");
    expect(
      (screen.getByTestId("agents-automation-effort") as HTMLSelectElement).value,
    ).toBe("xhigh");

    fireEvent.click(screen.getByTestId("agents-automation-run-mode-plan"));

    await waitFor(() =>
      expect(updateAutomationSetupMock).toHaveBeenCalledWith("conversation-setup", {
        runMode: "plan",
        completionSignal: "agent_completed",
      }),
    );
    await waitFor(() =>
      expect(toastSuccessMock).toHaveBeenCalledWith("Automation will run as Plan"),
    );

    updateAutomationSetupMock.mockClear();
    toastSuccessMock.mockClear();
    fireEvent.change(screen.getByTestId("agents-automation-provider"), {
      target: { value: "claude" },
    });

    await waitFor(() =>
      expect(updateAutomationSetupMock).toHaveBeenCalledWith("conversation-setup", {
        providerHarness: "claude",
        modelId: "sonnet",
        logicalEffort: "medium",
      }),
    );
    await waitFor(() =>
      expect(toastSuccessMock).toHaveBeenCalledWith("Automation run agent updated"),
    );

    updateAutomationSetupMock.mockClear();
    toastSuccessMock.mockClear();
    fireEvent.change(screen.getByTestId("agents-automation-effort"), {
      target: { value: "high" },
    });

    await waitFor(() =>
      expect(updateAutomationSetupMock).toHaveBeenCalledWith("conversation-setup", {
        providerHarness: "codex",
        modelId: "gpt-5.5",
        logicalEffort: "high",
      }),
    );
    await waitFor(() =>
      expect(toastSuccessMock).toHaveBeenCalledWith("Automation run agent updated"),
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
    expect(screen.getByTestId("agents-automation-proposal-cta")).toHaveTextContent(
      "Apply the proposed goal and phases",
    );

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

    expect(screen.getByTestId("agents-automation-proposal-cta")).toHaveTextContent(
      "Save the latest proposed goal and phases",
    );

    fireEvent.click(screen.getByTestId("agents-automation-proposal-update"));

    await waitFor(() =>
      expect(sendAgentMessageMock).toHaveBeenCalledWith(
        "project",
        "project-1",
        expect.stringContaining("Update the bound draft automation now"),
        undefined,
        undefined,
        {
          conversationId: "conversation-setup",
          providerHarness: "codex",
          modelId: "gpt-5.5",
          logicalEffort: "xhigh",
        },
      ),
    );
    expect(toastSuccessMock).toHaveBeenCalledWith("Automation update requested");
  });

  it("renders loading and error states without action controls", () => {
    useAutomationDetailMock.mockReturnValueOnce({
      data: undefined,
      isLoading: true,
      isError: false,
    });

    renderPanel();

    expect(screen.getByTestId("agents-automation-panel-loading")).toBeInTheDocument();
    expect(screen.queryByTestId("agents-automation-pause")).not.toBeInTheDocument();

    useAutomationDetailMock.mockReturnValueOnce({
      data: undefined,
      isLoading: false,
      isError: true,
    });

    renderPanel();

    expect(screen.getByText("Could not load automation.")).toBeInTheDocument();
    expect(screen.queryByTestId("agents-automation-open")).not.toBeInTheDocument();
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
    expect(screen.queryByTestId("agents-automation-pause")).not.toBeInTheDocument();
    expect(screen.getByTestId("agents-automation-resume")).toBeInTheDocument();

    fireEvent.click(screen.getByTestId("agents-automation-resume"));

    await waitFor(() => expect(resumeAutomationMock).toHaveBeenCalledWith("automation-1"));
    expect(toastSuccessMock).toHaveBeenCalledWith("Automation resumed");
  });

  it("pauses active automations and confirms stop requests", async () => {
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
    expect(await screen.findByText("Stop automation?")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Stop" }));

    await waitFor(() => expect(stopAutomationMock).toHaveBeenCalledWith("automation-1"));
    expect(toastSuccessMock).toHaveBeenCalledWith("Automation stopped");
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
    expect(screen.queryByTestId("agents-automation-paused")).not.toBeInTheDocument();
  });

  it("shows the paused reason when paused without a failed run", () => {
    useAutomationDetailMock.mockReturnValue({
      data: automationDetailFixture({
        automation: automationFixture({
          status: "paused",
          pausedReasonCode: "release_freeze",
          pausedReasonDetail: "Waiting on base branch",
        }),
        runs: [automationRunFixture({ status: "running", judgeState: "none", prNumber: null })],
      }),
      isLoading: false,
      isError: false,
    });

    renderPanel();

    expect(screen.getByTestId("agents-automation-paused")).toHaveTextContent(
      "Paused: release_freeze - Waiting on base branch",
    );
    expect(screen.queryByTestId("agents-automation-failure")).not.toBeInTheDocument();
  });

  it("does not render a failure or paused line for a healthy running automation", () => {
    useAutomationDetailMock.mockReturnValue({
      data: automationDetailFixture({
        runs: [automationRunFixture({ status: "running", judgeState: "none", prNumber: null })],
      }),
      isLoading: false,
      isError: false,
    });

    renderPanel();

    expect(screen.getByTestId("agents-automation-stage")).toHaveTextContent("Run 3 in progress");
    expect(screen.queryByTestId("agents-automation-failure")).not.toBeInTheDocument();
    expect(screen.queryByTestId("agents-automation-paused")).not.toBeInTheDocument();
  });

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

    renderPanel(null);

    expect(screen.getByText("Completed")).toBeInTheDocument();
    expect(screen.getByText("0 of 3")).toBeInTheDocument();
    expect(screen.getByText("No PR yet")).toBeInTheDocument();
    expect(screen.queryByTestId("agents-automation-pause")).not.toBeInTheDocument();
    expect(screen.queryByTestId("agents-automation-resume")).not.toBeInTheDocument();
    expect(screen.queryByTestId("agents-automation-stop")).not.toBeInTheDocument();
    expect(screen.queryByTestId("agents-automation-delete")).not.toBeInTheDocument();
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

    expect(await screen.findByText("Delete draft automation?")).toBeInTheDocument();
    expect(
      screen.getByText(/Archives the setup conversation and 0 run conversations\./),
    ).toBeInTheDocument();
    expect(screen.getByText(/Archives the linked spec\./)).toBeInTheDocument();
    expect(
      screen.getByText(/Permanently removes the automation and its run history\./),
    ).toBeInTheDocument();

    const dialog = screen.getByRole("alertdialog");
    fireEvent.click(within(dialog).getByRole("button", { name: "Delete draft" }));

    await waitFor(() =>
      expect(deleteAutomationMock).toHaveBeenCalledWith("automation-1"),
    );
    await waitFor(() =>
      expect(toastSuccessMock).toHaveBeenCalledWith("Automation deleted"),
    );
  });

  it("does not show a delete action for non-draft automations", () => {
    renderPanel();

    expect(screen.queryByTestId("agents-automation-delete")).not.toBeInTheDocument();
  });
});
