import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import type { ComponentProps, ReactNode } from "react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

import {
  chatApi,
  type AgentConversationWorkspace,
  type AgentConversationWorkspaceFreshness,
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
  eventSubscribers,
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
  eventSubscribers: new Map<string, Set<(payload: unknown) => void>>(),
}));

vi.mock("@/components/Chat/IntegratedChatPanel", () => ({
  IntegratedChatPanel: ({
    additionalQuestionSessionIds,
    planApprovalAction,
    onQuestionAnswered,
    renderComposer,
  }: {
    additionalQuestionSessionIds?: string[];
    planApprovalAction?: {
      label: string;
      onClick: () => void;
      disabled?: boolean;
      isPending?: boolean;
    };
    onQuestionAnswered?: (
      question: Record<string, unknown>,
      response: Record<string, unknown>,
      result: Record<string, unknown>,
    ) => void | Promise<void>;
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
      {onQuestionAnswered && (
        <>
          <button
            type="button"
            data-testid="accept-plan-mode-proposal"
            onClick={() => {
              void onQuestionAnswered(
                {
                  requestId: "req-plan-mode",
                  sessionId: "conversation-1",
                  question: "Switch this conversation to Plan mode?",
                  options: [],
                  multiSelect: false,
                  allowSkip: true,
                  metadata: {
                    kind: "plan_mode_proposal",
                    conversation_id: "conversation-1",
                    reason: "The CLI surface needs planning before implementation.",
                  },
                },
                {
                  requestId: "req-plan-mode",
                  selectedOptions: ["switch_to_plan"],
                },
                { success: true, deliveredToWaitingAgent: true },
              );
            }}
          >
            Accept plan proposal
          </button>
          <button
            type="button"
            data-testid="skip-plan-mode-proposal"
            onClick={() => {
              void onQuestionAnswered(
                {
                  requestId: "req-plan-mode",
                  sessionId: "conversation-1",
                  question: "Switch this conversation to Plan mode?",
                  options: [],
                  multiSelect: false,
                  allowSkip: true,
                  metadata: {
                    kind: "plan_mode_proposal",
                    conversation_id: "conversation-1",
                    reason: "The CLI surface needs planning before implementation.",
                  },
                },
                {
                  requestId: "req-plan-mode",
                  selectedOptions: [],
                  skipped: true,
                },
                { success: true, deliveredToWaitingAgent: true },
              );
            }}
          >
            Skip plan proposal
          </button>
          <button
            type="button"
            data-testid="accept-backend-handled-plan-mode-proposal"
            onClick={() => {
              void onQuestionAnswered(
                {
                  requestId: "req-plan-mode",
                  sessionId: "conversation-1",
                  question: "Switch this conversation to Plan mode?",
                  options: [],
                  multiSelect: false,
                  allowSkip: true,
                  metadata: {
                    kind: "plan_mode_proposal",
                    conversation_id: "conversation-1",
                    reason: "The CLI surface needs planning before implementation.",
                  },
                },
                {
                  requestId: "req-plan-mode",
                  selectedOptions: ["switch_to_plan"],
                },
                {
                  success: true,
                  deliveredToWaitingAgent: true,
                  planModeProposalHandled: true,
                },
              );
            }}
          >
            Accept backend-handled plan proposal
          </button>
        </>
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

vi.mock("@/providers/EventProvider", () => ({
  useEventBus: () => ({
    subscribe: (eventName: string, handler: (payload: unknown) => void) => {
      const subscribers = eventSubscribers.get(eventName) ?? new Set();
      subscribers.add(handler);
      eventSubscribers.set(eventName, subscribers);
      return () => {
        subscribers.delete(handler);
      };
    },
  }),
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
      {
        provider: "codex",
        enabled: true,
        isDefault: false,
        model: null,
        effort: null,
        approvalPolicy: null,
        sandboxMode: null,
        claudePermissionMode: null,
        claudeDangerouslySkipPermissions: false,
        claudeAllowDangerouslySkipPermissions: false,
        available: true,
        binaryFound: true,
        binaryPath: "/tmp/codex",
        status: "ready",
        error: null,
        missingCoreExecFeatures: [],
        supportedEfforts: ["low", "medium", "high", "xhigh"],
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
    provider,
    model,
    effort,
    mode,
    showHelperText,
    onSend,
    onForkSession,
  }: {
    provider: {
      value: string;
      disabled?: boolean;
      onValueChange: (value: "claude" | "codex") => void;
    };
    model: { value: string; onValueChange: (value: string) => void };
    effort: { value: string; onValueChange: (value: string) => void };
    mode?: {
      value: string;
      disabled?: boolean;
      onOpen?: () => void;
      onValueChange: (value: string) => void;
      options: Array<{
        id: string;
        label: string;
        disabled?: boolean;
        disabledReason?: string;
      }>;
    };
    showHelperText?: boolean;
    onSend: (message: string) => Promise<void> | void;
    onForkSession?: () => Promise<unknown> | void;
  }) => (
    <div>
      <div data-testid="workspace-provider-value">{provider.value}</div>
      <div data-testid="workspace-model-value">{model.value}</div>
      <div data-testid="workspace-effort-value">{effort.value}</div>
      <div data-testid="workspace-helper-enabled">{String(showHelperText !== false)}</div>
      {mode && (
        <div>
          <button
            type="button"
            data-testid="agent-composer-mode-chip"
            disabled={mode.disabled}
            onClick={() => mode.onOpen?.()}
          >
            {mode.value}
          </button>
          {mode.options.map((option) => {
            const disabled = mode.disabled || option.disabled;
            return (
              <button
                key={option.id}
                type="button"
                data-testid={`agent-mode-option-${option.id}`}
                disabled={disabled}
                onClick={() => {
                  if (!disabled) {
                    mode.onValueChange(option.id);
                  }
                }}
              >
                {option.label}
                {option.disabledReason ? (
                  <span>{option.disabledReason}</span>
                ) : null}
              </button>
            );
          })}
        </div>
      )}
      <button
        type="button"
        data-testid="change-workspace-provider"
        disabled={provider.disabled}
        onClick={() => provider.onValueChange("codex")}
      />
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

function emitEvent(eventName: string, payload: unknown) {
  eventSubscribers.get(eventName)?.forEach((handler) => handler(payload));
}

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

function workspaceFreshness(
  overrides: Partial<AgentConversationWorkspaceFreshness> = {},
): AgentConversationWorkspaceFreshness {
  return {
    conversationId: "conversation-1",
    freshnessScope: "local",
    baseRef: "main",
    baseDisplayName: "Project default (main)",
    targetRef: "origin/main",
    capturedBaseCommit: "base-sha",
    targetBaseCommit: "base-sha",
    isBaseAhead: false,
    hasUncommittedChanges: false,
    unpublishedCommitCount: null,
    remoteRefreshed: true,
    worktreeStatusChecked: true,
    baseStatus: "valid",
    effectiveBaseRef: null,
    effectiveBaseDisplayName: null,
    baseBlockReason: null,
    ...overrides,
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
    onActiveProviderChange: vi.fn(),
    onAgentUserMessageSent: vi.fn(),
    onConversationModeSwitched: vi.fn(),
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
    eventSubscribers.clear();
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

  it("normalizes workspace runtime and forwards provider-supported capabilities", () => {
    const onActiveModelChange = vi.fn();
    const onActiveEffortChange = vi.fn();
    renderPanel({ onActiveEffortChange, onActiveModelChange });

    expect(screen.getByTestId("workspace-provider-value").textContent).toBe("claude");
    expect(screen.getByTestId("workspace-effort-value").textContent).toBe("high");
    expect(screen.getByTestId("workspace-helper-enabled").textContent).toBe("true");

    fireEvent.click(screen.getByTestId("change-workspace-model"));
    fireEvent.click(screen.getByTestId("change-workspace-effort"));

    expect(onActiveModelChange).toHaveBeenCalledWith("sonnet", [
      "low",
      "medium",
      "high",
      "max",
    ], null);
    expect(onActiveEffortChange).toHaveBeenCalledWith("max", [
      "low",
      "medium",
      "high",
      "max",
    ], null);
  });

  it("allows provider changes in an existing workspace conversation", () => {
    const onActiveProviderChange = vi.fn();
    renderPanel({ onActiveProviderChange });

    const providerButton = screen.getByTestId("change-workspace-provider");
    expect(providerButton).not.toBeDisabled();

    fireEvent.click(providerButton);

    expect(onActiveProviderChange).toHaveBeenCalledWith("codex", [
      "low",
      "medium",
      "high",
      "xhigh",
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

  it("bridges active Chat-mode conversation questions into the workspace chat", () => {
    renderPanel({
      activeConversation: { ...projectConversation(), agentMode: "chat" },
      activeConversationMode: "chat",
      activeWorkspace: { ...workspace(), mode: "chat" },
      attachedIdeationSessionId: null,
    });

    expect(screen.getByTestId("integrated-chat-panel")).toHaveAttribute(
      "data-question-session-ids",
      "conversation-1",
    );
  });

  it("lets an unlocked ideation workspace select Agent mode from the composer", async () => {
    const user = userEvent.setup();
    const onActiveConversationModeChange = vi.fn();
    const onActiveConversationModeMenuOpen = vi.fn();

    renderPanel({
      activeConversation: { ...projectConversation(), agentMode: "ideation" },
      activeConversationMode: "ideation",
      activeConversationModeLocked: false,
      activeWorkspace: {
        ...workspace(),
        mode: "ideation",
        linkedPlanBranchId: "plan-branch-1",
        modeSwitchLocked: false,
      },
      onActiveConversationModeChange,
      onActiveConversationModeMenuOpen,
    });

    await user.click(screen.getByTestId("agent-composer-mode-chip"));
    const agentOption = screen.getByTestId("agent-mode-option-edit");

    await user.click(agentOption);

    expect(onActiveConversationModeMenuOpen).toHaveBeenCalledTimes(1);
    expect(onActiveConversationModeChange).toHaveBeenCalledWith("edit");
  });

  it("disables Agent mode in the composer while ideation execution owns the workspace", async () => {
    const user = userEvent.setup();
    const onActiveConversationModeChange = vi.fn();

    renderPanel({
      activeConversation: { ...projectConversation(), agentMode: "ideation" },
      activeConversationMode: "ideation",
      activeConversationModeLocked: true,
      activeWorkspace: {
        ...workspace(),
        mode: "ideation",
        linkedPlanBranchId: "plan-branch-1",
        modeSwitchLocked: true,
        modeSwitchLockReason: "Plan execution is still active",
      },
      onActiveConversationModeChange,
    });

    await user.click(screen.getByTestId("agent-composer-mode-chip"));
    const agentOption = screen.getByTestId("agent-mode-option-edit");
    expect(agentOption).toBeDisabled();
    expect(
      within(agentOption).getByText("Plan execution is still active"),
    ).toBeInTheDocument();

    await user.click(agentOption);

    expect(onActiveConversationModeChange).not.toHaveBeenCalled();
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
    const promotedWorkspace = {
      ...workspace(),
      mode: "ideation" as const,
      linkedIdeationSessionId: "planning-session-1",
    };
    switchAgentConversationModeMock.mockResolvedValue({
      workspace: promotedWorkspace,
    });
    const onConversationModeSwitched = vi.fn();

    renderPanel({
      activeConversation: { ...projectConversation(), agentMode: "plan" },
      activeConversationMode: "plan",
      activeWorkspace: {
        ...workspace(),
        mode: "plan",
        linkedIdeationSessionId: "planning-session-1",
      },
      attachedIdeationSessionId: "planning-session-1",
      onConversationModeSwitched,
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
    expect(onConversationModeSwitched).toHaveBeenCalledWith(
      "conversation-1",
      "ideation",
      promotedWorkspace,
    );
  });

  it("hides approved plan composer CTAs when the workspace has changes", async () => {
    getSessionPlanMock.mockResolvedValue(planArtifact("approved"));

    renderPanel({
      activeConversation: { ...projectConversation(), agentMode: "plan" },
      activeConversationMode: "plan",
      activeWorkspace: {
        ...workspace(),
        mode: "plan",
        linkedIdeationSessionId: "planning-session-1",
      },
      activeWorkspaceFreshness: workspaceFreshness({
        hasUncommittedChanges: true,
      }),
      attachedIdeationSessionId: "planning-session-1",
    });

    await waitFor(() =>
      expect(screen.queryByTestId("agents-plan-composer-cta-row")).not.toBeInTheDocument(),
    );
    expect(
      screen.queryByRole("button", { name: /Verify Plan/i }),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: /Implement Directly/i }),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: /Create Proposals/i }),
    ).not.toBeInTheDocument();
  });

  it("hides plan composer CTAs once the workspace has switched to edit mode", async () => {
    getSessionPlanMock.mockResolvedValue(planArtifact("approved"));

    renderPanel({
      activeConversation: { ...projectConversation(), agentMode: "plan" },
      activeConversationMode: "plan",
      activeWorkspace: {
        ...workspace(),
        mode: "edit",
        linkedIdeationSessionId: "planning-session-1",
      },
      attachedIdeationSessionId: "planning-session-1",
    });

    await waitFor(() =>
      expect(screen.queryByTestId("agents-plan-composer-cta-row")).not.toBeInTheDocument(),
    );
    expect(
      screen.queryByRole("button", { name: /Verify Plan/i }),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: /Implement Directly/i }),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: /Create Proposals/i }),
    ).not.toBeInTheDocument();
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

  it("switches to Plan mode when the user accepts a plan-mode proposal question", async () => {
    const user = userEvent.setup();
    const planWorkspace = { ...workspace(), mode: "plan" as const };
    switchAgentConversationModeMock.mockResolvedValue({
      workspace: planWorkspace,
    });
    const onConversationModeSwitched = vi.fn();
    const onAgentUserMessageSent = vi.fn();

    renderPanel({
      activeConversation: { ...projectConversation(), agentMode: "edit" },
      activeConversationMode: "edit",
      activeWorkspace: { ...workspace(), mode: "edit" },
      onConversationModeSwitched,
      onAgentUserMessageSent,
    });

    await user.click(screen.getByTestId("accept-plan-mode-proposal"));

    await waitFor(() =>
      expect(switchAgentConversationModeMock).toHaveBeenCalledWith({
        conversationId: "conversation-1",
        mode: "plan",
      }),
    );
    expect(onConversationModeSwitched).toHaveBeenCalledWith(
      "conversation-1",
      "plan",
      planWorkspace,
    );
    await waitFor(() =>
      expect(sendAgentMessageMock).toHaveBeenCalledWith(
        "project",
        "project-1",
        expect.stringContaining(
          "Planning focus: The CLI surface needs planning before implementation.",
        ),
        undefined,
        undefined,
        expect.objectContaining({
          conversationId: "conversation-1",
          providerHarness: "claude",
          modelId: "opus",
          logicalEffort: "xhigh",
        }),
      ),
    );
    expect(onAgentUserMessageSent).toHaveBeenCalledWith(
      expect.objectContaining({
        content: expect.stringContaining("Continue in Plan mode"),
      }),
    );
  });

  it("does not duplicate the Plan-mode switch when the backend handled the accepted proposal", async () => {
    const user = userEvent.setup();

    renderPanel({
      activeConversation: { ...projectConversation(), agentMode: "edit" },
      activeConversationMode: "edit",
      activeWorkspace: { ...workspace(), mode: "edit" },
    });

    await user.click(screen.getByTestId("accept-backend-handled-plan-mode-proposal"));

    expect(switchAgentConversationModeMock).not.toHaveBeenCalled();
    expect(sendAgentMessageMock).not.toHaveBeenCalled();
  });

  it("retries the Plan-mode proposal switch after the active agent run completes", async () => {
    const user = userEvent.setup();
    const planWorkspace = { ...workspace(), mode: "plan" as const };
    switchAgentConversationModeMock
      .mockRejectedValueOnce(
        new Error("Cannot change mode while the agent is running"),
      )
      .mockResolvedValueOnce({
        workspace: planWorkspace,
      });
    const onConversationModeSwitched = vi.fn();

    renderPanel({
      activeConversation: { ...projectConversation(), agentMode: "edit" },
      activeConversationMode: "edit",
      activeWorkspace: { ...workspace(), mode: "edit" },
      onConversationModeSwitched,
    });

    await user.click(screen.getByTestId("accept-plan-mode-proposal"));

    await waitFor(() =>
      expect(switchAgentConversationModeMock).toHaveBeenCalledTimes(1),
    );
    expect(onConversationModeSwitched).not.toHaveBeenCalled();

    emitEvent("agent:run_completed", {
      conversation_id: "conversation-1",
      context_type: "project",
      context_id: "conversation-1",
    });

    await waitFor(() =>
      expect(switchAgentConversationModeMock).toHaveBeenCalledTimes(2),
    );
    expect(onConversationModeSwitched).toHaveBeenCalledWith(
      "conversation-1",
      "plan",
      planWorkspace,
    );
  });

  it("keeps retrying while the Plan-mode switch still hits the running-agent guard", async () => {
    const user = userEvent.setup();
    const planWorkspace = { ...workspace(), mode: "plan" as const };
    const runningError = new Error(
      "Cannot change mode while the agent is running",
    );
    switchAgentConversationModeMock
      .mockRejectedValueOnce(runningError)
      .mockRejectedValueOnce(runningError)
      .mockResolvedValueOnce({
        workspace: planWorkspace,
      });
    const onConversationModeSwitched = vi.fn();

    renderPanel({
      activeConversation: { ...projectConversation(), agentMode: "edit" },
      activeConversationMode: "edit",
      activeWorkspace: { ...workspace(), mode: "edit" },
      onConversationModeSwitched,
    });

    await user.click(screen.getByTestId("accept-plan-mode-proposal"));

    await waitFor(() =>
      expect(switchAgentConversationModeMock).toHaveBeenCalledTimes(1),
    );

    emitEvent("agent:run_completed", {
      conversation_id: "conversation-1",
      context_type: "project",
      context_id: "conversation-1",
    });

    await waitFor(() =>
      expect(switchAgentConversationModeMock).toHaveBeenCalledTimes(2),
    );
    expect(onConversationModeSwitched).not.toHaveBeenCalled();

    await waitFor(
      () => expect(switchAgentConversationModeMock).toHaveBeenCalledTimes(3),
      { timeout: 2500 },
    );
    expect(onConversationModeSwitched).toHaveBeenCalledWith(
      "conversation-1",
      "plan",
      planWorkspace,
    );
  });

  it("keeps the current mode when the user skips a plan-mode proposal question", async () => {
    const user = userEvent.setup();

    renderPanel({
      activeConversation: { ...projectConversation(), agentMode: "chat" },
      activeConversationMode: "chat",
      activeWorkspace: { ...workspace(), mode: "chat" },
    });

    await user.click(screen.getByTestId("skip-plan-mode-proposal"));

    expect(switchAgentConversationModeMock).not.toHaveBeenCalled();
  });

  it("starts direct implementation from the composer CTA row with the selected runtime", async () => {
    const user = userEvent.setup();
    getSessionPlanMock.mockResolvedValue(planArtifact("approved"));
    const editWorkspace = {
      ...workspace(),
      mode: "edit" as const,
      linkedIdeationSessionId: "planning-session-1",
    };
    switchAgentConversationModeMock.mockResolvedValue({
      workspace: editWorkspace,
    });
    const onConversationModeSwitched = vi.fn();

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
      onConversationModeSwitched,
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
        suppressUserMessage: true,
      },
    );
    expect(sendAgentMessageMock.mock.calls[0]?.[2]).not.toContain(
      "do not create task proposals",
    );
    expect(onConversationModeSwitched).toHaveBeenCalledWith(
      "conversation-1",
      "edit",
      editWorkspace,
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
