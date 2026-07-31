import {
  act,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import type { ComponentProps, ReactNode } from "react";
import userEvent from "@testing-library/user-event";
import { invoke } from "@tauri-apps/api/core";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import {
  chatApi,
  type AgentConversationRuntimeIndexRow,
  type AgentConversationRuntimeStatus,
  type AgentConversationWorkspace,
  type AgentConversationWorkspaceFreshness,
  type AgentWorkspacePrReviewContext,
  type ForkAgentConversationResult,
} from "@/api/chat";
import { PersonaChip } from "@/components/Chat/PersonaChip";
import { TooltipProvider } from "@/components/ui/tooltip";
import { useChatStore } from "@/stores/chatStore";
import { useAgentSessionStore } from "@/stores/agentSessionStore";

import type { AgentConversation } from "./agentConversations";
import { AgentsActiveConversationPanel } from "./AgentsActiveConversationPanel";
import { useAgentArtifactUiStore } from "./agentArtifactUiStore";
import { agentWorkspaceKeys } from "./agentWorkspaceQueries";
import type { ComposerRuntimeSpeedField } from "./composer/runtime/runtimeSelectorTypes";

const {
  getSessionPlanMock,
  getPlanComplexityAssessmentMock,
  approvePlanArtifactMock,
  sendAgentMessageMock,
  switchAgentConversationModeMock,
  activateAgentPlanDirectImplementationMock,
  activateAgentTaskPipelineMock,
  getAgentConversationRuntimeIndexMock,
  getAgentConversationRuntimeStatusesMock,
  getAgentConversationWorkspaceMock,
  getAgentWorkspacePrReviewContextMock,
  useVerificationStatusMock,
  getVerificationSpecialistsMock,
  confirmVerificationMock,
  composerQuestionModeRef,
  composerAgentStatusRef,
  composerPersonaControlRef,
  agentPersonasEnabledRef,
  personaQueryMock,
  switchPersonaMock,
  eventSubscribers,
  listAgentTaskListTasksMock,
  listAgentTaskListsMock,
  listAgentTasksMock,
  useAutomationDetailMock,
  invalidateAutomationQueriesMock,
  finalizeAutomationMock,
  triggerAutomationRunNowMock,
  openUrlMock,
  toastErrorMock,
  toastInfoMock,
  toastSuccessMock,
  tasksEnabledRef,
  confirmImplementDirectlyMock,
  confirmCreateProposalsMock,
} = vi.hoisted(() => ({
  getSessionPlanMock: vi.fn(),
  getPlanComplexityAssessmentMock: vi.fn(),
  approvePlanArtifactMock: vi.fn(),
  sendAgentMessageMock: vi.fn(),
  switchAgentConversationModeMock: vi.fn(),
  activateAgentPlanDirectImplementationMock: vi.fn(),
  activateAgentTaskPipelineMock: vi.fn(),
  getAgentConversationRuntimeIndexMock: vi.fn(),
  getAgentConversationRuntimeStatusesMock: vi.fn(),
  getAgentConversationWorkspaceMock: vi.fn(),
  getAgentWorkspacePrReviewContextMock: vi.fn(),
  useVerificationStatusMock: vi.fn(),
  getVerificationSpecialistsMock: vi.fn(),
  confirmVerificationMock: vi.fn(),
  composerQuestionModeRef: { current: undefined as unknown },
  composerAgentStatusRef: { current: "idle" },
  composerPersonaControlRef: { current: undefined as ReactNode },
  agentPersonasEnabledRef: { current: false },
  personaQueryMock: vi.fn(),
  switchPersonaMock: vi.fn(),
  eventSubscribers: new Map<string, Set<(payload: unknown) => void>>(),
  listAgentTaskListTasksMock: vi.fn(),
  listAgentTaskListsMock: vi.fn(),
  listAgentTasksMock: vi.fn(),
  useAutomationDetailMock: vi.fn(),
  invalidateAutomationQueriesMock: vi.fn(),
  finalizeAutomationMock: vi.fn(),
  triggerAutomationRunNowMock: vi.fn(),
  openUrlMock: vi.fn(),
  toastErrorMock: vi.fn(),
  toastInfoMock: vi.fn(),
  toastSuccessMock: vi.fn(),
  tasksEnabledRef: { current: true },
  confirmImplementDirectlyMock: vi.fn(),
  confirmCreateProposalsMock: vi.fn(),
}));

vi.mock("@/hooks/useIdeationSettings", () => ({
  useIdeationSettings: () => ({
    settings: {
      tasksEnabled: tasksEnabledRef.current,
      tasksFeatureState: tasksEnabledRef.current ? "enabled" : "disabled",
      autoVerifyPlans: false,
      autoVerifyDraftPlans: true,
      requireAcceptForFinalize: false,
      requireVerificationForAccept: false,
      externalOverrides: {},
    },
    isLoading: false,
    isError: false,
  }),
}));

const deferredHydrationTimeout = { timeout: 3_000 };
const { approvedPlanRuntime } = vi.hoisted(() => ({
  approvedPlanRuntime: {
    provider: "claude",
    model: "opus",
    effort: "high",
    serviceTier: "provider_default",
    coordinationMode: "solo",
    personaId: null,
  } as const,
}));

vi.mock("./useApprovedPlanContinuation", () => ({
  useApprovedPlanContinuation: () => ({
    confirmImplementDirectly: (
      onConfirm: (runtime: typeof approvedPlanRuntime) => Promise<unknown>,
    ) => confirmImplementDirectlyMock(onConfirm),
    confirmCreateProposals: (
      onConfirm: (runtime: typeof approvedPlanRuntime) => Promise<unknown>,
    ) => confirmCreateProposalsMock(onConfirm),
    confirmationDialogProps: {},
    ConfirmationDialog: () => null,
  }),
}));

vi.mock("@tauri-apps/plugin-opener", () => ({
  openUrl: (...args: unknown[]) => openUrlMock(...args),
}));

vi.mock("sonner", () => ({
  toast: {
    error: toastErrorMock,
    info: toastInfoMock,
    success: toastSuccessMock,
  },
}));

vi.mock("@/hooks/usePersonas", () => ({
  usePersonas: () => personaQueryMock(),
  useSwitchConversationPersona: () => ({
    isPending: false,
    mutateAsync: switchPersonaMock,
  }),
  usePersonaOverlayPreview: () => ({
    isPending: true,
    isError: false,
    data: undefined,
    error: null,
  }),
}));

vi.mock("@/components/Chat/IntegratedChatPanel", () => ({
  IntegratedChatPanel: ({
    additionalQuestionSessionIds,
    agentProcessContextIdOverride,
    conversationIdOverride,
    headerContent,
    planApprovalAction,
    onQuestionAnswered,
    onBuildPersona,
    renderComposer,
    sendOptions,
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
    onBuildPersona?: () => void;
    renderComposer: (props: Record<string, unknown>) => ReactNode;
    sendOptions?: {
      conversationId?: string;
      providerHarness?: string;
      modelId?: string;
      logicalEffort?: string;
      codexFastMode?: boolean | null;
    };
    storeContextKeyOverride?: string;
  }) => (
    <div
      data-testid="integrated-chat-panel"
      data-question-session-ids={additionalQuestionSessionIds?.join(",") ?? ""}
      data-agent-process-context-id={agentProcessContextIdOverride ?? ""}
      data-conversation-id={conversationIdOverride ?? ""}
      data-send-codex-fast-mode={
        sendOptions?.codexFastMode === undefined
          ? ""
          : String(sendOptions.codexFastMode)
      }
      data-send-conversation-id={sendOptions?.conversationId ?? ""}
      data-send-provider-harness={sendOptions?.providerHarness ?? ""}
      data-send-model-id={sendOptions?.modelId ?? ""}
      data-send-logical-effort={sendOptions?.logicalEffort ?? ""}
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
          <button
            type="button"
            data-testid="accept-automation-setup-proposal"
            onClick={() => {
              void onQuestionAnswered(
                {
                  requestId: "req-automation-setup",
                  sessionId: "conversation-1",
                  header: "Update automation?",
                  question: "Apply the proposed goal and phases to this automation?",
                  options: [],
                  multiSelect: false,
                  allowSkip: true,
                  metadata: {
                    kind: "automation_setup_proposal",
                  },
                },
                {
                  requestId: "req-automation-setup",
                  selectedOptions: ["apply_automation_proposal"],
                },
                { success: true, deliveredToWaitingAgent: true },
              );
            }}
          >
            Accept automation proposal
          </button>
        </>
      )}
      {headerContent}
      {onBuildPersona && (
        <button type="button" onClick={onBuildPersona}>
          Create persona for this project
        </button>
      )}
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
        ...(agentPersonasEnabledRef.current && composerPersonaControlRef.current !== undefined
          ? { personaControl: composerPersonaControlRef.current }
          : {}),
      })}
    </div>
  ),
}));

vi.mock("@/api/artifact", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/api/artifact")>();
  return {
    ...actual,
    artifactApi: {
      ...actual.artifactApi,
      getSessionPlan: (...args: unknown[]) => getSessionPlanMock(...args),
      getPlanComplexityAssessment: (...args: unknown[]) =>
        getPlanComplexityAssessmentMock(...args),
      approvePlanArtifact: (...args: unknown[]) =>
        approvePlanArtifactMock(...args),
    },
  };
});

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
      getAgentConversationRuntimeIndex: getAgentConversationRuntimeIndexMock,
      getAgentConversationRuntimeStatuses: getAgentConversationRuntimeStatusesMock,
      getAgentConversationWorkspace: getAgentConversationWorkspaceMock,
      getAgentWorkspacePrReviewContext: getAgentWorkspacePrReviewContextMock,
      sendAgentMessage: sendAgentMessageMock,
      switchAgentConversationMode: switchAgentConversationModeMock,
      activateAgentPlanDirectImplementation:
        activateAgentPlanDirectImplementationMock,
      activateAgentTaskPipeline: activateAgentTaskPipelineMock,
    },
  };
});

vi.mock("@/api/agent-tasks", () => ({
  agentTaskApi: {
    listAgentTasks: (...args: unknown[]) => listAgentTasksMock(...args),
    listAgentTaskLists: (...args: unknown[]) => listAgentTaskListsMock(...args),
    listAgentTasksForList: (...args: unknown[]) =>
      listAgentTaskListTasksMock(...args),
    listConversationTasks: (...args: unknown[]) => listAgentTasksMock(...args),
    listConversationTaskLists: (...args: unknown[]) =>
      listAgentTaskListsMock(...args),
    listConversationTaskListTasks: (...args: unknown[]) =>
      listAgentTaskListTasksMock(...args),
  },
}));

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

vi.mock("@/hooks/useAutomations", () => ({
  invalidateAutomationQueries: (...args: unknown[]) =>
    invalidateAutomationQueriesMock(...args),
  useAutomationDetail: (...args: unknown[]) => useAutomationDetailMock(...args),
}));

vi.mock("@/api/automations", async () => {
  const actual = await vi.importActual<typeof import("@/api/automations")>(
    "@/api/automations",
  );
  return {
    ...actual,
    automationsApi: {
      ...actual.automationsApi,
      finalize: (...args: unknown[]) => finalizeAutomationMock(...args),
      triggerRunNow: (...args: unknown[]) => triggerAutomationRunNowMock(...args),
    },
  };
});

vi.mock("@/stores/chatStore", () => {
  type ChatStoreMockState = {
    activeConversationIds: Record<string, string | null>;
    activeAgentRunIds: Record<string, string>;
    activeAgentRunMeta: Record<string, { launchRole: string | null; agentName: string | null }>;
    agentStatus: Record<string, string>;
    agentActivityLabels: Record<string, string>;
    isSending: Record<string, boolean>;
    setActiveConversation: (
      contextKey: string,
      conversationId: string | null,
    ) => void;
    setAgentRunning: (contextKey: string, isRunning: boolean) => void;
    setAgentStatus: (contextKey: string, status: string) => void;
    setAgentActivityLabel: (contextKey: string, label: string | null) => void;
  };
  const chatState: ChatStoreMockState = {
    activeConversationIds: {},
    activeAgentRunIds: {},
    activeAgentRunMeta: {},
    agentStatus: {},
    agentActivityLabels: {},
    isSending: {},
    setActiveConversation: vi.fn((contextKey, conversationId) => {
      if (conversationId == null) {
        delete chatState.activeConversationIds[contextKey];
        return;
      }
      chatState.activeConversationIds[contextKey] = conversationId;
    }),
    setAgentRunning: vi.fn((contextKey, isRunning) => {
      if (isRunning) {
        chatState.agentStatus[contextKey] = "generating";
        return;
      }
      delete chatState.agentStatus[contextKey];
      delete chatState.activeAgentRunIds[contextKey];
      delete chatState.agentActivityLabels[contextKey];
    }),
    setAgentStatus: vi.fn((contextKey, status) => {
      chatState.agentStatus[contextKey] = status;
    }),
    setAgentActivityLabel: vi.fn((contextKey, label) => {
      if (label == null) {
        delete chatState.agentActivityLabels[contextKey];
        return;
      }
      chatState.agentActivityLabels[contextKey] = label;
    }),
  };
  const useChatStore = Object.assign(
    (selector?: (state: ChatStoreMockState) => unknown) =>
      selector ? selector(chatState) : [],
    {
      getState: () => chatState,
      setState: (partial: Partial<ChatStoreMockState>) => {
        Object.assign(chatState, partial);
      },
    },
  );

  return {
    selectActiveAgentRunMeta: (storeKey: string) =>
      (state: ChatStoreMockState) => state.activeAgentRunMeta[storeKey],
    selectQueuedMessages: () => () => [],
    useChatStore,
  };
});

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
    capability,
    showHelperText,
    isReadOnly,
    sendDisabledReason,
    onSend,
    onForkSession,
    dataTestId,
    personaControl,
    runtimeDefault,
    runtimeTag,
    speed,
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
      secondaryOptionIds?: string[];
      options: Array<{
        id: string;
        label: string;
        disabled?: boolean;
        disabledReason?: string;
      }>;
    };
    capability?: {
      value: string;
      disabled?: boolean;
      pending?: boolean;
      testId?: string;
      onValueChange: (value: string) => void | Promise<unknown>;
      options: Array<{
        id: string;
        label: string;
        disabled?: boolean;
      }>;
    };
    showHelperText?: boolean;
    isReadOnly?: boolean;
    sendDisabledReason?: string | null;
    onSend: (message: string) => Promise<void> | void;
    onForkSession?: () => Promise<unknown> | void;
    dataTestId?: string;
    personaControl?: ReactNode;
    runtimeDefault?: {
      source?: string | null;
      scopeLabel?: string;
      isResetting?: boolean;
      disabled?: boolean;
      onReset: () => Promise<unknown> | void;
    };
    runtimeTag?: string;
    speed?: ComposerRuntimeSpeedField;
  }) => (
    <div data-testid={dataTestId}>
      <div data-testid="workspace-provider-value">{provider.value}</div>
      <div data-testid="workspace-model-value">{model.value}</div>
      <div data-testid="workspace-effort-value">{effort.value}</div>
      <div data-testid="workspace-runtime-tag">{runtimeTag ?? ""}</div>
      {speed ? (
        <div data-testid={speed.testId}>{speed.value}</div>
      ) : null}
      <div data-testid="workspace-helper-enabled">{String(showHelperText !== false)}</div>
      <div data-testid="workspace-composer-readonly">{String(Boolean(isReadOnly))}</div>
      <div data-testid="workspace-composer-disabled-reason">
        {sendDisabledReason ?? ""}
      </div>
      {mode && (
        <div>
          <button
            type="button"
            data-testid="agent-composer-mode-chip"
            disabled={mode.disabled}
            onClick={() => mode.onOpen?.()}
          >
            {mode.options.find((option) => option.id === mode.value)?.label ?? "—"}
          </button>
          {mode.options.filter(
            (option) =>
              !mode.secondaryOptionIds?.includes(option.id) ||
              option.id === mode.value,
          ).map((option) => {
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
          {mode.secondaryOptionIds?.length ? (
            <button type="button">Show more modes</button>
          ) : null}
        </div>
      )}
      {capability && (
        <div>
          <button
            type="button"
            data-testid={capability.testId ?? "agent-composer-capability"}
            disabled={capability.disabled || capability.pending}
          >
            {capability.value}
          </button>
          {capability.options.map((option) => (
            <button
              key={option.id}
              type="button"
              data-testid={`${capability.testId ?? "agent-composer-capability"}-${option.id}`}
              disabled={option.disabled}
              onClick={() => void capability.onValueChange(option.id)}
            >
              {option.label}
            </button>
          ))}
        </div>
      )}
      {personaControl}
      {runtimeDefault ? (
        <>
          <div data-testid="workspace-advanced-popover-header">
            Advanced · {runtimeDefault.scopeLabel ?? runtimeDefault.source ?? ""}
          </div>
          <button
            type="button"
            data-testid="agent-composer-runtime-reset"
            disabled={runtimeDefault.disabled || runtimeDefault.isResetting}
            onClick={() => void runtimeDefault.onReset()}
          >
            Reset runtime {runtimeDefault.scopeLabel ?? runtimeDefault.source ?? ""}
          </button>
        </>
      ) : null}
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
    onBackToWorkspaceChat,
    workspaceControl,
  }: {
    onBackToWorkspaceChat?: () => void;
    workspaceControl?: ReactNode;
  }) => (
    <div data-testid="mock-agents-chat-header">
      {onBackToWorkspaceChat ? (
        <button type="button" onClick={onBackToWorkspaceChat}>
          Back to Workspace Chat
        </button>
      ) : null}
      {workspaceControl}
    </div>
  ),
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
    coordinationMode: "solo",
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

function prReviewContext(): AgentWorkspacePrReviewContext {
  const now = "2026-07-20T12:00:00.000Z";
  return {
    success: true,
    workspace: {
      ...workspace(),
      mode: "review_pr",
      publicationPrNumber: 411,
      publicationPrUrl: "https://github.com/example/repo/pull/411",
    },
    events: [],
    prNumber: 411,
    prUrl: "https://github.com/example/repo/pull/411",
    currentHeadSha: "reviewed-head-a",
    pendingActionHeadStatus: "current",
    health: null,
    reviewFeedback: null,
    monitor: {
      conversationId: "conversation-1",
      projectId: "project-1",
      prNumber: 411,
      status: "awaiting_user",
      monitorEnabled: true,
      autoApproveEnabled: false,
      firstReviewCompleted: true,
      firstActionResolved: false,
      lastSeenHeadSha: "reviewed-head-a",
      lastReviewedHeadSha: "reviewed-head-a",
      lastReviewRunId: "run-1",
      lastReviewOutcome: "request_changes",
      lastSubmittedReviewId: null,
      reviewArtifactId: "artifact-1",
      reviewArtifactHeadSha: "reviewed-head-a",
      reviewArtifactVersion: 1,
      reviewArtifactUpdatedAt: now,
      lastError: null,
      createdAt: now,
      updatedAt: now,
    },
    pendingAction: {
      id: "reloaded-action",
      conversationId: "conversation-1",
      prNumber: 411,
      headSha: "reviewed-head-a",
      proposedAction: "request_changes",
      summary: "Reloaded durable reviewer proposal",
      reviewBody: "Please address the regression.",
      findingsJson: null,
      status: "pending",
      submittedReviewId: null,
      createdByRunId: "run-1",
      createdAt: now,
      updatedAt: now,
      resolvedAt: null,
    },
    recentActions: [],
    issueCommentEvidence: [],
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

function runtimeIndexWorkspaceRow(
  overrides: Partial<AgentConversationRuntimeIndexRow> = {},
): AgentConversationRuntimeIndexRow {
  return {
    id: "workspace:conversation-1",
    group: "main",
    kind: "workspace",
    lifecycle: "running",
    statusLabel: "Running",
    title: "Workspace chat",
    mode: "agent",
    orderIndex: 0,
    orderStartedAt: "2026-05-16T00:00:00.000Z",
    completedAt: null,
    conversationId: "conversation-1",
    contextType: "project",
    contextId: "conversation-1",
    taskId: null,
    agentRunId: "run-1",
    parentSessionId: null,
    childSessionId: null,
    providerHarness: "codex",
    providerSessionId: "session-1",
    errorMessage: null,
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
    planContractVersion: 2,
    blueprint: {
      id: "blueprint-1",
      type: "specification",
      name: "Implementation Blueprint",
      content: { type: "inline", text: "# Blueprint" },
      metadata: {
        createdAt: "2026-05-23T05:00:00Z",
        createdBy: "ralphx-ideation",
        version: 1,
      },
      derivedFrom: [],
      bucketId: "prd-library",
    },
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
    hasAttachedPlanArtifact: false,
    hasAutoOpenArtifacts: false,
    focusedWorkspaceReviewServiceTier: null,
    normalizedActiveRuntime: {
      provider: "claude",
      modelId: "opus",
      effort: "xhigh",
    },
    onActiveConversationModeChange: vi.fn(),
    onActiveConversationModeMenuOpen: vi.fn(),
    onActiveCapabilityChange: vi.fn(),
    onActiveEffortChange: vi.fn(),
    onActiveModelChange: vi.fn(),
    onActiveProviderChange: vi.fn(),
    onAgentUserMessageSent: vi.fn(),
    onConversationModeSwitched: vi.fn(),
    onFocusIdeationSession: vi.fn(),
    onFocusIdeationSessionForConversation: vi.fn(),
    onFocusWorkspaceReview: vi.fn(),
    onFocusVerificationSession: vi.fn(),
    onFocusTaskRuntime: vi.fn(),
    onFocusAutomationRun: vi.fn(),
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
    onStartPersonaBuilder: vi.fn(),
    publishShortcutLabel: "P",
    publishAttemptsByConversationId: {},
    selectedConversationId: "conversation-1",
    selectedTaskArtifactId: null,
    setTerminalChatDockElement: vi.fn(),
    switchingConversationModeId: null,
    terminalArchivedReason: null,
    terminalUnavailableReason: null,
    ...overrides,
  };
  const view = render(
    <QueryClientProvider client={queryClient}>
      <TooltipProvider delayDuration={0}>
        <AgentsActiveConversationPanel {...props} />
      </TooltipProvider>
    </QueryClientProvider>,
  );
  return {
    props,
    queryClient,
    rerenderPanel: (
      nextOverrides: Partial<
        ComponentProps<typeof AgentsActiveConversationPanel>
      > = {},
    ) => {
      view.rerender(
        <QueryClientProvider client={queryClient}>
          <TooltipProvider delayDuration={0}>
            <AgentsActiveConversationPanel {...props} {...nextOverrides} />
          </TooltipProvider>
        </QueryClientProvider>,
      );
    },
  };
}

function setPlanArtifactVisible(conversationId = "conversation-1") {
  useAgentArtifactUiStore.getState().setArtifactState(conversationId, {
    isOpen: true,
    activeTab: "plan",
    taskMode: "graph",
  });
}

describe("AgentsActiveConversationPanel", () => {
  afterEach(() => {
    vi.useRealTimers();
  });

  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(invoke).mockResolvedValue(undefined);
    eventSubscribers.clear();
    useChatStore.setState({ activeConversationIds: {} });
    useChatStore.setState({ activeAgentRunMeta: {} });
    useAgentSessionStore.setState({ roleRuntimeOverridesByConversationId: {} });
    useAgentSessionStore.setState({ artifactByConversationId: {} });
    useAgentArtifactUiStore.setState({ artifactByConversationId: {} });
    composerQuestionModeRef.current = undefined;
    composerAgentStatusRef.current = "idle";
    composerPersonaControlRef.current = undefined;
    agentPersonasEnabledRef.current = false;
    tasksEnabledRef.current = true;
    confirmImplementDirectlyMock.mockImplementation(
      (onConfirm: (runtime: typeof approvedPlanRuntime) => Promise<unknown>) =>
        void onConfirm(approvedPlanRuntime).catch(() => undefined),
    );
    confirmCreateProposalsMock.mockImplementation(
      (onConfirm: (runtime: typeof approvedPlanRuntime) => Promise<unknown>) =>
        void onConfirm(approvedPlanRuntime).catch(() => undefined),
    );
    personaQueryMock.mockReturnValue({
      data: [
        {
          id: "persona-1",
          slug: "design-voice",
          name: "Design Voice",
          status: "active",
        },
      ],
      isLoading: false,
      isError: false,
    });
    switchPersonaMock.mockResolvedValue(undefined);
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
    activateAgentPlanDirectImplementationMock.mockResolvedValue({
      workspace: {
        ...workspace(),
        mode: "edit",
        linkedIdeationSessionId: "planning-session-1",
      },
      artifactReferences: [
        {
          artifactId: "artifact-1",
          kind: "plan",
          title: "Plan Overview",
          sessionId: "planning-session-1",
          version: 1,
          status: "approved",
        },
        {
          artifactId: "blueprint-1",
          kind: "plan_blueprint",
          title: "Implementation Blueprint",
          sessionId: "planning-session-1",
          version: 2,
          status: "approved",
        },
      ],
      planContextFingerprint: "plan-context-fingerprint-1",
    });
    getAgentConversationWorkspaceMock.mockResolvedValue({
      ...workspace(),
      mode: "edit",
    });
    activateAgentTaskPipelineMock.mockResolvedValue({
      ...workspace(),
      mode: "tasks",
      linkedIdeationSessionId: "planning-session-1",
      taskPipelineSessionId: "planning-session-1",
      taskPipelineAvailable: true,
    });
    getAgentConversationRuntimeIndexMock.mockResolvedValue({
      conversationId: "conversation-1",
      rows: [runtimeIndexWorkspaceRow()],
    });
    getAgentConversationRuntimeStatusesMock.mockResolvedValue({});
    getAgentWorkspacePrReviewContextMock.mockResolvedValue(prReviewContext());
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
    listAgentTasksMock.mockResolvedValue([]);
    listAgentTaskListsMock.mockResolvedValue([]);
    listAgentTaskListTasksMock.mockResolvedValue([]);
    useAutomationDetailMock.mockReturnValue({
      data: null,
      isLoading: false,
      isError: false,
    });
    finalizeAutomationMock.mockResolvedValue({ id: "automation-1", status: "active" });
    triggerAutomationRunNowMock.mockResolvedValue({ scheduled: true, reason: null });
  });

  it("keeps a disabled-feature historical Tasks mode labeled and first", () => {
    tasksEnabledRef.current = false;

    renderPanel({
      activeConversation: { ...projectConversation(), agentMode: "tasks" },
      activeConversationMode: "tasks",
      activeWorkspace: {
        ...workspace(),
        mode: "tasks",
        taskPipelineSessionId: "planning-session-1",
        taskPipelineAvailable: true,
      },
    });

    expect(screen.getByTestId("agent-composer-mode-chip")).toHaveTextContent(
      "Tasks",
    );
    expect(screen.getAllByTestId(/^agent-mode-option-/)[0]).toHaveTextContent(
      "Tasks",
    );
  });

  it("keeps a legacy Ideation mode labeled and first while current", () => {
    renderPanel({
      activeConversation: { ...projectConversation(), agentMode: "ideation" },
      activeConversationMode: "ideation",
      activeWorkspace: { ...workspace(), mode: "ideation" },
    });

    expect(screen.getByTestId("agent-composer-mode-chip")).toHaveTextContent(
      "Ideation",
    );
    expect(screen.getAllByTestId(/^agent-mode-option-/)[0]).toHaveTextContent(
      "Ideation",
    );
  });

  it("uses the new-conversation secondary mode disclosure", () => {
    renderPanel({
      activeConversation: { ...projectConversation(), agentMode: "edit" },
      activeConversationMode: "edit",
      activeWorkspace: { ...workspace(), mode: "edit" },
    });

    expect(screen.queryByTestId("agent-mode-option-automation"))
      .not.toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: "Show more modes" }),
    ).toBeInTheDocument();
  });

  it("reloads a durable Review PR proposal after navigating away and evicting its query", async () => {
    const reviewConversation = {
      ...projectConversation(),
      agentMode: "review_pr" as const,
    };
    const reviewWorkspace = prReviewContext().workspace;
    const { queryClient, rerenderPanel } = renderPanel({
      activeConversation: reviewConversation,
      activeConversationMode: "review_pr",
      activeWorkspace: reviewWorkspace,
    });

    expect(
      await screen.findByText("Reloaded durable reviewer proposal"),
    ).toBeInTheDocument();
    expect(getAgentWorkspacePrReviewContextMock).toHaveBeenCalledTimes(1);

    rerenderPanel({
      activeConversation: {
        ...projectConversation(),
        id: "conversation-2",
        agentMode: "edit",
      },
      activeConversationMode: "edit",
      activeWorkspace: {
        ...workspace(),
        conversationId: "conversation-2",
        mode: "edit",
      },
      selectedConversationId: "conversation-2",
    });
    queryClient.removeQueries({
      queryKey: agentWorkspaceKeys.prReview("conversation-1"),
    });
    rerenderPanel({
      activeConversation: reviewConversation,
      activeConversationMode: "review_pr",
      activeWorkspace: reviewWorkspace,
      selectedConversationId: "conversation-1",
    });

    expect(
      await screen.findByText("Reloaded durable reviewer proposal"),
    ).toBeInTheDocument();
    expect(getAgentWorkspacePrReviewContextMock).toHaveBeenCalledTimes(2);
    expect(
      screen.getByRole("button", { name: /Request Changes/i }),
    ).toBeEnabled();
  });

  it("renders the bound persona control supplied by the Chat surface in the Agents composer", () => {
    agentPersonasEnabledRef.current = true;
    composerPersonaControlRef.current = (
      <PersonaChip
        conversationId="conversation-1"
        personaId="persona-1"
        isAgentRunning={false}
        lastRunPersonaId="persona-1"
        lastRunPersonaSlug="design-voice"
        lastRunPersonaInjected
      />
    );

    renderPanel();

    const composer = screen.getByTestId("agents-conversation-composer");
    expect(
      within(composer).getByRole("button", {
        name: "Switch conversation persona",
      }),
    ).toHaveTextContent("design-voice");
  });

  it("routes the active project Persona Builder action through the Chat surface", () => {
    const onStartPersonaBuilder = vi.fn();
    renderPanel({ onStartPersonaBuilder });

    fireEvent.click(
      screen.getByRole("button", { name: "Create persona for this project" }),
    );

    expect(onStartPersonaBuilder).toHaveBeenCalledOnce();
  });

  it("renders the mapped not-applied persona affordance in the Agents composer", async () => {
    agentPersonasEnabledRef.current = true;
    composerPersonaControlRef.current = (
      <PersonaChip
        conversationId="conversation-1"
        personaId="persona-1"
        isAgentRunning={false}
        lastRunPersonaId="persona-1"
        lastRunPersonaSlug="design-voice"
        lastRunPersonaInjected={false}
        lastRunPersonaSkippedReason="native_agent_flag"
      />
    );

    renderPanel();

    const composer = screen.getByTestId("agents-conversation-composer");
    const trigger = within(composer).getByRole("button", {
      name: "Switch conversation persona",
    });
    expect(trigger).toHaveTextContent("design-voice not applied");

    fireEvent.pointerMove(trigger);
    expect(
      await screen.findByRole("tooltip", {
        name: "Native agent mode does not support personas",
      }),
    ).toBeInTheDocument();
  });

  it("renders the no-persona control supplied by the Chat surface in the Agents composer", () => {
    agentPersonasEnabledRef.current = true;
    composerPersonaControlRef.current = (
      <PersonaChip
        conversationId="conversation-1"
        personaId={null}
        isAgentRunning={false}
      />
    );

    renderPanel();

    const composer = screen.getByTestId("agents-conversation-composer");
    expect(
      within(composer).getByRole("button", {
        name: "Switch conversation persona",
      }),
    ).toHaveTextContent("No persona");
  });

  it("renders no persona element when the Chat surface feature flag gate is off", () => {
    composerPersonaControlRef.current = (
      <PersonaChip
        conversationId="conversation-1"
        personaId="persona-1"
        isAgentRunning={false}
      />
    );

    renderPanel();

    const composer = screen.getByTestId("agents-conversation-composer");
    expect(
      within(composer).queryByRole("button", {
        name: "Switch conversation persona",
      }),
    ).not.toBeInTheDocument();
    expect(screen.queryByTestId("persona-chip")).not.toBeInTheDocument();
  });

  it("keeps an unchanged persona control from rerendering during unrelated panel updates", () => {
    const CountingPersonaControl = vi.fn(({ slug }: { slug: string }) => (
      <span>{slug}</span>
    ));

    agentPersonasEnabledRef.current = true;
    composerPersonaControlRef.current = (
      <CountingPersonaControl slug="design-voice" />
    );
    const { rerenderPanel } = renderPanel();
    expect(CountingPersonaControl).toHaveBeenCalledOnce();

    composerPersonaControlRef.current = (
      <CountingPersonaControl slug="design-voice" />
    );
    rerenderPanel({
      publishAttemptsByConversationId: {
        "another-conversation": {
          conversationId: "another-conversation",
          startedAtMs: 1,
        },
      },
    });

    expect(CountingPersonaControl).toHaveBeenCalledTimes(2);
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

  it("scopes reviewer and fixer runtime controls to the active role and restores the conversation runtime", async () => {
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "get_manual_role_defaults") {
        return Promise.resolve({ project_id: "project-1", roles: [
          {
            role: "workspace_reviewer", display_name: "Reviewer", description: "Review", family: "workspace", family_display_name: "Workspace", requires_tasks: false,
            configured: null, effective: { provider: "codex", model: "gpt-5.6", effort: "high", service_tier: "fast", coordination_mode: "solo", persona_id: null, approval_policy: null, sandbox_mode: null }, source: "project", diagnostics: [], controls: { capabilities: [], speeds: [], persona: { enabled: true, disabled_reason: null } },
          },
          {
            role: "workspace_repair", display_name: "Fixer", description: "Fix", family: "workspace", family_display_name: "Workspace", requires_tasks: false,
            configured: null, effective: { provider: "claude", model: "sonnet", effort: "medium", service_tier: "provider_default", coordination_mode: "solo", persona_id: null, approval_policy: null, sandbox_mode: null }, source: "project", diagnostics: [], controls: { capabilities: [], speeds: [], persona: { enabled: true, disabled_reason: null } },
          },
          {
            role: "pr_fixer", display_name: "PR Fixer", description: "Fix PR", family: "workspace", family_display_name: "Workspace", requires_tasks: false,
            configured: null, effective: { provider: "codex", model: "gpt-5.6", effort: "medium", service_tier: "standard", coordination_mode: "solo", persona_id: null, approval_policy: null, sandbox_mode: null }, source: "project", diagnostics: [], controls: { capabilities: [], speeds: [], persona: { enabled: true, disabled_reason: null } },
          },
        ] });
      }
      return Promise.resolve(undefined);
    });
    useAgentSessionStore.getState().setRuntimeForConversation(
      "conversation-1",
      "project-1",
      { provider: "claude", modelId: "opus", effort: "xhigh" },
    );
    const { rerenderPanel } = renderPanel({
      chatFocus: {
        type: "workspace_review",
        conversationId: "review-conversation-1",
      },
    });

    expect(await screen.findByTestId("agents-role-runtime-banner")).toHaveTextContent("Reviewer run active");
    expect(screen.getByTestId("workspace-runtime-tag")).toHaveTextContent("REV");
    expect(screen.getByTestId("workspace-advanced-popover-header")).toHaveTextContent("Advanced · Reviewer runtime");
    await waitFor(() => expect(screen.getByTestId("workspace-provider-value")).toHaveTextContent("codex"));
    fireEvent.click(screen.getByTestId("change-workspace-model"));
    expect(useAgentSessionStore.getState().roleRuntimeOverridesByConversationId["conversation-1"]?.workspace_reviewer?.model).toBe("sonnet");
    expect(useAgentSessionStore.getState().runtimeByConversationId["conversation-1"]).toEqual({
      provider: "claude",
      modelId: "opus",
      effort: "xhigh",
    });
    fireEvent.click(screen.getByTestId("agent-composer-runtime-reset"));
    expect(useAgentSessionStore.getState().roleRuntimeOverridesByConversationId["conversation-1"]?.workspace_reviewer).toBeUndefined();
    expect(screen.getByTestId("workspace-provider-value")).toHaveTextContent("codex");
    expect(screen.getByTestId("workspace-model-value")).toHaveTextContent("gpt-5.6");
    expect(useAgentSessionStore.getState().runtimeByConversationId["conversation-1"]).toEqual({
      provider: "claude",
      modelId: "opus",
      effort: "xhigh",
    });

    rerenderPanel({
      chatFocus: {
        type: "workspace_repair",
        conversationId: "repair-conversation-1",
      },
    });
    expect(await screen.findByTestId("agents-role-runtime-banner")).toHaveTextContent("Fixer run active");
    expect(screen.getByTestId("workspace-runtime-tag")).toHaveTextContent("FIX");
    await waitFor(() => expect(screen.getByTestId("workspace-provider-value")).toHaveTextContent("claude"));
    expect(screen.getByTestId("workspace-model-value")).toHaveTextContent("sonnet");
    fireEvent.click(screen.getByTestId("change-workspace-model"));
    expect(useAgentSessionStore.getState().roleRuntimeOverridesByConversationId["conversation-1"]?.workspace_repair?.model).toBe("sonnet");

    rerenderPanel({
      chatFocus: { type: "pr_fixer", conversationId: "pr-fixer-conversation-1" },
    });
    expect(await screen.findByTestId("agents-role-runtime-banner")).toHaveTextContent("PR Fixer run active");
    fireEvent.click(screen.getByTestId("change-workspace-model"));
    expect(useAgentSessionStore.getState().roleRuntimeOverridesByConversationId["conversation-1"]?.pr_fixer?.model).toBe("sonnet");

    rerenderPanel({ chatFocus: { type: "workspace" } });
    expect(screen.queryByTestId("agents-role-runtime-banner")).not.toBeInTheDocument();
    expect(screen.getByTestId("workspace-provider-value")).toHaveTextContent("claude");
    expect(screen.getByTestId("workspace-model-value")).toHaveTextContent("opus");
  });

  it("keeps local runtime state unchanged when active role reset refetch fails", async () => {
    const roleDefault = {
      role: "workspace_edit",
      source: "project_ui",
      value: {
        provider: "codex",
        model: "gpt-5.5",
        effort: "xhigh",
        service_tier: "fast",
        coordination_mode: "solo",
        persona_id: null,
        approval_policy: "never",
        sandbox_mode: "danger-full-access",
      },
    };
    let conversationDefaultCalls = 0;
    vi.mocked(invoke).mockImplementation((command) => {
      if (command === "get_agent_conversation_role_default") {
        conversationDefaultCalls += 1;
        return conversationDefaultCalls === 1
          ? Promise.resolve(roleDefault)
          : Promise.reject(new Error("role default refetch failed"));
      }
      if (command === "reset_agent_conversation_role_default") {
        return Promise.resolve(roleDefault);
      }
      return Promise.resolve(undefined);
    });
    useAgentSessionStore.getState().setRuntimeForConversation(
      "conversation-1",
      "project-1",
      {
        provider: "claude",
        modelId: "opus",
        effort: "xhigh",
      },
    );

    renderPanel();

    await waitFor(() => expect(conversationDefaultCalls).toBe(1));
    fireEvent.click(screen.getByTestId("agent-composer-runtime-reset"));

    await waitFor(() =>
      expect(toastErrorMock).toHaveBeenCalledWith("role default refetch failed"),
    );
    expect(useAgentSessionStore.getState().runtimeByConversationId["conversation-1"]).toEqual({
      provider: "claude",
      modelId: "opus",
      effort: "xhigh",
    });
    expect(screen.getByTestId("integrated-chat-panel")).toHaveAttribute(
      "data-send-codex-fast-mode",
      "null",
    );
  });

  it("locks automation-owned run conversations while the run is non-terminal", () => {
    useAutomationDetailMock.mockReturnValue({
      data: {
        runs: [{ id: "run-1", status: "published" }],
      },
      isLoading: false,
      isError: false,
    });

    renderPanel({
      activeConversation: {
        ...projectConversation(),
        agentMode: "automation",
        automationId: "automation-1",
        automationRunId: "run-1",
      },
    });

    expect(screen.getByTestId("agents-automation-run-readonly-banner")).toHaveTextContent(
      "Automation run conversations are read-only while the automation is working on this run.",
    );
    expect(screen.getByTestId("workspace-composer-readonly")).toHaveTextContent("true");
    expect(screen.getByTestId("workspace-composer-disabled-reason")).toHaveTextContent(
      "Automation run conversations are read-only while the automation is working on this run.",
    );
  });

  it("allows chat feedback while an automation run awaits plan approval", () => {
    useAutomationDetailMock.mockReturnValue({
      data: {
        runs: [{ id: "run-1", status: "awaiting_plan_approval" }],
      },
      isLoading: false,
      isError: false,
    });

    renderPanel({
      activeConversation: {
        ...projectConversation(),
        agentMode: "automation",
        automationId: "automation-1",
        automationRunId: "run-1",
      },
    });

    expect(
      screen.queryByTestId("agents-automation-run-readonly-banner"),
    ).not.toBeInTheDocument();
    expect(screen.getByTestId("workspace-composer-readonly")).toHaveTextContent(
      "false",
    );
    expect(screen.getByTestId("workspace-composer-disabled-reason")).toHaveTextContent(
      "",
    );
  });

  it("keeps completed automation run conversations read-only until judging settles", () => {
    useAutomationDetailMock.mockReturnValue({
      data: {
        runs: [{ id: "run-1", status: "completed", judgeState: "none" }],
      },
      isLoading: false,
      isError: false,
    });

    renderPanel({
      activeConversation: {
        ...projectConversation(),
        agentMode: "automation",
        automationId: "automation-1",
        automationRunId: "run-1",
      },
    });

    expect(screen.getByTestId("agents-automation-run-readonly-banner")).toBeInTheDocument();
    expect(screen.getByTestId("workspace-composer-readonly")).toHaveTextContent("true");
  });

  it("routes setup automation runs through the Runtime tray with plan focus seeding", async () => {
    const onFocusAutomationRun = vi.fn();
    const onSelectArtifact = vi.fn();
    useAutomationDetailMock.mockReturnValue({
      data: {
        automation: {
          id: "automation-1",
          name: "Nightly release",
          status: "active",
          planApprovalMode: "manual",
        },
        runs: [
          {
            id: "run-1",
            automationId: "automation-1",
            runIndex: 1,
            status: "awaiting_plan_approval",
            judgeState: "none",
            planPhase: true,
            planArtifactId: "plan-artifact-1",
            prNumber: null,
            prUrl: null,
            conversationId: "conversation-run-1",
          },
        ],
      },
      isLoading: false,
      isError: false,
    });

    renderPanel({
      onFocusAutomationRun,
      onSelectArtifact,
      activeConversation: {
        ...projectConversation(),
        agentMode: "automation",
        automationId: "automation-1",
        automationRunId: null,
      },
    });

    expect(
      screen.queryByTestId("agents-automation-runs-widget"),
    ).not.toBeInTheDocument();
    fireEvent.click(await screen.findByTestId("agents-composer-runtimes-toggle"));
    const runs = await screen.findByTestId("agents-composer-runtimes-group-runs");
    expect(within(runs).getByText("Awaiting plan approval")).toBeInTheDocument();

    fireEvent.click(
      within(runs).getByTestId("agents-composer-automation-run-run-1"),
    );

    expect(onSelectArtifact).toHaveBeenCalledWith("plan");
    expect(onFocusAutomationRun).toHaveBeenCalledWith(
      "automation-1",
      "run-1",
      "conversation-run-1",
      {
        runStatus: "awaiting_plan_approval",
        judgeState: "none",
        workspaceMode: "plan",
        hasPlanArtifact: true,
        hasPullRequest: false,
      },
    );
  });

  it.each(["running", "provisioning"] as const)(
    "keeps automation run conversations read-only while %s",
    (status) => {
      useAutomationDetailMock.mockReturnValue({
        data: {
          runs: [{ id: "run-1", status }],
        },
        isLoading: false,
        isError: false,
      });

      renderPanel({
        activeConversation: {
          ...projectConversation(),
          agentMode: "automation",
          automationId: "automation-1",
          automationRunId: "run-1",
        },
      });

      expect(screen.getByTestId("agents-automation-run-readonly-banner")).toHaveTextContent(
        "Automation run conversations are read-only",
      );
      expect(screen.getByTestId("workspace-composer-readonly")).toHaveTextContent(
        "true",
      );
    },
  );

  it("disables the composer mode picker for automation-run conversations", async () => {
    useAutomationDetailMock.mockReturnValue({
      data: {
        runs: [{ id: "run-1", status: "awaiting_plan_approval" }],
      },
      isLoading: false,
      isError: false,
    });

    renderPanel({
      activeConversation: {
        ...projectConversation(),
        agentMode: "automation",
        automationId: "automation-1",
        automationRunId: "run-1",
      },
      activeWorkspace: {
        ...workspace(),
        mode: "plan",
        modeSwitchLocked: false,
      },
    });

    expect(screen.getByTestId("agent-composer-mode-chip")).toBeDisabled();
  });

  it("keeps automation-owned run conversations editable after terminal run status", () => {
    useAutomationDetailMock.mockReturnValue({
      data: {
        runs: [{ id: "run-1", status: "merged" }],
      },
      isLoading: false,
      isError: false,
    });

    renderPanel({
      activeConversation: {
        ...projectConversation(),
        agentMode: "automation",
        automationId: "automation-1",
        automationRunId: "run-1",
      },
    });

    expect(screen.queryByTestId("agents-automation-run-readonly-banner")).not.toBeInTheDocument();
    expect(screen.getByTestId("workspace-composer-readonly")).toHaveTextContent("false");
  });

  it("keeps setup conversations editable without showing setup controls above the composer", () => {
    const onOpenAutomation = vi.fn();
    renderPanel({
      onOpenAutomation,
      activeConversation: {
        ...projectConversation(),
        agentMode: "automation",
        automationId: "automation-7",
        automationRunId: null,
      },
    });

    expect(
      screen.queryByTestId("agents-automation-setup-banner"),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByTestId("agents-automation-runtime-selector"),
    ).not.toBeInTheDocument();
    // Setup conversations stay editable — the user configures by chatting.
    expect(screen.getByTestId("workspace-composer-readonly")).toHaveTextContent(
      "false",
    );
    // Setup and run banners are mutually exclusive.
    expect(
      screen.queryByTestId("agents-automation-run-readonly-banner"),
    ).not.toBeInTheDocument();
    expect(onOpenAutomation).not.toHaveBeenCalled();
  });

  it("hides automation approval CTA until the draft has a complete spec", () => {
    useAutomationDetailMock.mockReturnValue({
      data: {
        automation: {
          id: "automation-7",
          status: "draft",
          firstRunPrompt: null,
          goalPrompt: "",
          providerHarness: "codex",
          modelId: "gpt-5.5",
          runMode: "edit",
          baseRefKind: "project_default",
          baseRef: "",
          completionSignal: "pr_merged",
          goalItemsJson: null,
        },
        runs: [],
      },
      isLoading: false,
      isError: false,
    });

    renderPanel({
      activeConversation: {
        ...projectConversation(),
        agentMode: "automation",
        automationId: "automation-7",
        automationRunId: null,
      },
    });

    expect(
      screen.queryByTestId("agents-automation-composer-cta-row"),
    ).not.toBeInTheDocument();
  });

  it("shows an automation Approve CTA for complete draft setup conversations", async () => {
    useAutomationDetailMock.mockReturnValue({
      data: {
        automation: {
          id: "automation-7",
          status: "draft",
          firstRunPrompt: "Update dependencies",
          goalPrompt: "Keep dependencies current",
          providerHarness: "codex",
          modelId: "gpt-5.5",
          runMode: "edit",
          baseRefKind: "project_default",
          baseRef: "",
          completionSignal: "pr_merged",
          goalItemsJson:
            '[{"id":"phase-1","title":"Update dependencies","status":"pending"}]',
        },
        runs: [],
      },
      isLoading: false,
      isError: false,
    });

    renderPanel({
      activeConversation: {
        ...projectConversation(),
        agentMode: "automation",
        automationId: "automation-7",
        automationRunId: null,
      },
    });

    expect(screen.getByTestId("agents-automation-composer-cta-row")).toBeInTheDocument();
    expect(screen.getByTestId("agents-automation-composer-cta-hint")).toHaveTextContent(
      "Ready for approval",
    );
    expect(screen.getByTestId("agents-automation-composer-cta-copy")).not.toHaveTextContent(
      "Recommended",
    );
    expect(screen.getByTestId("agents-automation-composer-cta-approve")).toHaveTextContent(
      "Approve",
    );

    fireEvent.click(screen.getByTestId("agents-automation-composer-cta-approve"));

    await waitFor(() => expect(finalizeAutomationMock).toHaveBeenCalledWith("automation-7"));
    expect(triggerAutomationRunNowMock).not.toHaveBeenCalled();
  });

  it("shows a separate automation Run CTA for approved setup conversations", async () => {
    useAutomationDetailMock.mockReturnValue({
      data: {
        automation: {
          id: "automation-7",
          status: "active",
          firstRunPrompt: "Update dependencies",
          goalPrompt: "Keep dependencies current",
          providerHarness: "codex",
          modelId: "gpt-5.5",
          runMode: "edit",
          baseRefKind: "project_default",
          baseRef: "",
          completionSignal: "pr_merged",
          goalItemsJson:
            '[{"id":"phase-1","title":"Update dependencies","status":"pending"}]',
        },
        runs: [],
      },
      isLoading: false,
      isError: false,
    });

    renderPanel({
      activeConversation: {
        ...projectConversation(),
        agentMode: "automation",
        automationId: "automation-7",
        automationRunId: null,
      },
    });

    expect(screen.getByTestId("agents-automation-composer-cta-hint")).toHaveTextContent(
      "Run available",
    );
    fireEvent.click(screen.getByTestId("agents-automation-composer-cta-run"));

    await waitFor(() =>
      expect(triggerAutomationRunNowMock).toHaveBeenCalledWith("automation-7"),
    );
    expect(toastSuccessMock).toHaveBeenCalledWith("Automation run queued");
    expect(finalizeAutomationMock).not.toHaveBeenCalled();
  });

  it("reports automation Run CTA backend refusal as info", async () => {
    triggerAutomationRunNowMock.mockResolvedValueOnce({
      scheduled: false,
      reason: "latest run is not ready",
    });
    useAutomationDetailMock.mockReturnValue({
      data: {
        automation: {
          id: "automation-7",
          status: "active",
          firstRunPrompt: "Update dependencies",
          goalPrompt: "Keep dependencies current",
          providerHarness: "codex",
          modelId: "gpt-5.5",
          runMode: "edit",
          baseRefKind: "project_default",
          baseRef: "",
          completionSignal: "pr_merged",
          goalItemsJson:
            '[{"id":"phase-1","title":"Update dependencies","status":"pending"}]',
        },
        runs: [],
      },
      isLoading: false,
      isError: false,
    });

    renderPanel({
      activeConversation: {
        ...projectConversation(),
        agentMode: "automation",
        automationId: "automation-7",
        automationRunId: null,
      },
    });

    fireEvent.click(screen.getByTestId("agents-automation-composer-cta-run"));

    await waitFor(() =>
      expect(triggerAutomationRunNowMock).toHaveBeenCalledWith("automation-7"),
    );
    expect(toastInfoMock).toHaveBeenCalledWith("latest run is not ready");
    expect(toastSuccessMock).not.toHaveBeenCalled();
  });

  it("refreshes the automation artifact after accepting a chat automation proposal", async () => {
    useAutomationDetailMock.mockReturnValue({
      data: {
        automation: {
          id: "automation-7",
          status: "draft",
          firstRunPrompt: null,
          goalPrompt: "",
          providerHarness: "codex",
          modelId: "gpt-5.5",
          runMode: "edit",
          baseRefKind: "project_default",
          baseRef: "",
          completionSignal: "pr_merged",
          goalItemsJson: null,
        },
        runs: [],
      },
      isLoading: false,
      isError: false,
    });

    renderPanel({
      activeConversation: {
        ...projectConversation(),
        agentMode: "automation",
        automationId: "automation-7",
        automationRunId: null,
      },
    });

    fireEvent.click(screen.getByTestId("accept-automation-setup-proposal"));

    await waitFor(() =>
      expect(invalidateAutomationQueriesMock).toHaveBeenCalledWith(
        expect.anything(),
        "automation-7",
      ),
    );
    expect(switchAgentConversationModeMock).not.toHaveBeenCalled();
  });

  it("does not show the setup banner for automation run conversations", () => {
    useAutomationDetailMock.mockReturnValue({
      data: {
        runs: [{ id: "run-1", status: "published" }],
      },
      isLoading: false,
      isError: false,
    });

    renderPanel({
      onOpenAutomation: vi.fn(),
      activeConversation: {
        ...projectConversation(),
        agentMode: "automation",
        automationId: "automation-1",
        automationRunId: "run-1",
      },
    });

    expect(
      screen.getByTestId("agents-automation-run-readonly-banner"),
    ).toBeInTheDocument();
    expect(
      screen.queryByTestId("agents-automation-setup-banner"),
    ).not.toBeInTheDocument();
  });

  it("shows no automation banners for a plain conversation", () => {
    renderPanel({
      onOpenAutomation: vi.fn(),
      activeConversation: { ...projectConversation(), agentMode: "chat" },
    });

    expect(
      screen.queryByTestId("agents-automation-setup-banner"),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByTestId("agents-automation-run-readonly-banner"),
    ).not.toBeInTheDocument();
  });

  it("uses workspace runtime controls while focused on the workspace Review chat", () => {
    const onActiveModelChange = vi.fn();
    const onActiveEffortChange = vi.fn();
    renderPanel({
      chatFocus: {
        type: "workspace_review",
        conversationId: "review-conversation-1",
      },
      onActiveEffortChange,
      onActiveModelChange,
    });

    expect(screen.getByTestId("workspace-provider-value").textContent).toBe("claude");
    expect(screen.getByTestId("workspace-model-value").textContent).toBe("opus");
    expect(screen.getByTestId("workspace-effort-value").textContent).toBe("high");

    fireEvent.click(screen.getByTestId("change-workspace-model"));
    fireEvent.click(screen.getByTestId("change-workspace-effort"));

    expect(onActiveModelChange).not.toHaveBeenCalled();
    expect(onActiveEffortChange).not.toHaveBeenCalled();
    expect(
      useAgentSessionStore.getState().roleRuntimeOverridesByConversationId[
        "conversation-1"
      ]?.workspace_reviewer,
    ).toMatchObject({ model: "sonnet", effort: "max" });
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
    ], null);
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

  it("shows a single workspace runtime inside the Runtimes tab", async () => {
    getAgentConversationRuntimeStatusesMock.mockResolvedValue({
      "conversation-1": workspaceRuntimeStatus(),
    });
    getAgentConversationRuntimeIndexMock.mockResolvedValue({
      conversationId: "conversation-1",
      rows: [runtimeIndexWorkspaceRow({ mode: "ideation" })],
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

    fireEvent.click(await screen.findByTestId("agents-composer-runtimes-toggle"));

    expect(
      await screen.findByTestId(
        "agents-composer-runtime-row-workspace",
        undefined,
        deferredHydrationTimeout,
      ),
    ).toHaveTextContent("Workspace chat");
    expect(screen.queryByTestId("agents-runtime-status-widget")).not.toBeInTheDocument();
  });

  it("keeps nonfocused task runtime rows out of workspace composer chrome", async () => {
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

    await waitFor(() =>
      expect(getAgentConversationRuntimeStatusesMock).toHaveBeenCalledWith([
        "conversation-1",
      ]),
    );

    expect(screen.queryByTestId("agents-runtime-status-widget")).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "View Task" })).not.toBeInTheDocument();
    expect(onFocusTaskRuntime).not.toHaveBeenCalled();
    expect(onOpenTaskArtifact).not.toHaveBeenCalled();
  });

  it("marks child-only workspace layout updates as not owned by the visible transcript", async () => {
    getAgentConversationRuntimeStatusesMock.mockResolvedValue({
      "conversation-1": workspaceRuntimeStatus({
        primarySource: "review",
        summaryLabel: "Runtime activity",
        items: [
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

    renderPanel();

    expect(screen.queryByTestId("agents-runtime-status-widget")).not.toBeInTheDocument();

  });

  it("shows focused task runtime rows in the Runtimes tab", async () => {
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
    getAgentConversationRuntimeIndexMock.mockResolvedValue({
      conversationId: "conversation-1",
      rows: [
        runtimeIndexWorkspaceRow({ lifecycle: "waiting", statusLabel: "Waiting" }),
        runtimeIndexWorkspaceRow({
          id: "task:task-2",
          group: "pipeline",
          kind: "task",
          lifecycle: "running",
          statusLabel: "Reviewing",
          title: "Review task",
          mode: null,
          orderIndex: 1,
          conversationId: "review-conversation-1",
          contextType: "review",
          contextId: "task-2",
          taskId: "task-2",
        }),
      ],
    });

    renderPanel({
      chatFocus: {
        type: "task_runtime",
        taskId: "task-2",
        contextType: "review",
      },
    });

    fireEvent.click(await screen.findByTestId("agents-composer-runtimes-toggle"));

    expect(
      await screen.findByTestId(
        "agents-composer-runtime-row-task",
        undefined,
        deferredHydrationTimeout,
      ),
    ).toHaveTextContent("Review task");
    expect(screen.getByText("Viewing")).toBeInTheDocument();
  });

  it("routes clicks from every Runtimes tab row kind", async () => {
    const onSelectChatFocus = vi.fn();
    const onFocusIdeationSession = vi.fn();
    const onFocusVerificationSession = vi.fn();
    const onFocusWorkspaceReview = vi.fn();
    const onFocusTaskRuntime = vi.fn();
    const onOpenTaskArtifact = vi.fn();
    getAgentConversationRuntimeStatusesMock.mockResolvedValue({
      "conversation-1": workspaceRuntimeStatus({
        isRunning: false,
        agentStatus: "idle",
        primarySource: null,
        summaryLabel: null,
        items: [],
      }),
    });
    getAgentConversationRuntimeIndexMock.mockResolvedValue({
      conversationId: "conversation-1",
      rows: [
        runtimeIndexWorkspaceRow({ lifecycle: "waiting", mode: "chat", statusLabel: "Waiting" }),
        runtimeIndexWorkspaceRow({
          id: "ideation:session-1",
          group: "ideation_verification",
          kind: "ideation",
          lifecycle: "completed",
          statusLabel: "Done",
          title: "Plan ideation",
          mode: "ideation",
          orderIndex: 1,
          conversationId: null,
          contextType: "ideation",
          contextId: "session-1",
          taskId: null,
          agentRunId: null,
        }),
        runtimeIndexWorkspaceRow({
          id: "verification:verification-child",
          group: "ideation_verification",
          kind: "verification",
          lifecycle: "failed",
          statusLabel: "Failed",
          title: "Verification",
          mode: "pr_review",
          orderIndex: 2,
          conversationId: null,
          contextType: "verification",
          contextId: "verification-child",
          taskId: null,
          agentRunId: null,
          parentSessionId: "session-parent",
          childSessionId: "verification-child",
        }),
        runtimeIndexWorkspaceRow({
          id: "workspace_review:review-conversation-1",
          group: "ideation_verification",
          kind: "workspace_review",
          lifecycle: "blocked",
          statusLabel: "Blocked",
          title: "Review workspace changes",
          mode: null,
          orderIndex: 3,
          conversationId: "review-conversation-1",
          contextType: "project",
          contextId: "review-conversation-1",
          taskId: null,
          agentRunId: null,
          providerHarness: "claude",
        }),
        runtimeIndexWorkspaceRow({
          id: "task:task-3",
          group: "pipeline",
          kind: "task",
          lifecycle: "running",
          statusLabel: "Merging",
          title: "Merge task",
          mode: "agent",
          orderIndex: 4,
          conversationId: "merge-conversation-1",
          contextType: "merge",
          contextId: "task-3",
          taskId: "task-3",
          agentRunId: null,
        }),
      ],
    });

    renderPanel({
      onSelectChatFocus,
      onFocusIdeationSession,
      onFocusVerificationSession,
      onFocusWorkspaceReview,
      onFocusTaskRuntime,
      onOpenTaskArtifact,
    });

    fireEvent.click(await screen.findByTestId("agents-composer-runtimes-toggle"));
    const workspaceRow = await screen.findByTestId(
      "agents-composer-runtime-row-workspace",
      undefined,
      deferredHydrationTimeout,
    );
    expect(workspaceRow).toHaveTextContent("Chat mode");
    expect(screen.getByTestId("agents-composer-runtime-row-ideation")).toHaveTextContent(
      "Ideation mode",
    );
    expect(screen.getByTestId("agents-composer-runtime-row-verification")).toHaveTextContent(
      "PR Review",
    );
    expect(screen.getByTestId("agents-composer-runtime-row-workspace_review")).toHaveTextContent(
      "claude",
    );
    expect(screen.getByTestId("agents-composer-runtime-row-task")).toHaveTextContent(
      "Agent mode",
    );
    fireEvent.click(screen.getByTestId("agents-composer-workspace-changes-header"));
    expect(screen.queryByTestId("agents-composer-runtimes-list")).not.toBeInTheDocument();
    fireEvent.click(screen.getByTestId("agents-composer-runtimes-toggle"));
    const reopenedWorkspaceRow = await screen.findByTestId(
      "agents-composer-runtime-row-workspace",
      undefined,
      deferredHydrationTimeout,
    );

    fireEvent.click(reopenedWorkspaceRow);
    fireEvent.click(screen.getByTestId("agents-composer-runtime-row-ideation"));
    fireEvent.click(screen.getByTestId("agents-composer-runtime-row-verification"));
    fireEvent.click(screen.getByTestId("agents-composer-runtime-row-workspace_review"));
    fireEvent.click(screen.getByTestId("agents-composer-runtime-row-task"));

    expect(onSelectChatFocus).toHaveBeenCalledWith("workspace");
    expect(onFocusIdeationSession).toHaveBeenCalledWith("session-1");
    expect(onFocusVerificationSession).toHaveBeenCalledWith(
      "session-parent",
      "verification-child",
    );
    expect(onFocusWorkspaceReview).toHaveBeenCalledWith("review-conversation-1");
    expect(onFocusTaskRuntime).toHaveBeenCalledWith("task-3", "merge");
    expect(onOpenTaskArtifact).toHaveBeenCalledWith("task-3");
  });

  it("renders the composer task ledger for a focused task runtime chat", async () => {
    listAgentTasksMock.mockResolvedValue([
      {
        taskId: "runtime-ledger-task-1",
        taskNumber: 1,
        title: "Investigate child runtime ledger",
        state: "active",
        ownerAgent: "ralphx-general-worker",
        blockedBy: [],
        blocks: [],
        availability: "ready",
        updatedAt: "2026-07-04T23:00:00Z",
      },
    ]);

    renderPanel({
      chatFocus: {
        type: "task_runtime",
        taskId: "task-42",
        contextType: "task_execution",
      },
      chatFocusOptions: [
        {
          type: "workspace",
          label: "Workspace",
          description: "Show the workspace agent chat",
        },
        {
          type: "task_runtime",
          label: "Task",
          description: "Show the task agent chat",
          tone: "accent",
        },
      ],
    });

    await screen.findByTestId(
      "agents-composer-context-tray",
      undefined,
      deferredHydrationTimeout,
    );

    await waitFor(
      () =>
        expect(listAgentTasksMock).toHaveBeenCalledWith({
          contextType: "task_execution",
          contextId: "task-42",
          projectId: "project-1",
          includeDone: true,
        }),
      deferredHydrationTimeout,
    );

    fireEvent.click(screen.getByTestId("agents-composer-tasks-toggle"));

    await waitFor(() =>
      expect(screen.getByTestId("agents-composer-task-1")).toHaveTextContent(
        "Investigate child runtime ledger",
      ),
    );
  });

  it("does not pin task runtime chat to the selected workspace conversation", () => {
    renderPanel({
      selectedConversationId: "stale-task-conversation",
      chatFocus: {
        type: "task_runtime",
        taskId: "task-42",
        contextType: "task_execution",
      },
      chatFocusOptions: [
        {
          type: "workspace",
          label: "Workspace",
          description: "Show the workspace agent chat",
        },
        {
          type: "task_runtime",
          label: "Task",
          description: "Show the task agent chat",
          tone: "accent",
        },
      ],
    });

    const panel = screen.getByTestId("integrated-chat-panel");
    expect(panel).toHaveAttribute("data-conversation-id", "");
    expect(panel).toHaveAttribute("data-send-conversation-id", "");
    expect(panel).toHaveAttribute(
      "data-store-context-key",
      "task_execution:task-42",
    );
    expect(panel).toHaveAttribute("data-agent-process-context-id", "task-42");
  });

  it("keeps nonfocused workspace Review runtime query rows out of workspace composer chrome", async () => {
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

    await waitFor(() =>
      expect(getAgentConversationRuntimeStatusesMock).toHaveBeenCalledWith([
        "conversation-1",
      ]),
    );

    expect(screen.queryByTestId("agents-runtime-status-widget")).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "View Review" }),
    ).not.toBeInTheDocument();
    expect(onFocusWorkspaceReview).not.toHaveBeenCalled();
  });

  it("keeps the workspace Review runtime widget out of workspace composer chrome when workspace chat is focused", () => {
    const onFocusWorkspaceReview = vi.fn();
    getAgentConversationRuntimeStatusesMock.mockResolvedValue({
      "conversation-1": workspaceRuntimeStatus({
        isRunning: false,
        agentStatus: "idle",
        primarySource: null,
        summaryLabel: null,
        items: [],
      }),
    });

    renderPanel({
      chatFocus: { type: "workspace" },
      onFocusWorkspaceReview,
    });

    expect(screen.queryByTestId("agents-runtime-status-widget")).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "View Review" }),
    ).not.toBeInTheDocument();
    expect(onFocusWorkspaceReview).not.toHaveBeenCalled();
  });

  it("shows the workspace Review row in the Runtimes tab while workspace Review chat is focused", async () => {
    getAgentConversationRuntimeStatusesMock.mockResolvedValue({
      "conversation-1": workspaceRuntimeStatus({
        isRunning: false,
        agentStatus: "idle",
        primarySource: null,
        summaryLabel: null,
        items: [],
      }),
    });
    getAgentConversationRuntimeIndexMock.mockResolvedValue({
      conversationId: "conversation-1",
      rows: [
        runtimeIndexWorkspaceRow(),
        runtimeIndexWorkspaceRow({
          id: "workspace_review:review-conversation-1",
          group: "ideation_verification",
          kind: "workspace_review",
          lifecycle: "running",
          statusLabel: "Reviewing",
          title: "Review workspace changes",
          mode: null,
          orderIndex: 1,
          conversationId: "review-conversation-1",
          contextType: "project",
          contextId: "review-conversation-1",
          taskId: null,
        }),
      ],
    });

    renderPanel({
      chatFocus: {
        type: "workspace_review",
        conversationId: "review-conversation-1",
      },
    });

    fireEvent.click(await screen.findByTestId("agents-composer-runtimes-toggle"));

    expect(
      await screen.findByTestId(
        "agents-composer-runtime-row-workspace_review",
        undefined,
        deferredHydrationTimeout,
      ),
    ).toHaveTextContent("Review workspace changes");
    expect(screen.getByText("Viewing")).toBeInTheDocument();
  });

  it("queries parent runtime status when the selected conversation is a Review child", async () => {
    getAgentConversationRuntimeStatusesMock.mockResolvedValue({
      "conversation-1": workspaceRuntimeStatus({
        primarySource: "workspace_review",
        summaryLabel: "Reviewing",
      }),
    });

    renderPanel({
      activeConversation: {
        ...projectConversation(),
        id: "review-conversation-1",
        parentConversationId: "conversation-1",
      },
      selectedConversationId: "review-conversation-1",
      chatFocus: {
        type: "workspace_review",
        conversationId: "review-conversation-1",
      },
    });

    await waitFor(() =>
      expect(getAgentConversationRuntimeStatusesMock).toHaveBeenCalledWith([
        "conversation-1",
      ]),
    );
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

  it.each([
    { type: "workspace_repair" as const, conversationId: "repair-conversation-1" },
    { type: "pr_fixer" as const, conversationId: "pr-fixer-conversation-1" },
  ])("routes $type focus sends through its child project chat", (chatFocus) => {
    renderPanel({ chatFocus });

    const panel = screen.getByTestId("integrated-chat-panel");
    expect(panel).toHaveAttribute("data-conversation-id", chatFocus.conversationId);
    expect(panel).toHaveAttribute(
      "data-agent-process-context-id",
      chatFocus.conversationId,
    );
    expect(panel).toHaveAttribute(
      "data-store-context-key",
      `project:${chatFocus.conversationId}`,
    );
    expect(panel).toHaveAttribute(
      "data-send-conversation-id",
      chatFocus.conversationId,
    );
  });

  it("does not inherit parent Codex fast mode while focused on workspace Review", () => {
    renderPanel({
      activeConversation: {
        ...projectConversation(),
        providerHarness: "codex",
        logicalModel: "gpt-5.5",
        logicalEffort: "high",
        serviceTier: "fast",
      },
      chatFocus: {
        type: "workspace_review",
        conversationId: "review-conversation-1",
      },
      normalizedActiveRuntime: {
        provider: "codex",
        modelId: "gpt-5.5",
        effort: "high",
      },
    });

    const panel = screen.getByTestId("integrated-chat-panel");
    expect(panel).toHaveAttribute(
      "data-send-conversation-id",
      "review-conversation-1",
    );
    expect(panel).toHaveAttribute("data-send-codex-fast-mode", "false");
  });

  it("uses durable focused Review speed ahead of the client speed projection", () => {
    useAgentSessionStore.getState().setServiceTierForConversation(
      "review-conversation-1",
      "standard",
    );
    renderPanel({
      activeConversation: {
        ...projectConversation(),
        providerHarness: "codex",
        serviceTier: "standard",
      },
      chatFocus: {
        type: "workspace_review",
        conversationId: "review-conversation-1",
      },
      focusedWorkspaceReviewServiceTier: "fast",
      normalizedActiveRuntime: {
        provider: "codex",
        modelId: "gpt-5.5",
        effort: "high",
      },
    });

    expect(screen.getByTestId("agents-conversation-speed")).toHaveTextContent(
      "fast",
    );
  });

  it("uses the selectable workspace runtime for chat send options", () => {
    renderPanel({
      activeConversation: {
        ...projectConversation(),
        providerHarness: "codex",
        logicalModel: "gpt-5.6-terra",
        logicalEffort: "ultra",
      },
      normalizedActiveRuntime: {
        provider: "codex",
        modelId: "gpt-5.6-terra",
        effort: "ultra",
      },
    });

    expect(screen.getByTestId("workspace-model-value")).toHaveTextContent(
      "gpt-5.5",
    );
    const panel = screen.getByTestId("integrated-chat-panel");
    expect(panel).toHaveAttribute("data-send-provider-harness", "codex");
    expect(panel).toHaveAttribute("data-send-model-id", "gpt-5.5");
    expect(panel).toHaveAttribute("data-send-logical-effort", "xhigh");
  });

  it("sends the active agent conversation id and current model through the composer contract", () => {
    renderPanel({
      activeConversation: {
        ...projectConversation(),
        id: "agent-conversation-codex-1",
        providerHarness: "codex",
        logicalModel: "gpt-5.5",
        logicalEffort: "xhigh",
      },
      normalizedActiveRuntime: {
        provider: "codex",
        modelId: "gpt-5.5",
        effort: "xhigh",
      },
      selectedConversationId: "agent-conversation-codex-1",
    });

    const composerContract = screen.getByTestId("integrated-chat-panel");
    expect(composerContract).toHaveAttribute(
      "data-send-conversation-id",
      "agent-conversation-codex-1",
    );
    expect(composerContract).toHaveAttribute("data-send-model-id", "gpt-5.5");
  });

  it.each([
    {
      mode: "chat",
      provider: "claude",
      modelId: "sonnet",
      effort: "high",
    },
    {
      mode: "chat",
      provider: "codex",
      modelId: "gpt-5.5",
      effort: "xhigh",
    },
    {
      mode: "persona_builder",
      provider: "claude",
      modelId: "sonnet",
      effort: "high",
    },
    {
      mode: "persona_builder",
      provider: "codex",
      modelId: "gpt-5.5",
      effort: "xhigh",
    },
  ] as const)(
    "keeps $provider runtime on standalone $mode continuation sends",
    ({ mode, provider, modelId, effort }) => {
      renderPanel({
        activeConversation: {
          ...projectConversation(),
          id: "standalone-1",
          contextType: "standalone",
          contextId: "standalone-1",
          projectId: null,
          agentMode: mode,
          providerHarness: provider,
          logicalModel: modelId,
          logicalEffort: effort,
        },
        activeConversationMode: mode,
        activeProjectId: null,
        activeProjectOptions: [],
        activeWorkspace: null,
        normalizedActiveRuntime: { provider, modelId, effort },
        selectedConversationId: "standalone-1",
      });

      const panel = screen.getByTestId("integrated-chat-panel");
      expect(panel).toHaveAttribute("data-send-conversation-id", "standalone-1");
      expect(panel).toHaveAttribute("data-send-provider-harness", provider);
      expect(panel).toHaveAttribute("data-send-model-id", modelId);
      expect(panel).toHaveAttribute("data-send-logical-effort", effort);
    },
  );

  it("returns from child chat focus to the workspace chat from the header", async () => {
    const onSelectChatFocus = vi.fn();

    renderPanel({
      chatFocus: {
        type: "workspace_review",
        conversationId: "review-conversation-1",
      },
      onSelectChatFocus,
    });

    fireEvent.click(
      screen.getByRole("button", { name: "Back to Workspace Chat" }),
    );

    expect(onSelectChatFocus).toHaveBeenCalledWith("workspace");
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

  it("keeps a persisted disabled Team capability visible and lets it switch back to Defaults", async () => {
    const user = userEvent.setup();
    const onActiveCapabilityChange = vi.fn();

    renderPanel({
      activeConversation: {
        ...projectConversation(),
        coordinationMode: "rx_native_team",
      },
      onActiveCapabilityChange,
    });

    expect(
      screen.getByTestId("agents-conversation-capability-blocked"),
    ).toHaveTextContent(
      "This conversation's capability is disabled. Enable it in Settings > Capabilities or switch to Defaults.",
    );
    await user.click(screen.getByTestId("agents-conversation-capability"));
    expect(
      screen.getByTestId("agents-conversation-capability-rx_native_team"),
    ).toHaveTextContent("Team (disabled)");
    expect(
      screen.getByTestId("agents-conversation-capability-rx_native_team"),
    ).toBeDisabled();
    await user.click(
      screen.getByTestId("agents-conversation-capability-solo"),
    );

    expect(onActiveCapabilityChange).toHaveBeenCalledWith("solo");
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
    setPlanArtifactVisible();

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
    setPlanArtifactVisible();

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
        blueprintArtifactId: "blueprint-1",
        blueprintArtifactVersion: 1,
      }),
    );
  });

  it("shows Verify Plan beside Approve Plan for draft Plan-mode sessions", async () => {
    const user = userEvent.setup();
    const onSelectArtifact = vi.fn();
    getSessionPlanMock.mockResolvedValue(planArtifact("draft"));
    getVerificationSpecialistsMock.mockResolvedValue({
      specialists: [
        { name: "risk", enabled_by_default: false },
        { name: "scope", enabled_by_default: true },
      ],
    });
    setPlanArtifactVisible();

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

    const actions = within(
      await screen.findByTestId("agents-plan-composer-cta-actions"),
    );
    expect(
      actions.getByRole("button", { name: /Approve Plan/i }),
    ).toBeInTheDocument();

    await user.click(actions.getByRole("button", { name: /Verify Plan/i }));

    await waitFor(() =>
      expect(confirmVerificationMock).toHaveBeenCalledWith("planning-session-1"),
    );
    expect(onSelectArtifact).not.toHaveBeenCalledWith("verification");
    expect(approvePlanArtifactMock).not.toHaveBeenCalled();
  });

  it("hides View Plan when no plan artifact is attached yet", () => {
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
      hasAttachedPlanArtifact: false,
    });

    expect(
      screen.queryByTestId("agents-plan-composer-cta-row"),
    ).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: /View Plan/i }),
    ).not.toBeInTheDocument();
    expect(getSessionPlanMock).not.toHaveBeenCalled();
  });

  it("shows only View Plan when the plan tab is not visible", async () => {
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
      hasAttachedPlanArtifact: true,
      onOpenPlanArtifact,
    });

    const actionGroup = within(
      await screen.findByTestId("agents-plan-composer-cta-actions"),
    );
    const actionButtons = actionGroup.getAllByRole("button");
    const viewPlanButton = actionButtons[0];
    expect(viewPlanButton).toBeDefined();
    expect(actionButtons).toHaveLength(1);
    expect(viewPlanButton!).toHaveTextContent("View Plan");
    expect(
      actionGroup.queryByRole("button", { name: /Approve Plan/i }),
    ).not.toBeInTheDocument();
    expect(
      actionGroup.queryByRole("button", { name: /Verify Plan/i }),
    ).not.toBeInTheDocument();
    expect(getSessionPlanMock).not.toHaveBeenCalled();

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
      hasAttachedPlanArtifact: true,
    });

    const row = await screen.findByTestId("agents-plan-composer-cta-row");
    expect(
      within(row).queryByRole("button", { name: /View Plan/i }),
    ).not.toBeInTheDocument();
    expect(
      within(row).getByRole("button", { name: /Approve Plan/i }),
    ).toBeInTheDocument();
  });

  it("shows only View Plan when a non-Plan artifact tab is visible", async () => {
    const onOpenPlanArtifact = vi.fn();
    getSessionPlanMock.mockResolvedValue(planArtifact("draft"));
    useAgentArtifactUiStore.getState().setArtifactState("conversation-1", {
      isOpen: true,
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
      availableArtifactTabs: ["plan", "tasks"],
      hasAttachedPlanArtifact: true,
      onOpenPlanArtifact,
    });

    const row = await screen.findByTestId("agents-plan-composer-cta-row");
    expect(
      within(row).getByRole("button", { name: /View Plan/i }),
    ).toBeInTheDocument();
    expect(
      within(row).queryByRole("button", { name: /Approve Plan/i }),
    ).not.toBeInTheDocument();
    expect(
      within(row).queryByRole("button", { name: /Verify Plan/i }),
    ).not.toBeInTheDocument();
    expect(getSessionPlanMock).not.toHaveBeenCalled();
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
    setPlanArtifactVisible();
    const promotedWorkspace = {
      ...workspace(),
      mode: "tasks" as const,
      linkedIdeationSessionId: "planning-session-1",
      taskPipelineSessionId: "planning-session-1",
      taskPipelineAvailable: true,
    };
    activateAgentTaskPipelineMock.mockResolvedValue(promotedWorkspace);
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
      expect(activateAgentTaskPipelineMock).toHaveBeenCalledWith({
        conversationId: "conversation-1",
        sessionId: "planning-session-1",
        runtimeOverride: approvedPlanRuntime,
      }),
    );
    expect(sendAgentMessageMock).toHaveBeenCalledWith(
      "ideation",
      "planning-session-1",
      expect.stringContaining("Create implementation task proposals"),
      undefined,
      { runtimeOverride: approvedPlanRuntime },
    );
    expect(onConversationModeSwitched).toHaveBeenCalledWith(
      "conversation-1",
      "tasks",
      promotedWorkspace,
    );
  });

  it("refetches authoritative Tasks state when proposal launch fails after activation", async () => {
    const user = userEvent.setup();
    getSessionPlanMock.mockResolvedValue(planArtifact("approved"));
    getPlanComplexityAssessmentMock.mockResolvedValue({
      id: "assessment-partial",
      sessionId: "planning-session-1",
      artifactId: "artifact-1",
      artifactVersion: 1,
      level: "complex",
      score: 80,
      recommendedAction: "create_proposals",
      confidence: 0.9,
      reasonSummary: "Use tracked proposals.",
      signals: {},
      assessedBy: "ralphx-utility-plan-complexity",
      createdAt: "2026-05-23T05:02:00Z",
      updatedAt: "2026-05-23T05:02:00Z",
    });
    setPlanArtifactVisible();
    const tasksWorkspace = {
      ...workspace(),
      mode: "tasks" as const,
      linkedIdeationSessionId: "planning-session-1",
      taskPipelineSessionId: "planning-session-1",
      taskPipelineAvailable: true,
    };
    activateAgentTaskPipelineMock.mockResolvedValue(tasksWorkspace);
    getAgentConversationWorkspaceMock.mockResolvedValue(tasksWorkspace);
    sendAgentMessageMock.mockRejectedValueOnce(new Error("send failed"));
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
    await user.click(
      within(row).getByRole("button", { name: /Create Proposals/i }),
    );

    await waitFor(() => {
      expect(getAgentConversationWorkspaceMock).toHaveBeenCalledWith(
        "conversation-1",
      );
    });
    expect(activateAgentTaskPipelineMock).toHaveBeenCalledTimes(1);
    expect(onConversationModeSwitched).toHaveBeenLastCalledWith(
      "conversation-1",
      "tasks",
      tasksWorkspace,
    );
    expect(toastSuccessMock).not.toHaveBeenCalledWith(
      "Proposal creation requested",
    );
  });

  it("retries proposal launch with the runtime tuple already committed to Tasks", async () => {
    const user = userEvent.setup();
    getSessionPlanMock.mockResolvedValue(planArtifact("approved"));
    getPlanComplexityAssessmentMock.mockResolvedValue({
      id: "assessment-retry-runtime",
      sessionId: "planning-session-1",
      artifactId: "artifact-1",
      artifactVersion: 1,
      level: "complex",
      score: 80,
      recommendedAction: "create_proposals",
      confidence: 0.9,
      reasonSummary: "Use tracked proposals.",
      signals: {},
      assessedBy: "ralphx-utility-plan-complexity",
      createdAt: "2026-05-23T05:02:00Z",
      updatedAt: "2026-05-23T05:02:00Z",
    });
    setPlanArtifactVisible();
    const tasksWorkspace = {
      ...workspace(),
      mode: "tasks" as const,
      linkedIdeationSessionId: "planning-session-1",
      taskPipelineSessionId: "planning-session-1",
      taskPipelineAvailable: true,
    };
    const changedRuntime = {
      ...approvedPlanRuntime,
      model: "different-model",
    };
    activateAgentTaskPipelineMock.mockResolvedValue(tasksWorkspace);
    getAgentConversationWorkspaceMock.mockResolvedValue(tasksWorkspace);
    sendAgentMessageMock
      .mockRejectedValueOnce(new Error("send failed"))
      .mockResolvedValueOnce({
        conversationId: "ideation-conversation-1",
        agentRunId: "run-retry",
        isNewConversation: false,
        wasQueued: false,
        queuedAsPending: false,
        queuedMessageId: null,
      });
    confirmCreateProposalsMock.mockImplementation(
      async (
        onConfirm: (runtime: typeof approvedPlanRuntime) => Promise<unknown>,
      ) => {
        await onConfirm(approvedPlanRuntime).catch(() => undefined);
        await onConfirm(changedRuntime).catch(() => undefined);
      },
    );

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
    await user.click(
      within(row).getByRole("button", { name: /Create Proposals/i }),
    );

    await waitFor(() => expect(sendAgentMessageMock).toHaveBeenCalledTimes(2));
    expect(activateAgentTaskPipelineMock).toHaveBeenCalledTimes(1);
    expect(sendAgentMessageMock.mock.calls[0]?.[4]).toEqual({
      runtimeOverride: approvedPlanRuntime,
    });
    expect(sendAgentMessageMock.mock.calls[1]?.[4]).toEqual({
      runtimeOverride: approvedPlanRuntime,
    });
  });

  it("offers only direct implementation for an approved plan while Tasks are off", async () => {
    tasksEnabledRef.current = false;
    getSessionPlanMock.mockResolvedValue(planArtifact("approved"));
    setPlanArtifactVisible();

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
    expect(
      within(row).getByRole("button", { name: /Implement Directly/i }),
    ).toBeEnabled();
    expect(
      within(row).queryByRole("button", { name: /Create Proposals/i }),
    ).not.toBeInTheDocument();
    expect(
      within(row).getByTestId("agents-plan-composer-cta-hint"),
    ).toHaveTextContent("Recommended: Implement Directly");
    expect(within(row).queryByText("why?")).not.toBeInTheDocument();
    expect(getPlanComplexityAssessmentMock).not.toHaveBeenCalled();
  });

  it("focuses the linked ideation chat and pins the returned conversation from the composer CTA row", async () => {
    const user = userEvent.setup();
    getSessionPlanMock.mockResolvedValue(planArtifact("approved"));
    setPlanArtifactVisible();
    sendAgentMessageMock.mockResolvedValue({
      conversationId: "ideation-conversation-1",
      agentRunId: "run-proposals",
      isNewConversation: true,
      wasQueued: false,
      queuedAsPending: false,
      queuedMessageId: null,
    });
    const promotedWorkspace = {
      ...workspace(),
      mode: "ideation" as const,
      linkedIdeationSessionId: "planning-session-1",
    };
    switchAgentConversationModeMock.mockResolvedValue({
      workspace: promotedWorkspace,
    });
    const onFocusIdeationSessionForConversation = vi.fn();

    renderPanel({
      activeConversation: { ...projectConversation(), agentMode: "plan" },
      activeConversationMode: "plan",
      activeWorkspace: {
        ...workspace(),
        mode: "plan",
        linkedIdeationSessionId: "planning-session-1",
      },
      attachedIdeationSessionId: "planning-session-1",
      onFocusIdeationSessionForConversation,
    });

    const row = await screen.findByTestId("agents-plan-composer-cta-row");
    await user.click(
      within(row).getByRole("button", { name: /Create Proposals/i }),
    );

    await waitFor(() =>
      expect(onFocusIdeationSessionForConversation).toHaveBeenCalledWith(
        "conversation-1",
        "planning-session-1",
      ),
    );
    expect(
      useChatStore.getState().activeConversationIds["session:planning-session-1"],
    ).toBe("ideation-conversation-1");
  });

  it("does not focus the ideation chat when activation does not confirm Tasks mode", async () => {
    const user = userEvent.setup();
    getSessionPlanMock.mockResolvedValue(planArtifact("approved"));
    setPlanArtifactVisible();
    sendAgentMessageMock.mockResolvedValue({
      conversationId: "ideation-conversation-1",
      agentRunId: "run-proposals",
      isNewConversation: true,
      wasQueued: false,
      queuedAsPending: false,
      queuedMessageId: null,
    });
    activateAgentTaskPipelineMock.mockResolvedValue({
      ...workspace(),
      mode: "plan",
      linkedIdeationSessionId: "planning-session-1",
    });
    const onFocusIdeationSessionForConversation = vi.fn();

    renderPanel({
      activeConversation: { ...projectConversation(), agentMode: "plan" },
      activeConversationMode: "plan",
      activeWorkspace: {
        ...workspace(),
        mode: "plan",
        linkedIdeationSessionId: "planning-session-1",
      },
      attachedIdeationSessionId: "planning-session-1",
      onFocusIdeationSessionForConversation,
    });

    const row = await screen.findByTestId("agents-plan-composer-cta-row");
    await user.click(
      within(row).getByRole("button", { name: /Create Proposals/i }),
    );

    await waitFor(() => expect(sendAgentMessageMock).toHaveBeenCalled());
    expect(onFocusIdeationSessionForConversation).not.toHaveBeenCalled();
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
    setPlanArtifactVisible();

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
    setPlanArtifactVisible();

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

  it("hides approved plan composer CTAs for automation run conversations", async () => {
    getSessionPlanMock.mockResolvedValue(planArtifact("approved"));
    setPlanArtifactVisible();

    renderPanel({
      activeConversation: {
        ...projectConversation(),
        agentMode: "plan",
        automationId: "automation-1",
        automationRunId: "run-1",
      },
      activeConversationMode: "plan",
      activeWorkspace: {
        ...workspace(),
        mode: "plan",
        linkedIdeationSessionId: "planning-session-1",
      },
      attachedIdeationSessionId: "planning-session-1",
    });

    await waitFor(() =>
      expect(
        screen.queryByRole("button", { name: /Implement Directly/i }),
      ).not.toBeInTheDocument(),
    );
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
    setPlanArtifactVisible();

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
    expect(onConversationModeSwitched).toHaveBeenCalledTimes(1);
    await waitFor(() =>
      expect(sendAgentMessageMock).toHaveBeenCalledTimes(1),
    );
    expect(sendAgentMessageMock).toHaveBeenCalledWith(
      "project",
      "project-1",
      expect.stringContaining(
        "Planning focus: The CLI surface needs planning before implementation.",
      ),
      undefined,
      expect.objectContaining({
        conversationId: "conversation-1",
        providerHarness: "claude",
        modelId: "opus",
        logicalEffort: "high",
      }),
    );
    expect(onAgentUserMessageSent).toHaveBeenCalledWith(
      expect.objectContaining({
        content: expect.stringContaining("Continue in Plan mode"),
      }),
    );
  });

  it("does not switch or continue a backend-handled plan-mode proposal after retries", async () => {
    vi.useFakeTimers();

    renderPanel({
      activeConversation: { ...projectConversation(), agentMode: "edit" },
      activeConversationMode: "edit",
      activeWorkspace: { ...workspace(), mode: "edit" },
    });

    fireEvent.click(
      screen.getByTestId("accept-backend-handled-plan-mode-proposal"),
    );
    await act(async () => {
      await Promise.resolve();
    });

    emitEvent("agent:run_completed", {
      conversation_id: "conversation-1",
      context_type: "project",
      context_id: "conversation-1",
    });
    await act(async () => {
      vi.advanceTimersByTime(150);
      await Promise.resolve();
      vi.advanceTimersByTime(600);
      await Promise.resolve();
    });

    expect(switchAgentConversationModeMock).not.toHaveBeenCalled();
    expect(sendAgentMessageMock).not.toHaveBeenCalled();
  });

  it("continues once without switching when an unhandled proposal is cached in Plan mode", async () => {
    const onConversationModeSwitched = vi.fn();

    const { queryClient } = renderPanel({
      activeConversation: { ...projectConversation(), agentMode: "edit" },
      activeConversationMode: "edit",
      activeWorkspace: { ...workspace(), mode: "edit" },
      onConversationModeSwitched,
    });
    queryClient.setQueryData(agentWorkspaceKeys.workspace("conversation-1"), {
      ...workspace(),
      mode: "plan",
    });

    fireEvent.click(screen.getByTestId("accept-plan-mode-proposal"));

    await waitFor(() =>
      expect(sendAgentMessageMock).toHaveBeenCalledTimes(1),
    );
    expect(switchAgentConversationModeMock).not.toHaveBeenCalled();
    expect(onConversationModeSwitched).toHaveBeenCalledTimes(1);
  });

  it("uses cached Plan state when a deferred proposal retry runs", async () => {
    vi.useFakeTimers();
    const planWorkspace = { ...workspace(), mode: "plan" as const };
    const onConversationModeSwitched = vi.fn();
    switchAgentConversationModeMock.mockRejectedValueOnce(
      new Error("Cannot change mode while the agent is running"),
    );

    const { queryClient } = renderPanel({
      activeConversation: { ...projectConversation(), agentMode: "edit" },
      activeConversationMode: "edit",
      activeWorkspace: { ...workspace(), mode: "edit" },
      onConversationModeSwitched,
    });

    fireEvent.click(screen.getByTestId("accept-plan-mode-proposal"));
    await act(async () => {
      await Promise.resolve();
      await Promise.resolve();
    });
    expect(switchAgentConversationModeMock).toHaveBeenCalledTimes(1);

    queryClient.setQueryData(
      agentWorkspaceKeys.workspace("conversation-1"),
      planWorkspace,
    );
    emitEvent("agent:run_completed", {
      conversation_id: "conversation-1",
      context_type: "project",
      context_id: "conversation-1",
    });
    await act(async () => {
      vi.advanceTimersByTime(150);
      await Promise.resolve();
    });

    expect(switchAgentConversationModeMock).toHaveBeenCalledTimes(1);
    expect(onConversationModeSwitched).toHaveBeenCalledWith(
      "conversation-1",
      "plan",
      planWorkspace,
    );
    expect(sendAgentMessageMock).toHaveBeenCalledTimes(1);

    await act(async () => {
      vi.advanceTimersByTime(600);
      await Promise.resolve();
    });
    expect(switchAgentConversationModeMock).toHaveBeenCalledTimes(1);
    expect(sendAgentMessageMock).toHaveBeenCalledTimes(1);
  });

  it("shares one retry attempt when the event and fallback timers overlap", async () => {
    vi.useFakeTimers();
    const planWorkspace = { ...workspace(), mode: "plan" as const };
    const onConversationModeSwitched = vi.fn();
    const retry = deferred<{ workspace: AgentConversationWorkspace }>();
    switchAgentConversationModeMock
      .mockRejectedValueOnce(
        new Error("Cannot change mode while the agent is running"),
      )
      .mockReturnValueOnce(retry.promise);

    renderPanel({
      activeConversation: { ...projectConversation(), agentMode: "edit" },
      activeConversationMode: "edit",
      activeWorkspace: { ...workspace(), mode: "edit" },
      onConversationModeSwitched,
    });

    fireEvent.click(screen.getByTestId("accept-plan-mode-proposal"));
    expect(switchAgentConversationModeMock).toHaveBeenCalledTimes(1);
    await act(async () => {
      await Promise.resolve();
      await Promise.resolve();
    });

    act(() => {
      emitEvent("agent:run_completed", {
        conversation_id: "conversation-1",
        context_type: "project",
        context_id: "conversation-1",
      });
      vi.advanceTimersByTime(150);
    });
    expect(switchAgentConversationModeMock).toHaveBeenCalledTimes(2);

    act(() => {
      vi.advanceTimersByTime(600);
    });
    expect(switchAgentConversationModeMock).toHaveBeenCalledTimes(2);

    await act(async () => {
      retry.resolve({ workspace: planWorkspace });
      await retry.promise;
      await Promise.resolve();
    });

    expect(onConversationModeSwitched).toHaveBeenCalledTimes(1);
    expect(sendAgentMessageMock).toHaveBeenCalledTimes(1);
  });

  it("does not continue again when a completed-run event replays after a successful retry", async () => {
    vi.useFakeTimers();
    const planWorkspace = { ...workspace(), mode: "plan" as const };
    switchAgentConversationModeMock
      .mockRejectedValueOnce(
        new Error("Cannot change mode while the agent is running"),
      )
      .mockResolvedValueOnce({ workspace: planWorkspace });

    renderPanel({
      activeConversation: { ...projectConversation(), agentMode: "edit" },
      activeConversationMode: "edit",
      activeWorkspace: { ...workspace(), mode: "edit" },
    });

    fireEvent.click(screen.getByTestId("accept-plan-mode-proposal"));
    await act(async () => {
      await Promise.resolve();
      await Promise.resolve();
    });
    act(() => {
      emitEvent("agent:run_completed", {
        conversation_id: "conversation-1",
        context_type: "project",
        context_id: "conversation-1",
      });
      vi.advanceTimersByTime(150);
    });
    await act(async () => {
      await Promise.resolve();
      await Promise.resolve();
    });
    expect(sendAgentMessageMock).toHaveBeenCalledTimes(1);

    act(() => {
      emitEvent("agent:run_completed", {
        conversation_id: "conversation-1",
        context_type: "project",
        context_id: "conversation-1",
      });
      vi.advanceTimersByTime(1_000);
    });

    expect(switchAgentConversationModeMock).toHaveBeenCalledTimes(2);
    expect(sendAgentMessageMock).toHaveBeenCalledTimes(1);
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
    setPlanArtifactVisible();
    const editWorkspace = {
      ...workspace(),
      mode: "edit" as const,
      linkedIdeationSessionId: "planning-session-1",
    };
    activateAgentPlanDirectImplementationMock.mockResolvedValue({
      workspace: editWorkspace,
      artifactReferences: [
        {
          artifactId: "artifact-1",
          kind: "plan",
          sessionId: "planning-session-1",
          version: 1,
          status: "approved",
        },
        {
          artifactId: "blueprint-1",
          kind: "plan_blueprint",
          sessionId: "planning-session-1",
          version: 2,
          status: "approved",
        },
      ],
      planContextFingerprint: "plan-context-fingerprint-1",
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
      expect(activateAgentPlanDirectImplementationMock).toHaveBeenCalledWith({
        conversationId: "conversation-1",
        sessionId: "planning-session-1",
        retry: false,
      }),
    );
    expect(switchAgentConversationModeMock).not.toHaveBeenCalled();
    expect(sendAgentMessageMock).toHaveBeenCalledWith(
      "project",
      "project-1",
      expect.stringContaining("Implement the approved plan directly"),
      undefined,
      expect.objectContaining({
        conversationId: "conversation-1",
        runtimeOverride: approvedPlanRuntime,
        requireApprovedLinkedPlan: true,
        expectedLinkedPlanFingerprint: "plan-context-fingerprint-1",
        suppressUserMessage: true,
      }),
    );
    expect(
      sendAgentMessageMock.mock.calls[0]?.[4],
    ).not.toHaveProperty("composerArtifactReferences");
    expect(sendAgentMessageMock.mock.calls[0]?.[2]).not.toContain(
      "do not create task proposals",
    );
    expect(onConversationModeSwitched).toHaveBeenCalledWith(
      "conversation-1",
      "edit",
      editWorkspace,
    );
  });

  it("refetches authoritative Edit state when the post-transition launch fails", async () => {
    const user = userEvent.setup();
    getSessionPlanMock.mockResolvedValue(planArtifact("approved"));
    setPlanArtifactVisible();
    const transitionedWorkspace = {
      ...workspace(),
      mode: "edit" as const,
      linkedIdeationSessionId: "planning-session-1",
    };
    activateAgentPlanDirectImplementationMock.mockResolvedValue({
      workspace: transitionedWorkspace,
      artifactReferences: [
        {
          artifactId: "artifact-1",
          kind: "plan",
          sessionId: "planning-session-1",
          version: 1,
          status: "approved",
        },
        {
          artifactId: "blueprint-1",
          kind: "plan_blueprint",
          sessionId: "planning-session-1",
          version: 2,
          status: "approved",
        },
      ],
      planContextFingerprint: "plan-context-fingerprint-1",
    });
    getAgentConversationWorkspaceMock.mockResolvedValue(transitionedWorkspace);
    sendAgentMessageMock.mockRejectedValueOnce(new Error("provider unavailable"));
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
    await user.click(
      within(row).getByRole("button", { name: /Implement Directly/i }),
    );

    await waitFor(() => {
      expect(getAgentConversationWorkspaceMock).toHaveBeenCalledWith(
        "conversation-1",
      );
    });
    expect(activateAgentPlanDirectImplementationMock).toHaveBeenCalledTimes(1);
    expect(switchAgentConversationModeMock).not.toHaveBeenCalled();
    expect(onConversationModeSwitched).toHaveBeenLastCalledWith(
      "conversation-1",
      "edit",
      transitionedWorkspace,
    );
    expect(toastSuccessMock).not.toHaveBeenCalledWith("Implementation started");
  });

  it("retries a post-transition launch with the runtime tuple already committed to Edit", async () => {
    const user = userEvent.setup();
    getSessionPlanMock.mockResolvedValue(planArtifact("approved"));
    setPlanArtifactVisible();
    const transitionedWorkspace = {
      ...workspace(),
      mode: "edit" as const,
      linkedIdeationSessionId: "planning-session-1",
    };
    const changedRuntime = {
      ...approvedPlanRuntime,
      model: "different-model",
    };
    getAgentConversationWorkspaceMock.mockResolvedValue(transitionedWorkspace);
    sendAgentMessageMock
      .mockRejectedValueOnce(new Error("provider unavailable"))
      .mockResolvedValueOnce({
        conversationId: "conversation-1",
        agentRunId: "run-retry",
        isNewConversation: false,
        wasQueued: false,
        queuedAsPending: false,
        queuedMessageId: null,
      });
    confirmImplementDirectlyMock.mockImplementation(
      async (
        onConfirm: (runtime: typeof approvedPlanRuntime) => Promise<unknown>,
      ) => {
        await onConfirm(approvedPlanRuntime).catch(() => undefined);
        await onConfirm(changedRuntime).catch(() => undefined);
      },
    );

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
    await user.click(
      within(row).getByRole("button", { name: /Implement Directly/i }),
    );

    await waitFor(() => expect(sendAgentMessageMock).toHaveBeenCalledTimes(2));
    expect(activateAgentPlanDirectImplementationMock).toHaveBeenNthCalledWith(
      1,
      {
        conversationId: "conversation-1",
        sessionId: "planning-session-1",
        retry: false,
      },
    );
    expect(activateAgentPlanDirectImplementationMock).toHaveBeenNthCalledWith(
      2,
      {
        conversationId: "conversation-1",
        sessionId: "planning-session-1",
        retry: true,
      },
    );
    expect(switchAgentConversationModeMock).not.toHaveBeenCalled();
    expect(sendAgentMessageMock.mock.calls[0]?.[4]).toEqual(
      expect.objectContaining({
        conversationId: "conversation-1",
        runtimeOverride: approvedPlanRuntime,
        suppressUserMessage: true,
      }),
    );
    expect(sendAgentMessageMock.mock.calls[1]?.[4]).toEqual(
      expect.objectContaining({
        conversationId: "conversation-1",
        runtimeOverride: approvedPlanRuntime,
        suppressUserMessage: true,
      }),
    );
  });

  it("starts plan verification from the composer CTA row", async () => {
    const user = userEvent.setup();
    const onSelectArtifact = vi.fn();
    getSessionPlanMock.mockResolvedValue(planArtifact("approved"));
    setPlanArtifactVisible();
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
      expect(confirmVerificationMock).toHaveBeenCalledWith("planning-session-1"),
    );
    expect(onSelectArtifact).not.toHaveBeenCalledWith("verification");
  });

  it("keeps a verified composer control and confirms a manual rerun", async () => {
    const user = userEvent.setup();
    useVerificationStatusMock.mockReturnValue({
      data: { status: "verified", inProgress: false },
      isLoading: false,
      isFetching: false,
    });
    getSessionPlanMock.mockResolvedValue(planArtifact("approved"));
    setPlanArtifactVisible();

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

    await user.click(
      within(
        await screen.findByTestId("agents-plan-composer-cta-row"),
      ).getByRole("button", { name: "Verified" }),
    );

    expect(screen.getByText("Verify this plan again?")).toBeInTheDocument();
    expect(confirmVerificationMock).not.toHaveBeenCalled();

    await user.click(screen.getByRole("button", { name: "Verify again" }));
    await waitFor(() =>
      expect(confirmVerificationMock).toHaveBeenCalledWith("planning-session-1"),
    );
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
        {
          conversationId: "conversation-fork",
          providerHarness: "codex",
          modelId: "gpt-5.5",
          logicalEffort: "high",
          codexFastMode: null,
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
