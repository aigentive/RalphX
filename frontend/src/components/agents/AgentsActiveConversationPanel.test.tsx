import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import type { ComponentProps, ReactNode } from "react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

import {
  chatApi,
  type AgentConversationRuntimeStatus,
  type AgentConversationWorkspace,
  type AgentConversationWorkspaceFreshness,
  type ForkAgentConversationResult,
} from "@/api/chat";
import { TooltipProvider } from "@/components/ui/tooltip";
import { useAgentSessionStore } from "@/stores/agentSessionStore";

import type { AgentConversation } from "./agentConversations";
import { AgentsActiveConversationPanel } from "./AgentsActiveConversationPanel";
import { useAgentArtifactUiStore } from "./agentArtifactUiStore";

const {
  getSessionPlanMock,
  getPlanComplexityAssessmentMock,
  approvePlanArtifactMock,
  sendAgentMessageMock,
  switchAgentConversationModeMock,
  getAgentConversationRuntimeStatusesMock,
  useVerificationStatusMock,
  getVerificationSpecialistsMock,
  confirmVerificationMock,
  composerQuestionModeRef,
  composerAgentStatusRef,
  eventSubscribers,
  openUrlMock,
} = vi.hoisted(() => ({
  getSessionPlanMock: vi.fn(),
  getPlanComplexityAssessmentMock: vi.fn(),
  approvePlanArtifactMock: vi.fn(),
  sendAgentMessageMock: vi.fn(),
  switchAgentConversationModeMock: vi.fn(),
  getAgentConversationRuntimeStatusesMock: vi.fn(),
  useVerificationStatusMock: vi.fn(),
  getVerificationSpecialistsMock: vi.fn(),
  confirmVerificationMock: vi.fn(),
  composerQuestionModeRef: { current: undefined as unknown },
  composerAgentStatusRef: { current: "idle" },
  eventSubscribers: new Map<string, Set<(payload: unknown) => void>>(),
  openUrlMock: vi.fn(),
}));

vi.mock("@tauri-apps/plugin-opener", () => ({
  openUrl: (...args: unknown[]) => openUrlMock(...args),
}));

vi.mock("@/components/Chat/IntegratedChatPanel", () => ({
  IntegratedChatPanel: ({
    additionalQuestionSessionIds,
    agentProcessContextIdOverride,
    conversationIdOverride,
    headerContent,
    planApprovalAction,
    onQuestionAnswered,
    renderComposer,
    storeContextKeyOverride,
  }: {
    additionalQuestionSessionIds?: string[];
    agentProcessContextIdOverride?: string;
    conversationIdOverride?: string;
    headerContent?: ReactNode;
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
    storeContextKeyOverride?: string;
  }) => (
    <div
      data-testid="integrated-chat-panel"
      data-question-session-ids={additionalQuestionSessionIds?.join(",") ?? ""}
      data-agent-process-context-id={agentProcessContextIdOverride ?? ""}
      data-conversation-id={conversationIdOverride ?? ""}
      data-store-context-key={storeContextKeyOverride ?? ""}
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
      {headerContent}
      {renderComposer({
        onSend: vi.fn(),
        onStop: vi.fn(),
        agentStatus: composerAgentStatusRef.current,
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
      getAgentConversationRuntimeStatuses: getAgentConversationRuntimeStatusesMock,
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
        supportsFastMode: false,
        fastModeSupportedModels: [],
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
        supportsFastMode: true,
        fastModeSupportedModels: ["gpt-5.5", "gpt-5.4"],
        updatedAt: "2026-05-16T00:00:00.000Z",
      },
    ],
    isLoading: false,
    isPlaceholderData: false,
  }),
}));

vi.mock("@/stores/chatStore", () => ({
  selectQueuedMessages: () => () => [],
  useChatStore: (
    selector?: (state: {
      agentStatus: Record<string, string>;
      isSending: Record<string, boolean>;
    }) => unknown,
  ) =>
    selector ? selector({ agentStatus: {}, isSending: {} }) : [],
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
  AgentConversationBaseLine: ({
    disabled,
    editable,
    prefixLabel,
  }: {
    disabled?: boolean;
    editable?: boolean;
    prefixLabel?: string;
  }) => (
    <div
      data-testid="mock-agent-conversation-base-line"
      data-disabled={String(disabled ?? false)}
      data-editable={String(editable ?? false)}
    >
      {prefixLabel}
    </div>
  ),
}));

vi.mock("./AgentsChatHeaderController", () => ({
  AgentsChatHeaderController: ({
    workspaceControl,
  }: {
    workspaceControl?: ReactNode;
  }) => <div data-testid="mock-agents-chat-header">{workspaceControl}</div>,
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

function workspaceRuntimeStatus(
  overrides: Partial<AgentConversationRuntimeStatus> = {},
): AgentConversationRuntimeStatus {
  return {
    conversationId: "conversation-1",
    isRunning: true,
    agentStatus: "generating",
    primarySource: "workspace",
    summaryLabel: "Agent running",
    items: [
      {
        source: "workspace",
        contextType: "project",
        contextId: "conversation-1",
        label: "Agent running",
        title: "Workspace chat",
        agentStatus: "generating",
        taskId: null,
        internalStatus: null,
        runningProcess: null,
        ideationSession: null,
        parentSessionId: null,
        childSessionId: null,
        conversationId: "conversation-1",
      },
    ],
    ...overrides,
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

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((promiseResolve, promiseReject) => {
    resolve = promiseResolve;
    reject = promiseReject;
  });
  return { promise, resolve, reject };
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
    onFocusWorkspaceReview: vi.fn(),
    onFocusVerificationSession: vi.fn(),
    onFocusTaskRuntime: vi.fn(),
    onOpenTaskArtifact: vi.fn(),
    onForkConversation: vi.fn().mockResolvedValue(forkResult()),
    onOpenPublishPane: vi.fn(),
    onOpenPlanArtifact: vi.fn(),
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
    selectedTaskArtifactId: null,
    setTerminalChatDockElement: vi.fn(),
    switchingConversationModeId: null,
    terminalArchivedReason: null,
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
    useAgentSessionStore.setState({ artifactByConversationId: {} });
    useAgentArtifactUiStore.setState({ artifactByConversationId: {} });
    composerQuestionModeRef.current = undefined;
    composerAgentStatusRef.current = "idle";
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
    getAgentConversationRuntimeStatusesMock.mockResolvedValue({});
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

  it("hides the composer runtime status for a single edit workspace runtime", async () => {
    getAgentConversationRuntimeStatusesMock.mockResolvedValue({
      "conversation-1": workspaceRuntimeStatus(),
    });

    renderPanel({
      activeConversation: { ...projectConversation(), agentMode: "edit" },
      activeConversationMode: "edit",
      activeWorkspace: { ...workspace(), mode: "edit" },
    });

    await waitFor(() =>
      expect(getAgentConversationRuntimeStatusesMock).toHaveBeenCalledWith([
        "conversation-1",
      ]),
    );
    expect(
      screen.queryByTestId("agents-runtime-status-widget"),
    ).not.toBeInTheDocument();
  });

  it("hides the composer runtime status for a single ideation workspace runtime without linked chats", async () => {
    getAgentConversationRuntimeStatusesMock.mockResolvedValue({
      "conversation-1": workspaceRuntimeStatus(),
    });

    renderPanel({
      activeConversation: { ...projectConversation(), agentMode: "ideation" },
      activeConversationMode: "ideation",
      activeWorkspace: { ...workspace(), mode: "ideation" },
    });

    await waitFor(() =>
      expect(getAgentConversationRuntimeStatusesMock).toHaveBeenCalledWith([
        "conversation-1",
      ]),
    );
    expect(
      screen.queryByTestId("agents-runtime-status-widget"),
    ).not.toBeInTheDocument();
  });

  it("keeps the composer runtime status for a single ideation workspace runtime when linked chats exist", async () => {
    getAgentConversationRuntimeStatusesMock.mockResolvedValue({
      "conversation-1": workspaceRuntimeStatus(),
    });

    renderPanel({
      activeConversation: { ...projectConversation(), agentMode: "ideation" },
      activeConversationMode: "ideation",
      activeWorkspace: { ...workspace(), mode: "ideation" },
      chatFocusOptions: [
        {
          type: "workspace",
          label: "Workspace",
          description: "Show the workspace agent chat",
        },
        {
          type: "ideation",
          label: "Ideation",
          description: "Show the attached ideation chat",
          tone: "accent",
        },
      ],
    });

    expect(
      await screen.findByTestId("agents-runtime-status-widget"),
    ).toHaveTextContent("Workspace chat");
  });

  it("opens the task artifact and focuses task chat from the runtime status CTA", async () => {
    const onFocusTaskRuntime = vi.fn();
    const onOpenTaskArtifact = vi.fn();
    const workspaceItem = workspaceRuntimeStatus().items[0]!;
    getAgentConversationRuntimeStatusesMock.mockResolvedValue({
      "conversation-1": workspaceRuntimeStatus({
        primarySource: "review",
        summaryLabel: "Runtime activity",
        items: [
          { ...workspaceItem, agentStatus: "waiting_for_input" },
          {
            source: "review",
            contextType: "review",
            contextId: "task-2",
            label: "Reviewing",
            title: "Review task",
            agentStatus: "generating",
            taskId: "task-2",
            internalStatus: "reviewing",
            runningProcess: null,
            ideationSession: null,
            parentSessionId: null,
            childSessionId: null,
            conversationId: "review-conversation-1",
          },
        ],
      }),
    });

    renderPanel({ onFocusTaskRuntime, onOpenTaskArtifact });

    fireEvent.click(await screen.findByRole("button", { name: "View Task" }));

    expect(onFocusTaskRuntime).toHaveBeenCalledWith("task-2", "review");
    expect(onOpenTaskArtifact).toHaveBeenCalledWith("task-2");
  });

  it("focuses the workspace Review chat from the runtime status CTA", async () => {
    const onFocusWorkspaceReview = vi.fn();
    const workspaceItem = workspaceRuntimeStatus().items[0]!;
    getAgentConversationRuntimeStatusesMock.mockResolvedValue({
      "conversation-1": workspaceRuntimeStatus({
        primarySource: "workspace_review",
        summaryLabel: "Reviewing",
        items: [
          { ...workspaceItem, agentStatus: "waiting_for_input" },
          {
            source: "workspace_review",
            contextType: "project",
            contextId: "review-conversation-1",
            label: "Reviewing",
            title: "Review workspace changes",
            agentStatus: "generating",
            taskId: null,
            internalStatus: "reviewing",
            runningProcess: null,
            ideationSession: null,
            parentSessionId: null,
            childSessionId: null,
            conversationId: "review-conversation-1",
          },
        ],
      }),
    });

    renderPanel({ onFocusWorkspaceReview });

    fireEvent.click(await screen.findByRole("button", { name: "View Review" }));

    expect(onFocusWorkspaceReview).toHaveBeenCalledWith("review-conversation-1");
  });

  it("routes workspace Review focus through the review child project chat", () => {
    renderPanel({
      chatFocus: {
        type: "workspace_review",
        conversationId: "review-conversation-1",
      },
    });

    const panel = screen.getByTestId("integrated-chat-panel");
    expect(panel).toHaveAttribute("data-conversation-id", "review-conversation-1");
    expect(panel).toHaveAttribute(
      "data-agent-process-context-id",
      "review-conversation-1",
    );
    expect(panel).toHaveAttribute(
      "data-store-context-key",
      "project:review-conversation-1",
    );
  });

  it("refines selected task artifact focus to the matching runtime context", async () => {
    const onFocusTaskRuntime = vi.fn();
    const workspaceItem = workspaceRuntimeStatus().items[0]!;
    getAgentConversationRuntimeStatusesMock.mockResolvedValue({
      "conversation-1": workspaceRuntimeStatus({
        primarySource: "merge",
        summaryLabel: "Runtime activity",
        items: [
          { ...workspaceItem, agentStatus: "waiting_for_input" },
          {
            source: "merge",
            contextType: "merge",
            contextId: "task-3",
            label: "Merging",
            title: "Merge task",
            agentStatus: "generating",
            taskId: "task-3",
            internalStatus: "merging",
            runningProcess: null,
            ideationSession: null,
            parentSessionId: null,
            childSessionId: null,
            conversationId: "merge-conversation-1",
          },
        ],
      }),
    });

    renderPanel({
      onFocusTaskRuntime,
      selectedTaskArtifactId: "task-3",
    });

    await waitFor(() =>
      expect(onFocusTaskRuntime).toHaveBeenCalledWith("task-3", "merge"),
    );
  });

  it("moves the base selector to the header and shows branch PR metadata below the composer", async () => {
    const user = userEvent.setup();
    openUrlMock.mockResolvedValue(undefined);

    renderPanel({
      activeWorkspace: {
        ...workspace(),
        branchName: "ralphx/demo/agent-conversation-1",
        publicationPrNumber: 42,
        publicationPrUrl: "https://github.com/mock/project/pull/42",
      },
    });

    const header = screen.getByTestId("mock-agents-chat-header");
    const baseLine = within(header).getByTestId(
      "mock-agent-conversation-base-line",
    );
    expect(baseLine).toHaveTextContent("BASE:");
    expect(baseLine).toHaveAttribute("data-editable", "true");

    expect(
      screen.getByTestId("agents-conversation-branch-line"),
    ).toHaveTextContent("agent-conversation-1");
    const prLink = screen.getByTestId("agents-conversation-pr-link");
    expect(prLink).toHaveTextContent("PR #42");

    await user.click(prLink);

    expect(openUrlMock).toHaveBeenCalledWith(
      "https://github.com/mock/project/pull/42",
    );
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

  it("keeps the mode picker enabled while the agent is waiting for input", async () => {
    const user = userEvent.setup();
    const onActiveConversationModeChange = vi.fn();
    composerAgentStatusRef.current = "waiting_for_input";

    renderPanel({
      activeConversation: { ...projectConversation(), agentMode: "edit" },
      activeConversationMode: "edit",
      activeConversationModeLocked: false,
      activeWorkspace: {
        ...workspace(),
        mode: "edit",
        modeSwitchLocked: false,
      },
      onActiveConversationModeChange,
    });

    const modeChip = screen.getByTestId("agent-composer-mode-chip");
    expect(modeChip).not.toBeDisabled();

    await user.click(modeChip);
    const planOption = screen.getByTestId("agent-mode-option-plan");
    expect(planOption).not.toBeDisabled();

    await user.click(planOption);

    expect(onActiveConversationModeChange).toHaveBeenCalledWith("plan");
  });

  it("keeps the mode picker disabled while the agent is generating", async () => {
    composerAgentStatusRef.current = "generating";

    renderPanel({
      activeConversation: { ...projectConversation(), agentMode: "edit" },
      activeConversationMode: "edit",
      activeConversationModeLocked: false,
      activeWorkspace: {
        ...workspace(),
        mode: "edit",
        modeSwitchLocked: false,
      },
    });

    expect(screen.getByTestId("agent-composer-mode-chip")).toBeDisabled();
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

  it("shows View Plan before Approve Plan when the plan tab is not visible", async () => {
    const user = userEvent.setup();
    const onOpenPlanArtifact = vi.fn();
    getSessionPlanMock.mockResolvedValue(planArtifact("draft"));
    approvePlanArtifactMock.mockResolvedValue(planArtifact("approved"));
    useAgentArtifactUiStore.getState().setArtifactState("conversation-1", {
      isOpen: false,
      activeTab: "tasks",
      taskMode: "graph",
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
      availableArtifactTabs: ["plan"],
      onOpenPlanArtifact,
    });

    const actionGroup = within(
      await screen.findByTestId("agents-plan-composer-cta-actions"),
    );
    const actionButtons = actionGroup.getAllByRole("button");
    const viewPlanButton = actionButtons[0];
    const approvePlanButton = actionButtons[1];
    expect(viewPlanButton).toBeDefined();
    expect(approvePlanButton).toBeDefined();
    expect(viewPlanButton!).toHaveTextContent("View Plan");
    expect(approvePlanButton!).toHaveTextContent("Approve Plan");

    await user.click(viewPlanButton!);

    expect(onOpenPlanArtifact).toHaveBeenCalledTimes(1);
    expect(approvePlanArtifactMock).not.toHaveBeenCalled();
  });

  it("hides View Plan when the plan tab is already visible", async () => {
    getSessionPlanMock.mockResolvedValue(planArtifact("draft"));
    useAgentArtifactUiStore.getState().setArtifactState("conversation-1", {
      isOpen: true,
      activeTab: "plan",
      taskMode: "graph",
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
      availableArtifactTabs: ["plan"],
    });

    const row = await screen.findByTestId("agents-plan-composer-cta-row");
    expect(
      within(row).queryByRole("button", { name: /View Plan/i }),
    ).not.toBeInTheDocument();
    expect(
      within(row).getByRole("button", { name: /Approve Plan/i }),
    ).toBeInTheDocument();
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

  it("shows and disables composer plan CTAs while the recommendation check is running", async () => {
    const user = userEvent.setup();
    const assessment = deferred<null>();
    const approvedPlan = planArtifact("approved");
    getSessionPlanMock.mockResolvedValue({
      ...approvedPlan,
      planApproval: {
        status: "approved",
        approvedArtifactId: "artifact-1",
        approvedVersion: 1,
        approvedAt: new Date().toISOString(),
      },
    });
    getPlanComplexityAssessmentMock.mockReturnValue(assessment.promise);

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
    expect(within(row).getByTestId("agents-plan-composer-cta-hint"))
      .toHaveTextContent(/Checking recommended next action/i);

    const implementButton = within(row).getByRole("button", {
      name: /Implement Directly/i,
    });
    const createButton = within(row).getByRole("button", {
      name: /Create Proposals/i,
    });
    const verifyButton = within(row).getByRole("button", {
      name: /Verify Plan/i,
    });

    expect(implementButton).toBeDisabled();
    expect(createButton).toBeDisabled();
    expect(verifyButton).toBeDisabled();

    await user.click(implementButton);
    await user.click(createButton);
    await user.click(verifyButton);

    expect(sendAgentMessageMock).not.toHaveBeenCalled();
    expect(confirmVerificationMock).not.toHaveBeenCalled();

    assessment.resolve(null);
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
        codexFastMode: false,
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
          codexFastMode: false,
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
