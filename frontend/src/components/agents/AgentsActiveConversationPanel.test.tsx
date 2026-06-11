import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import type { ComponentProps, ReactNode } from "react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

import {
  chatApi,
  type AgentConversationWorkspace,
  type ForkAgentConversationResult,
} from "@/api/chat";
import { TooltipProvider } from "@/components/ui/tooltip";

import type { AgentConversation } from "./agentConversations";
import { AgentsActiveConversationPanel } from "./AgentsActiveConversationPanel";

const {
  getSessionPlanMock,
  getPlanComplexityAssessmentMock,
  approvePlanArtifactMock,
  sendAgentMessageMock,
  switchAgentConversationModeMock,
  useVerificationStatusMock,
  getVerificationSpecialistsMock,
  confirmVerificationMock,
  composerQuestionModeRef,
} = vi.hoisted(() => ({
  getSessionPlanMock: vi.fn(),
  getPlanComplexityAssessmentMock: vi.fn(),
  approvePlanArtifactMock: vi.fn(),
  sendAgentMessageMock: vi.fn(),
  switchAgentConversationModeMock: vi.fn(),
  useVerificationStatusMock: vi.fn(),
  getVerificationSpecialistsMock: vi.fn(),
  confirmVerificationMock: vi.fn(),
  composerQuestionModeRef: { current: undefined as unknown },
}));

vi.mock("@/components/Chat/IntegratedChatPanel", () => ({
  IntegratedChatPanel: ({
    additionalQuestionSessionIds,
    planApprovalAction,
    renderComposer,
  }: {
    additionalQuestionSessionIds?: string[];
    planApprovalAction?: {
      label: string;
      onClick: () => void;
      disabled?: boolean;
      isPending?: boolean;
    };
    renderComposer: (props: Record<string, unknown>) => ReactNode;
  }) => (
    <div
      data-testid="integrated-chat-panel"
      data-question-session-ids={additionalQuestionSessionIds?.join(",") ?? ""}
    >
      {planApprovalAction && (
        <button
          type="button"
          data-testid="question-plan-approval-action"
          disabled={planApprovalAction.disabled}
          data-pending={String(planApprovalAction.isPending ?? false)}
          onClick={planApprovalAction.onClick}
        >
          {planApprovalAction.label}
        </button>
      )}
      {renderComposer({
        onSend: vi.fn(),
        onStop: vi.fn(),
        agentStatus: "idle",
        isSending: false,
        isReadOnly: false,
        autoFocus: false,
        hasQueuedMessages: false,
        onEditLastQueued: vi.fn(),
        attachments: [],
        enableAttachments: false,
        onFilesSelected: vi.fn(),
        onRemoveAttachment: vi.fn(),
        attachmentsUploading: false,
        ...(composerQuestionModeRef.current !== undefined
          ? { questionMode: composerQuestionModeRef.current }
          : {}),
      })}
    </div>
  ),
}));

vi.mock("@/api/artifact", () => ({
  artifactApi: {
    getSessionPlan: (...args: unknown[]) => getSessionPlanMock(...args),
    getPlanComplexityAssessment: (...args: unknown[]) =>
      getPlanComplexityAssessmentMock(...args),
    approvePlanArtifact: (...args: unknown[]) =>
      approvePlanArtifactMock(...args),
  },
}));

vi.mock("@/api/verification", () => ({
  verificationApi: {
    getSpecialists: (...args: unknown[]) =>
      getVerificationSpecialistsMock(...args),
    confirm: (...args: unknown[]) => confirmVerificationMock(...args),
  },
}));

vi.mock("@/api/chat", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/api/chat")>();
  return {
    ...actual,
    chatApi: {
      ...actual.chatApi,
      listWorkspaceOpenTargets: vi.fn().mockResolvedValue([]),
      openAgentConversationWorkspacePath: vi.fn().mockResolvedValue(undefined),
      sendAgentMessage: sendAgentMessageMock,
      switchAgentConversationMode: switchAgentConversationModeMock,
    },
  };
});

vi.mock("@/hooks/useVerificationStatus", () => ({
  verificationStatusKey: (sessionId: string) => ["verification", sessionId],
  useVerificationStatus: (...args: unknown[]) =>
    useVerificationStatusMock(...args),
}));

vi.mock("@/hooks/useAgentModels", () => ({
  useAgentModels: () => ({
    isReady: true,
    registry: {
      claude: [
        {
          id: "sonnet",
          label: "sonnet",
          menuLabel: "sonnet",
          defaultEffort: "medium",
          supportedEfforts: ["low", "medium", "high", "max"],
        },
        {
          id: "opus",
          label: "opus",
          menuLabel: "opus",
          defaultEffort: "xhigh",
          supportedEfforts: ["low", "medium", "high", "xhigh", "max"],
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
        isDefault: true,
        model: null,
        effort: null,
        approvalPolicy: null,
        sandboxMode: null,
        claudePermissionMode: null,
        claudeDangerouslySkipPermissions: false,
        claudeAllowDangerouslySkipPermissions: false,
        available: true,
        binaryFound: true,
        binaryPath: "/tmp/claude",
        status: "ready",
        error: null,
        missingCoreExecFeatures: [],
        supportedEfforts: ["low", "medium", "high", "max"],
        updatedAt: "2026-05-16T00:00:00.000Z",
      },
    ],
    isLoading: false,
    isPlaceholderData: false,
  }),
}));

vi.mock("@/stores/chatStore", () => ({
  selectQueuedMessages: () => () => [],
  useChatStore: () => [],
}));

vi.mock("@/stores/uiStore", () => ({
  useUiStore: (selector: (state: { openModal: () => void; executionStatus: { isPaused: boolean } }) => unknown) =>
    selector({ openModal: vi.fn(), executionStatus: { isPaused: false } }),
}));

vi.mock("./AgentComposerSurface", () => ({
  AgentComposerSurface: ({
    model,
    effort,
    showHelperText,
    onSend,
    onForkSession,
  }: {
    model: { value: string; onValueChange: (value: string) => void };
    effort: { value: string; onValueChange: (value: string) => void };
    showHelperText?: boolean;
    onSend: (message: string) => Promise<void> | void;
    onForkSession?: () => Promise<unknown> | void;
  }) => (
    <div>
      <div data-testid="workspace-model-value">{model.value}</div>
      <div data-testid="workspace-effort-value">{effort.value}</div>
      <div data-testid="workspace-helper-enabled">{String(showHelperText !== false)}</div>
      <button
        type="button"
        data-testid="change-workspace-model"
        onClick={() => model.onValueChange("sonnet")}
      />
      <button
        type="button"
        data-testid="change-workspace-effort"
        onClick={() => effort.onValueChange("max")}
      />
      <button
        type="button"
        data-testid="send-fork-command"
        onClick={() => void onSend("/fork")}
      />
      <button
        type="button"
        data-testid="send-fork-followup-command"
        onClick={() => void onSend("/fork continue this thread")}
      />
      <button
        type="button"
        data-testid="composer-fork-action"
        onClick={() => void onForkSession?.()}
      />
    </div>
  ),
  AgentComposerProjectLine: () => null,
}));

vi.mock("./AgentConversationBaseLine", () => ({
  AgentConversationBaseLine: () => null,
}));

vi.mock("./AgentsChatHeaderController", () => ({
  AgentsChatHeaderController: () => null,
}));

vi.mock("./AgentProviderSettingsButton", () => ({
  AgentProviderSettingsButton: () => null,
}));

vi.mock("./AgentsTerminalRegion", () => ({
  AgentsTerminalDockHost: () => null,
}));

function projectConversation(): AgentConversation {
  return {
    id: "conversation-1",
    contextType: "project",
    contextId: "project-1",
    projectId: "project-1",
    ideationSessionId: null,
    claudeSessionId: null,
    providerSessionId: null,
    providerHarness: null,
    agentMode: "ideation",
    title: "Conversation",
    messageCount: 0,
    lastMessageAt: null,
    createdAt: "2026-05-16T00:00:00.000Z",
    updatedAt: "2026-05-16T00:00:00.000Z",
    archivedAt: null,
  };
}

function workspace(): AgentConversationWorkspace {
  return {
    conversationId: "conversation-1",
    projectId: "project-1",
    mode: "ideation",
    baseRefKind: "project_default",
    baseRef: "main",
    baseDisplayName: "Project default (main)",
    baseCommit: null,
    branchName: "ralphx/conversation-1",
    worktreePath: "/tmp/conversation-1",
    linkedIdeationSessionId: null,
    linkedPlanBranchId: null,
    modeSwitchLocked: false,
    modeSwitchLockReason: null,
    publicationPrNumber: null,
    publicationPrUrl: null,
    publicationPrStatus: null,
    publicationPushStatus: null,
    status: "active",
    createdAt: "2026-05-16T00:00:00.000Z",
    updatedAt: "2026-05-16T00:00:00.000Z",
  };
}

function forkResult(): ForkAgentConversationResult {
  return {
    parentConversation: projectConversation() as never,
    conversation: { ...projectConversation(), id: "conversation-fork" } as never,
    workspace: null,
    providerSessionForked: true,
    copiedMessageCount: 2,
    copiedTimelineItemCount: 0,
  };
}

function planArtifact(status: "draft" | "approved" = "draft") {
  return {
    id: "artifact-1",
    type: "specification",
    name: "Implementation Plan",
    content: { type: "inline", text: "# Plan" },
    metadata: {
      createdAt: "2026-05-23T05:00:00Z",
      createdBy: "ralphx-ideation",
      version: 1,
    },
    derivedFrom: [],
    bucketId: "prd-library",
    planApproval:
      status === "draft"
        ? { status: "draft" }
        : {
            status: "approved",
            approvedArtifactId: "artifact-1",
            approvedVersion: 1,
            approvedAt: "2026-05-23T05:01:00Z",
          },
  };
}

function renderPanel(
  overrides: Partial<ComponentProps<typeof AgentsActiveConversationPanel>> = {},
) {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  const props: ComponentProps<typeof AgentsActiveConversationPanel> = {
    activeConversation: projectConversation(),
    activeConversationMode: "ideation",
    activeConversationModeLocked: false,
    activeProjectId: "project-1",
    activeProjectOptions: [{ id: "project-1", label: "RalphX" }],
    activeWorkspace: workspace(),
    activeWorkspaceFreshness: undefined,
    attachedIdeationSessionId: null,
    availableArtifactTabs: [],
    chatFocus: { type: "workspace" },
    chatFocusOptions: [],
    hasAutoOpenArtifacts: false,
    normalizedActiveRuntime: {
      provider: "claude",
      modelId: "opus",
      effort: "xhigh",
    },
    onActiveConversationModeChange: vi.fn(),
    onActiveConversationModeMenuOpen: vi.fn(),
    onActiveEffortChange: vi.fn(),
    onActiveModelChange: vi.fn(),
    onAgentUserMessageSent: vi.fn(),
    onFocusIdeationSession: vi.fn(),
    onForkConversation: vi.fn().mockResolvedValue(forkResult()),
    onOpenPublishPane: vi.fn(),
    onOpenPublishFile: vi.fn(),
    onPreloadArtifacts: vi.fn(),
    onPublishWorkspace: vi.fn(),
    onRenameConversation: vi.fn(),
    onSelectArtifact: vi.fn(),
    onToggleArtifacts: vi.fn(),
    onSelectChatFocus: vi.fn(),
    publishShortcutLabel: "P",
    publishingConversationId: null,
    selectedConversationId: "conversation-1",
    setTerminalChatDockElement: vi.fn(),
    switchingConversationModeId: null,
    terminalUnavailableReason: null,
    ...overrides,
  };
  render(
    <QueryClientProvider client={queryClient}>
      <TooltipProvider delayDuration={0}>
        <AgentsActiveConversationPanel {...props} />
      </TooltipProvider>
    </QueryClientProvider>,
  );
  return props;
}

describe("AgentsActiveConversationPanel", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    composerQuestionModeRef.current = undefined;
    getSessionPlanMock.mockResolvedValue(null);
    getPlanComplexityAssessmentMock.mockResolvedValue(null);
    approvePlanArtifactMock.mockResolvedValue(null);
    sendAgentMessageMock.mockResolvedValue({
      conversationId: "conversation-fork",
      agentRunId: "run-fork",
      isNewConversation: false,
      wasQueued: false,
      queuedAsPending: false,
      queuedMessageId: null,
    });
    switchAgentConversationModeMock.mockResolvedValue({
      workspace: { ...workspace(), mode: "ideation" },
    });
    useVerificationStatusMock.mockReturnValue({
      data: {
        sessionId: "planning-session-1",
        status: "unverified",
        inProgress: false,
        gaps: [],
        rounds: [],
        roundDetails: [],
        runHistory: [],
      },
      isFetching: false,
      isLoading: false,
    });
    getVerificationSpecialistsMock.mockResolvedValue({ specialists: [] });
    confirmVerificationMock.mockResolvedValue({ status: "ok" });
  });

  it("normalizes workspace runtime and forwards provider-supported efforts", () => {
    const onActiveModelChange = vi.fn();
    const onActiveEffortChange = vi.fn();
    renderPanel({ onActiveEffortChange, onActiveModelChange });

    expect(screen.getByTestId("workspace-effort-value").textContent).toBe("high");
    expect(screen.getByTestId("workspace-helper-enabled").textContent).toBe("true");

    fireEvent.click(screen.getByTestId("change-workspace-model"));
    fireEvent.click(screen.getByTestId("change-workspace-effort"));

    expect(onActiveModelChange).toHaveBeenCalledWith("sonnet", [
      "low",
      "medium",
      "high",
      "max",
    ]);
    expect(onActiveEffortChange).toHaveBeenCalledWith("max", [
      "low",
      "medium",
      "high",
      "max",
    ]);
  });

  it("bridges attached Plan-mode planning session questions into the workspace chat", () => {
    renderPanel({
      activeConversation: { ...projectConversation(), agentMode: "plan" },
      activeConversationMode: "plan",
      activeWorkspace: {
        ...workspace(),
        mode: "plan",
        linkedIdeationSessionId: "planning-session-1",
      },
      attachedIdeationSessionId: "planning-session-1",
    });

    expect(screen.getByTestId("integrated-chat-panel")).toHaveAttribute(
      "data-question-session-ids",
      "planning-session-1",
    );
  });

  it("provides an Approve Plan action for draft Plan-mode sessions", async () => {
    const user = userEvent.setup();
    getSessionPlanMock.mockResolvedValue({
      id: "artifact-1",
      type: "specification",
      name: "Implementation Plan",
      content: { type: "inline", text: "# Plan" },
      metadata: {
        createdAt: "2026-05-23T05:00:00Z",
        createdBy: "ralphx-ideation",
        version: 1,
      },
      derivedFrom: [],
      bucketId: "prd-library",
      planApproval: { status: "draft" },
    });
    approvePlanArtifactMock.mockResolvedValue({
      id: "artifact-1",
      type: "specification",
      name: "Implementation Plan",
      content: { type: "inline", text: "# Plan" },
      metadata: {
        createdAt: "2026-05-23T05:00:00Z",
        createdBy: "ralphx-ideation",
        version: 1,
      },
      derivedFrom: [],
      bucketId: "prd-library",
      planApproval: {
        status: "approved",
        approvedArtifactId: "artifact-1",
        approvedVersion: 1,
        approvedAt: "2026-05-23T05:01:00Z",
      },
    });

    renderPanel({
      activeConversation: { ...projectConversation(), agentMode: "plan" },
      activeConversationMode: "plan",
      activeWorkspace: {
        ...workspace(),
        mode: "plan",
        linkedIdeationSessionId: "planning-session-1",
      },
      attachedIdeationSessionId: "planning-session-1",
    });

    await user.click(await screen.findByTestId("question-plan-approval-action"));

    await waitFor(() =>
      expect(approvePlanArtifactMock).toHaveBeenCalledWith({
        sessionId: "planning-session-1",
        artifactId: "artifact-1",
      }),
    );
  });

  it("shows a composer-adjacent Approve Plan CTA for draft Plan-mode sessions", async () => {
    const user = userEvent.setup();
    getSessionPlanMock.mockResolvedValue(planArtifact("draft"));
    approvePlanArtifactMock.mockResolvedValue(planArtifact("approved"));

    renderPanel({
      activeConversation: { ...projectConversation(), agentMode: "plan" },
      activeConversationMode: "plan",
      activeWorkspace: {
        ...workspace(),
        mode: "plan",
        linkedIdeationSessionId: "planning-session-1",
      },
      attachedIdeationSessionId: "planning-session-1",
    });

    const row = await screen.findByTestId("agents-plan-composer-cta-row");
    expect(row).toHaveTextContent(/Approve draft plan/i);

    await user.click(within(row).getByRole("button", { name: /Approve Plan/i }));

    await waitFor(() =>
      expect(approvePlanArtifactMock).toHaveBeenCalledWith({
        sessionId: "planning-session-1",
        artifactId: "artifact-1",
      }),
    );
  });

  it("emphasizes Create Proposals in the composer CTA row when complexity recommends it", async () => {
    const user = userEvent.setup();
    getSessionPlanMock.mockResolvedValue(planArtifact("approved"));
    getPlanComplexityAssessmentMock.mockResolvedValue({
      id: "assessment-1",
      sessionId: "planning-session-1",
      artifactId: "artifact-1",
      artifactVersion: 1,
      level: "complex",
      score: 82,
      recommendedAction: "create_proposals",
      confidence: 0.9,
      reasonSummary: "The plan spans several tracked phases.",
      signals: {},
      assessedBy: "ralphx-utility-plan-complexity",
      createdAt: "2026-05-23T05:02:00Z",
      updatedAt: "2026-05-23T05:02:00Z",
    });
    switchAgentConversationModeMock.mockResolvedValue({
      workspace: {
        ...workspace(),
        mode: "ideation",
        linkedIdeationSessionId: "planning-session-1",
      },
    });

    renderPanel({
      activeConversation: { ...projectConversation(), agentMode: "plan" },
      activeConversationMode: "plan",
      activeWorkspace: {
        ...workspace(),
        mode: "plan",
        linkedIdeationSessionId: "planning-session-1",
      },
      attachedIdeationSessionId: "planning-session-1",
    });

    const row = await screen.findByTestId("agents-plan-composer-cta-row");
    await waitFor(() =>
      expect(
        within(row).getByTestId("agents-plan-composer-cta-create-proposals"),
      ).toHaveClass("bg-primary"),
    );
    const recommendedAction = within(row).getByRole("button", {
      name: /Create Proposals/i,
    });
    expect(row).toHaveClass("rounded-md", "border");
    expect(
      within(row).getByTestId("agents-plan-composer-cta-hint"),
    ).toHaveTextContent("Recommended: Create Proposals");
    expect(row).not.toHaveTextContent(/The plan spans several tracked phases/i);
    expect(
      within(row).getByRole("button", { name: /why\?/i }),
    ).toBeInTheDocument();
    await user.hover(
      within(row).getByRole("button", { name: /why\?/i }),
    );
    await waitFor(() =>
      expect(screen.getAllByText(/The plan spans several tracked phases/i).length)
        .toBeGreaterThan(0),
    );
    const actions = within(row).getByTestId("agents-plan-composer-cta-actions");
    expect(actions).toHaveClass("flex-wrap", "items-center");
    const actionButtons = within(actions).getAllByRole("button");
    expect(actionButtons).toHaveLength(3);
    expect(actionButtons[0]).toHaveTextContent("Create Proposals");
    expect(actionButtons[1]).toHaveTextContent("Implement Directly");
    expect(actionButtons[2]).toHaveTextContent("Verify Plan");
    expect(actionButtons[0]).toHaveClass("bg-primary");

    await user.click(recommendedAction);

    await waitFor(() =>
      expect(switchAgentConversationModeMock).toHaveBeenCalledWith({
        conversationId: "conversation-1",
        mode: "ideation",
      }),
    );
    expect(sendAgentMessageMock).toHaveBeenCalledWith(
      "ideation",
      "planning-session-1",
      expect.stringContaining("Proceed to proposals"),
    );
  });

  it("hides the composer CTA row while question UI is active", async () => {
    composerQuestionModeRef.current = {
      optionCount: 3,
      multiSelect: false,
    };
    getSessionPlanMock.mockResolvedValue(planArtifact("approved"));

    renderPanel({
      activeConversation: { ...projectConversation(), agentMode: "plan" },
      activeConversationMode: "plan",
      activeWorkspace: {
        ...workspace(),
        mode: "plan",
        linkedIdeationSessionId: "planning-session-1",
      },
      attachedIdeationSessionId: "planning-session-1",
    });

    await waitFor(() => expect(getSessionPlanMock).toHaveBeenCalled());

    expect(
      screen.queryByTestId("agents-plan-composer-cta-row"),
    ).not.toBeInTheDocument();
  });

  it("starts direct implementation from the composer CTA row with the selected runtime", async () => {
    const user = userEvent.setup();
    getSessionPlanMock.mockResolvedValue(planArtifact("approved"));
    switchAgentConversationModeMock.mockResolvedValue({
      workspace: {
        ...workspace(),
        mode: "edit",
        linkedIdeationSessionId: "planning-session-1",
      },
    });

    renderPanel({
      activeConversation: { ...projectConversation(), agentMode: "plan" },
      activeConversationMode: "plan",
      activeWorkspace: {
        ...workspace(),
        mode: "plan",
        linkedIdeationSessionId: "planning-session-1",
      },
      attachedIdeationSessionId: "planning-session-1",
      normalizedActiveRuntime: {
        provider: "codex",
        modelId: "gpt-5.5",
        effort: "high",
      },
    });

    const row = await screen.findByTestId("agents-plan-composer-cta-row");
    await user.click(
      within(row).getByRole("button", { name: /Implement Directly/i }),
    );

    await waitFor(() =>
      expect(switchAgentConversationModeMock).toHaveBeenCalledWith({
        conversationId: "conversation-1",
        mode: "edit",
      }),
    );
    expect(sendAgentMessageMock).toHaveBeenCalledWith(
      "project",
      "project-1",
      expect.stringContaining("Implement the approved plan directly"),
      undefined,
      undefined,
      {
        conversationId: "conversation-1",
        providerHarness: "codex",
        modelId: "gpt-5.5",
        logicalEffort: "high",
      },
    );
  });

  it("starts plan verification from the composer CTA row", async () => {
    const user = userEvent.setup();
    const onSelectArtifact = vi.fn();
    getSessionPlanMock.mockResolvedValue(planArtifact("approved"));
    getVerificationSpecialistsMock.mockResolvedValue({
      specialists: [
        { name: "risk", enabled_by_default: false },
        { name: "scope", enabled_by_default: true },
      ],
    });

    renderPanel({
      activeConversation: { ...projectConversation(), agentMode: "plan" },
      activeConversationMode: "plan",
      activeWorkspace: {
        ...workspace(),
        mode: "plan",
        linkedIdeationSessionId: "planning-session-1",
      },
      attachedIdeationSessionId: "planning-session-1",
      onSelectArtifact,
    });

    const row = await screen.findByTestId("agents-plan-composer-cta-row");
    await user.click(within(row).getByRole("button", { name: /Verify Plan/i }));

    await waitFor(() =>
      expect(confirmVerificationMock).toHaveBeenCalledWith(
        "planning-session-1",
        ["risk"],
      ),
    );
    expect(onSelectArtifact).toHaveBeenCalledWith("verification");
  });

  it("requires confirmation before running the typed fork command", async () => {
    const user = userEvent.setup();
    const onForkConversation = vi.fn().mockResolvedValue(forkResult());
    renderPanel({ onForkConversation });

    await user.click(screen.getByTestId("send-fork-command"));

    expect(screen.getByText("Fork session?")).toBeInTheDocument();
    expect(onForkConversation).not.toHaveBeenCalled();

    await user.click(screen.getByRole("button", { name: "Fork session" }));

    await waitFor(() =>
      expect(onForkConversation).toHaveBeenCalledWith("conversation-1"),
    );
  });

  it("sends a follow-up message to the forked conversation after confirmation", async () => {
    const user = userEvent.setup();
    const onAgentUserMessageSent = vi.fn();
    const onForkConversation = vi.fn().mockResolvedValue(forkResult());
    renderPanel({
      normalizedActiveRuntime: {
        provider: "codex",
        modelId: "gpt-5.5",
        effort: "high",
      },
      onAgentUserMessageSent,
      onForkConversation,
    });

    await user.click(screen.getByTestId("send-fork-followup-command"));
    await user.click(screen.getByRole("button", { name: "Fork session" }));

    await waitFor(() =>
      expect(chatApi.sendAgentMessage).toHaveBeenCalledWith(
        "project",
        "project-1",
        "continue this thread",
        undefined,
        undefined,
        {
          conversationId: "conversation-fork",
          providerHarness: "codex",
          modelId: "gpt-5.5",
          logicalEffort: "high",
        },
      ),
    );
    expect(onAgentUserMessageSent).toHaveBeenCalledWith({
      content: "continue this thread",
      result: {
        conversationId: "conversation-fork",
        agentRunId: "run-fork",
        isNewConversation: false,
        wasQueued: false,
        queuedAsPending: false,
        queuedMessageId: null,
      },
    });
  });

  it("requires confirmation before running the composer fork action", async () => {
    const user = userEvent.setup();
    const onForkConversation = vi.fn().mockResolvedValue(forkResult());
    renderPanel({ onForkConversation });

    await user.click(screen.getByTestId("composer-fork-action"));

    expect(screen.getByText("Fork session?")).toBeInTheDocument();
    expect(onForkConversation).not.toHaveBeenCalled();

    await user.click(screen.getByRole("button", { name: "Fork session" }));

    await waitFor(() =>
      expect(onForkConversation).toHaveBeenCalledWith("conversation-1"),
    );
  });
});
