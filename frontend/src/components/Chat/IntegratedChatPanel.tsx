/**
 * IntegratedChatPanel - Context-aware chat panel for split-screen layout
 *
 * This is the shared embedded chat surface that:
 * - Is part of the layout, not fixed positioned
 * - Supports context switching based on selected task
 * - No slide animations (instant show/hide)
 *
 * Design spec: specs/design/refined-studio-patterns.md
 */

import {
  useState,
  useRef,
  useEffect,
  useLayoutEffect,
  useMemo,
  useCallback,
} from "react";
import { type VirtuosoHandle } from "react-virtuoso";
import {
  useChat,
  useConversationHistoryWindow,
  useConversationTimelineWindow,
  getCachedConversationMessages,
  isOptimisticConversationId,
  chatKeys,
} from "@/hooks/useChat";
import {
  useChatStore,
  selectQueuedMessages,
  selectAgentActivityLabel,
  selectAgentStatus,
  selectActiveAgentRunId,
  selectActiveAgentRunHarness,
  selectIsAgentRunning,
  selectIsSending,
  selectToolCallStartTimes,
  selectLastAgentEventTimestamp,
  selectComposerDraft,
  type AgentStatus,
} from "@/stores/chatStore";
import { useUiStore } from "@/stores/uiStore";
import { useProjectStore } from "@/stores/projectStore";
import { useTaskStore } from "@/stores/taskStore";
import { useTasks, taskKeys } from "@/hooks/useTasks";
import { useChatPanelContext } from "@/hooks/useChatPanelContext";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import {
  chatApi,
  type ComposerArtifactReference,
  type ComposerExcerptReference,
  type ComposerIntegrationReference,
  type ComposerProjectReference,
  type CapabilityIntent,
  type SendAgentMessageResult,
  type TeamIntent,
  type TeamMessageTarget,
} from "@/api/chat";
import { isVisibleChatMessage } from "@/api/chat-message-visibility";
import type { MessageFolderReference } from "./MessageReferences.parse";
import { api } from "@/lib/tauri";
import { withAlpha } from "@/lib/theme-colors";
import { getContextConfig, buildStoreKey } from "@/lib/chat-context-registry";
import type { Task } from "@/types/task";
import type { ContextType } from "@/types/chat-conversation";
import {
  ALL_REVIEW_STATUSES,
  EXECUTION_STATUSES,
  MERGE_STATUSES,
} from "@/types/status";
import { AGENT_MERGER, AGENT_WORKER, AGENT_REVIEWER } from "@/constants/agents";
import { type AgentType } from "./StatusActivityBadge";
import { ChatSessionToolbar } from "./ChatSessionToolbar";
import { ChatSessionChips } from "./ChatSessionChips";
import { ConversationSelector } from "./ConversationSelector";
import { QueuedMessageList } from "./QueuedMessageList";
import { ChatInput, type QuestionMode } from "./ChatInput";
import {
  ChatMessageList,
  type PersonaAttributedRun,
} from "./ChatMessageList";
import {
  EmptyState,
  LoadingState,
  ContextIndicator,
  PreviousRunBanner,
  animationStyles,
  HistoryEmptyState,
} from "./IntegratedChatPanel.components";
import { useChatActions } from "@/hooks/useChatActions";
import { useChatEvents } from "@/hooks/useChatEvents";
import { useChatRecovery } from "@/hooks/useChatRecovery";
import {
  projectPersistedStreamingContentBlocks,
  applyTranscriptInput,
  createLiveTranscriptState,
  renderTranscriptBlocks,
} from "@/hooks/chat-active-state";
import { useQueuedMessagesHydration } from "@/hooks/useQueuedMessagesHydration";
// useAgentEvents is already called inside useChat — no direct import needed
import {
  useAskUserQuestion,
  type SubmitQuestionAnswerResult,
} from "@/hooks/useAskUserQuestion";
import { useQuestionInput } from "@/hooks/useQuestionInput";
import { QuestionInputBanner } from "./QuestionInputBanner";
import type {
  AskUserQuestionPayload,
  AskUserQuestionResponse,
} from "@/types/ask-user-question";
import { RecoveryPromptDialog } from "@/components/recovery/RecoveryPromptDialog";
import { useEventBus } from "@/providers/EventProvider";
import { logger } from "@/lib/logger";
import { ChildSessionNotification } from "./ChildSessionNotification";
import {
  useChatAttachments,
  type ChatAttachment as PendingChatAttachment,
} from "@/hooks/useChatAttachments";
import { useFeatureFlags } from "@/hooks/useFeatureFlags";
import { usePersonas, useSwitchConversationPersona } from "@/hooks/usePersonas";
import { useConfirmation } from "@/hooks/useConfirmation";
import { usePersonaRunEvents } from "@/hooks/usePersonaRunEvents";
import { useIdeationStore } from "@/stores/ideationStore";
import { PersonaUnavailableNotice } from "@/components/personas/PersonaUnavailableNotice";
import { extractErrorMessage } from "@/lib/errors";
import { PERSONA_UNAVAILABLE_PREFIX } from "@/lib/personaErrors";
import { getModelLabel } from "@/lib/model-utils";
import { resolveChatInputDelivery } from "@/lib/chat-input-delivery";
import { selectEffectiveModel } from "@/stores/chatStore";
import { TimeoutWarning } from "./TimeoutWarning";
import { ChildSessionNavigationContext } from "./tool-widgets/ChildSessionNavigationContext";
import { ChildSessionTranscriptModal } from "./ChildSessionTranscriptModal";
import { cn } from "@/lib/utils";
import { PersonaChip } from "./PersonaChip";
import { useChatBottomInset } from "./useChatBottomInset";
import type { ComposerRuntimePersonaField } from "@/components/agents/composer/runtime/runtimeSelectorTypes";
import { toast } from "sonner";

// Stable empty array to avoid new reference on every render when tasks query returns undefined
const EMPTY_TASKS: never[] = [];
const AUTOMATION_SETUP_PROPOSAL_KIND = "automation_setup_proposal";
const AUTOMATION_SETUP_PROPOSAL_APPLY_VALUE = "apply_automation_proposal";

type TranscriptWindowData =
  | {
      messages: readonly unknown[];
      totalMessageCount?: number;
    }
  | undefined;

type PersonaRetryAttempt = {
  message: string;
  options:
    | {
        folderReferences?: MessageFolderReference[];
        projectReferences?: ComposerProjectReference[];
        integrationReferences?: ComposerIntegrationReference[];
        artifactReferences?: ComposerArtifactReference[];
        excerptReferences?: ComposerExcerptReference[];
        capabilityIntent?: CapabilityIntent | null;
        teamIntent?: TeamIntent | null;
        teamMessageTarget?: TeamMessageTarget | null;
      }
    | undefined;
};

function transcriptWindowHasMessages(data: TranscriptWindowData): boolean {
  return (data?.totalMessageCount ?? data?.messages.length ?? 0) > 0;
}

function automationProposalApplyOptionIndex(
  question: AskUserQuestionPayload | null | undefined,
): number {
  if (!question) return -1;

  const optionIndex = question.options.findIndex(
    (option) => option.value === AUTOMATION_SETUP_PROPOSAL_APPLY_VALUE,
  );
  if (optionIndex < 0) return -1;

  if (question.metadata?.kind === AUTOMATION_SETUP_PROPOSAL_KIND) {
    return optionIndex;
  }

  const header = question.header?.toLowerCase() ?? "";
  return header.includes("automation") ? optionIndex : -1;
}

// ============================================================================
// Main Component
// ============================================================================

interface IntegratedChatPanelProps {
  /** Project ID for context */
  projectId: string | null;
  /** Explicit non-project conversation context owned by the host. */
  contextTypeOverride?: ContextType | undefined;
  contextIdOverride?: string | undefined;
  /** Optional ideation session ID - when set, uses ideation context */
  ideationSessionId?: string;
  /** Custom empty state component */
  emptyState?: React.ReactNode;
  /** Always show helper text under input */
  showHelperTextAlways?: boolean;
  /** Custom class for input container */
  inputContainerClassName?: string;
  /** Custom header content to replace default context indicator */
  headerContent?: React.ReactNode;
  /** Optional secondary row rendered below the primary chat header. */
  headerSubContent?: React.ReactNode;
  /** Hide provider/model/stat chips and conversation switcher in the header. */
  hideHeaderSessionControls?: boolean;
  /** Hide the secondary session toolbar below the header. */
  hideSessionToolbar?: boolean;
  /** Optional override for the chat surface background. */
  surfaceBackground?: string;
  /** Optional max-width wrapper for conversation content. */
  contentWidthClassName?: string;
  /** Extra session ids whose ask-user prompts should surface in this chat. */
  additionalQuestionSessionIds?: string[];
  /** Optional inline action rendered in the active question banner. */
  planApprovalAction?: {
    label: string;
    pendingLabel?: string;
    onClick: () => void;
    disabled?: boolean;
    isPending?: boolean;
  };
  /** Called when Escape is pressed with input blurred - used to close the panel */
  onClose?: () => void;
  /** Whether to autofocus chat input on mount */
  autoFocusInput?: boolean;
  /** Whether this panel is currently visible (used in dual-panel mode to suppress toasts on hidden panel) */
  isVisible?: boolean;
  /** Back navigation action rendered in the toolbar (e.g. "Back to Plan") */
  toolbarBackAction?: {
    label: string;
    icon?: React.ReactNode;
    onClick: () => void;
  };
  /** Force a specific conversation ID for externally-owned session lists. */
  conversationIdOverride?: string | undefined;
  /** Override task selection so host surfaces can ignore the global task detail state. */
  selectedTaskIdOverride?: string | null | undefined;
  /** Force task chat runtime mode when selected task status alone is not deterministic. */
  taskRuntimeContextTypeOverride?:
    "task_execution" | "review" | "merge" | "branch_update" | undefined;
  /** Force a specific store key for externally-owned queue/running state. */
  storeContextKeyOverride?: string | undefined;
  /** Override the backend process/queue context id used for recovery, stop, and queued-message edits. */
  agentProcessContextIdOverride?: string | undefined;
  /** Optional first-spawn provider/model overrides. */
  sendOptions?: {
    conversationId?: string | null;
    providerHarness?: string | null;
    modelId?: string | null;
    logicalEffort?: string | null;
    codexFastMode?: boolean | null;
  };
  /** Optional host-owned child session navigation. Falls back to transcript modal. */
  onChildSessionNavigate?: (sessionId: string) => void | Promise<void>;
  /** Opens a project-locked Persona Builder from the current project conversation. */
  onBuildPersona?: () => void;
  renderComposer?: (
    props: IntegratedChatComposerRenderProps,
  ) => React.ReactNode;
  onUserMessageSent?: (payload: {
    content: string;
    result: SendAgentMessageResult;
    composerIntegrationReferences?: ComposerIntegrationReference[];
  }) => void | Promise<void>;
  onQuestionAnswered?: (
    question: AskUserQuestionPayload,
    response: AskUserQuestionResponse,
    result: SubmitQuestionAnswerResult,
  ) => void | Promise<void>;
}

export interface IntegratedChatComposerRenderProps {
  onSend: (
    message: string,
    options?: {
      folderReferences?: MessageFolderReference[];
      projectReferences?: ComposerProjectReference[];
      integrationReferences?: ComposerIntegrationReference[];
      artifactReferences?: ComposerArtifactReference[];
      excerptReferences?: ComposerExcerptReference[];
      capabilityIntent?: CapabilityIntent | null;
      teamIntent?: TeamIntent | null;
      teamMessageTarget?: TeamMessageTarget | null;
    },
  ) => Promise<void>;
  onStop: () => Promise<void>;
  agentStatus: AgentStatus;
  isSending: boolean;
  hasQueuedMessages: boolean;
  onEditLastQueued: () => void;
  isReadOnly: boolean;
  placeholder: string;
  autoFocus: boolean;
  enableAttachments: boolean;
  attachments: PendingChatAttachment[];
  onFilesSelected: (files: File[]) => Promise<PendingChatAttachment[]>;
  onRemoveAttachment: (id: string) => Promise<void>;
  attachmentsUploading: boolean;
  questionMode?: QuestionMode;
  value?: string;
  onChange?: (value: string) => void;
  /** Model in use for this chat context, when known. Read-only signal. */
  effectiveModel?: { id: string; label: string } | undefined;
  /** Provider harness label (e.g. "claude", "codex") for this chat context. */
  providerHarness?: string | null | undefined;
  /** Conversation persona confirmation for host-owned composer surfaces. */
  personaControl?: React.ReactNode;
  /** Native runtime-picker persona field for Agent composer surfaces. */
  persona?: ComposerRuntimePersonaField;
}

export function IntegratedChatPanel({
  projectId,
  contextTypeOverride,
  contextIdOverride,
  ideationSessionId,
  emptyState,
  showHelperTextAlways = false,
  inputContainerClassName,
  headerContent,
  headerSubContent,
  hideHeaderSessionControls = false,
  hideSessionToolbar = false,
  surfaceBackground,
  contentWidthClassName,
  additionalQuestionSessionIds,
  planApprovalAction,
  onClose,
  autoFocusInput = true,
  isVisible = true,
  toolbarBackAction,
  conversationIdOverride,
  selectedTaskIdOverride,
  taskRuntimeContextTypeOverride,
  storeContextKeyOverride,
  agentProcessContextIdOverride,
  sendOptions,
  onChildSessionNavigate,
  onBuildPersona,
  renderComposer,
  onUserMessageSent,
  onQuestionAnswered,
}: IntegratedChatPanelProps) {
  const { chromeRef, containerRef, registerTranscriptSpacer } =
    useChatBottomInset();
  const bus = useEventBus();
  const queryClient = useQueryClient();
  const { data: featureFlags } = useFeatureFlags();
  const openModal = useUiStore((s) => s.openModal);
  const switchConversationPersona = useSwitchConversationPersona();
  const { data: personas = [] } = usePersonas();
  const {
    confirm: confirmPersonaChange,
    confirmationDialogProps: personaConfirmationDialogProps,
    ConfirmationDialog: PersonaConfirmationDialog,
  } = useConfirmation();
  const pollStartRef = useRef<number | null>(null);
  const personaRetryAttemptRef = useRef<PersonaRetryAttempt | null>(null);
  const [personaUnavailableError, setPersonaUnavailableError] = useState<
    string | null
  >(null);
  const [isRetryingPersonaSend, setIsRetryingPersonaSend] = useState(false);
  const handlePersonaUnavailable = useCallback((message: string) => {
    setPersonaUnavailableError(
      message.slice(PERSONA_UNAVAILABLE_PREFIX.length).replace(/\]$/, ""),
    );
  }, []);
  const [
    transcriptPaintCoverConversationId,
    setTranscriptPaintCoverConversationId,
  ] = useState<string | null>(null);
  const [childSessionModalId, setChildSessionModalId] = useState<string | null>(
    null,
  );
  const ideationSessionsById = useIdeationStore((s) => s.sessions);
  const selectedTaskId = selectedTaskIdOverride ?? null;
  // History state from store - shared with Agents task detail for time-travel feature
  const taskHistoryState = useUiStore((s) => s.taskHistoryState);
  const isHistoryMode = !!taskHistoryState;
  const hasHistoryConversation = !!taskHistoryState?.conversationId;
  const historyConversationOverride = isHistoryMode
    ? (taskHistoryState?.conversationId ?? null)
    : conversationIdOverride;

  // Get task data from React Query (useTasks) which has full task data
  const { data: tasks = EMPTY_TASKS } = useTasks(projectId ?? "", {
    enabled: Boolean(projectId && selectedTaskId),
  });

  // Read from Zustand store (event-updated, sync) — same pattern as Agents task detail
  const taskFromStore = useTaskStore((state) =>
    selectedTaskId ? state.tasks[selectedTaskId] : undefined,
  );

  // Find from list query
  const taskFromList = selectedTaskId
    ? tasks.find((t) => t.id === selectedTaskId)
    : undefined;

  // Fallback: fetch the specific task by ID when not found in store or list
  const { data: taskFromDetail } = useQuery<Task, Error>({
    queryKey: taskKeys.detail(selectedTaskId ?? ""),
    queryFn: () => api.tasks.get(selectedTaskId!),
    enabled: Boolean(selectedTaskId) && !taskFromStore && !taskFromList,
  });

  const selectedTask: Task | undefined =
    taskFromStore ?? taskFromList ?? taskFromDetail;

  // Determine effective status - use historical status in history mode, otherwise current status
  const effectiveStatus =
    taskHistoryState?.status ?? selectedTask?.internalStatus;

  // Agent-status-aware overrides: keep mode active while agent is still running,
  // even if task status has already transitioned
  const executionKey = selectedTaskId
    ? buildStoreKey("task_execution", selectedTaskId)
    : "";
  const executionAgentRunning = useChatStore(
    selectIsAgentRunning(executionKey),
  );
  const reviewKey = selectedTaskId
    ? buildStoreKey("review", selectedTaskId)
    : "";
  const reviewAgentRunning = useChatStore(selectIsAgentRunning(reviewKey));
  const mergeKey = selectedTaskId ? buildStoreKey("merge", selectedTaskId) : "";
  const mergeAgentRunning = useChatStore(selectIsAgentRunning(mergeKey));
  const forcedTaskRuntimeContext = selectedTaskId
    ? taskHistoryState?.contextType ?? taskRuntimeContextTypeOverride
    : undefined;

  // Execution states: worker agent is running (only when NOT in ideation mode)
  // Agent-status override is gated on !taskHistoryState: in history mode, no agent
  // is running so the override is always false, but the explicit guard prevents
  // stale agentStatus entries from activating mode flags for historical contexts.
  const isExecutionMode =
    !ideationSessionId &&
    !!selectedTaskId &&
    (forcedTaskRuntimeContext === "task_execution" ||
      (!forcedTaskRuntimeContext &&
        ((effectiveStatus
          ? (EXECUTION_STATUSES as readonly string[]).includes(effectiveStatus)
          : false) ||
          (!taskHistoryState && executionAgentRunning))));

  // Review states: reviewer agent conversation (only when NOT in ideation mode)
  // Include 'approved' so historical view loads the reviewer's conversation
  const isReviewMode =
    !ideationSessionId &&
    !!selectedTaskId &&
    (forcedTaskRuntimeContext === "review" ||
      (!forcedTaskRuntimeContext &&
        ((effectiveStatus
          ? (ALL_REVIEW_STATUSES as readonly string[]).includes(
              effectiveStatus,
            ) || effectiveStatus === "approved"
          : false) ||
          (!taskHistoryState && reviewAgentRunning))));

  // Merge states: merger agent conversation (only when NOT in ideation mode)
  const isMergeMode =
    !ideationSessionId &&
    !!selectedTaskId &&
    (forcedTaskRuntimeContext === "merge" ||
      (!forcedTaskRuntimeContext &&
        ((effectiveStatus
          ? (MERGE_STATUSES as readonly string[]).includes(effectiveStatus)
          : false) ||
          (!taskHistoryState && mergeAgentRunning))));

  // Use extracted context management hook
  const {
    chatContext,
    storeContextKey,
    currentContextType,
    currentContextId,
    activeConversationId,
    streamingToolCalls,
    setStreamingToolCalls,
    streamingContentBlocks,
    setStreamingContentBlocks,
    streamingTasks,
    setStreamingTasks,
    isFinalizing,
    setIsFinalizing,
    autoSelectConversation,
    // overrideAgentRunId is available but we use taskHistoryState.timestamp for scroll positioning
  } = useChatPanelContext({
    projectId,
    contextTypeOverride,
    contextIdOverride,
    ideationSessionId,
    selectedTaskId: selectedTaskId ?? undefined,
    isExecutionMode,
    isReviewMode,
    isMergeMode,
    isHistoryMode,
    // Pass history mode overrides for conversation selection
    overrideConversationId: historyConversationOverride,
    storeContextKeyOverride,
    overrideAgentRunId: taskHistoryState?.agentRunId,
    isVisible,
  });
  usePersonaRunEvents(activeConversationId);
  const agentProcessContextId =
    agentProcessContextIdOverride ?? currentContextId;
  useQueuedMessagesHydration({
    contextType: currentContextType,
    contextId: agentProcessContextId,
    storeContextKey,
    enabled: !isHistoryMode,
  });
  const setActiveConversation = useChatStore((s) => s.setActiveConversation);

  // Refs for stable agent:run_started handler — prevent stale closure writes during context transitions.
  // useLayoutEffect keeps refs synchronised before any Tauri IPC events can arrive.
  const storeContextKeyRef = useRef(storeContextKey);
  const currentContextTypeRef = useRef(currentContextType);
  const currentContextIdRef = useRef(currentContextId);
  const isHistoryModeRef = useRef(isHistoryMode);
  useLayoutEffect(() => {
    storeContextKeyRef.current = storeContextKey;
    currentContextTypeRef.current = currentContextType;
    currentContextIdRef.current = currentContextId;
    isHistoryModeRef.current = isHistoryMode;
  }, [storeContextKey, currentContextType, currentContextId, isHistoryMode]);

  // Agent lifecycle events (useAgentEvents) are handled inside useChat — no duplicate subscription needed.

  // If a new run starts in this context, switch to its conversation (live mode only).
  // Reads context values from refs to avoid stale closure writes during teardown/resubscribe window.
  useEffect(() => {
    return bus.subscribe<{
      context_type: string;
      context_id: string;
      conversation_id: string;
    }>("agent:run_started", (payload) => {
      if (isHistoryModeRef.current) return;

      // Existing exact match
      if (
        payload.context_type === currentContextTypeRef.current &&
        payload.context_id === currentContextIdRef.current &&
        payload.conversation_id
      ) {
        setActiveConversation(
          storeContextKeyRef.current,
          payload.conversation_id,
        );
        return;
      }
      // Handle retry scenario: task context watching a new execution starting
      // When task is in failed/ready state, currentContextType is "task" but
      // the new execution emits "task_execution". Accept if task ID matches.
      // Dual-write: set on the panel's current slot (storeContextKey) so the
      // current panel immediately shows the conversation with no blank flash,
      // AND pre-populate the new execution slot so when the panel transitions
      // to task_execution context the conversation is already set.
      if (
        payload.context_type === "task_execution" &&
        currentContextTypeRef.current === "task" &&
        payload.context_id === currentContextIdRef.current &&
        payload.conversation_id
      ) {
        setActiveConversation(
          storeContextKeyRef.current,
          payload.conversation_id,
        );
        const executionKey = buildStoreKey(
          payload.context_type as ContextType,
          payload.context_id,
        );
        if (executionKey !== storeContextKeyRef.current) {
          setActiveConversation(executionKey, payload.conversation_id);
        }
      }
    });
  }, [bus, setActiveConversation]);

  // Subscribe to agent:conversation_created — invalidate conversation list query so new conversations appear immediately.
  useEffect(() => {
    return bus.subscribe<{
      context_id: string;
      context_type: string;
      conversation_id: string;
    }>("agent:conversation_created", (payload) => {
      if (payload.context_id !== currentContextId) return;
      void queryClient.invalidateQueries({
        queryKey: chatKeys.conversationList(
          payload.context_type as ContextType,
          payload.context_id,
        ),
      });
    });
  }, [bus, queryClient, currentContextId]);

  // Use context-aware selectors - unified queue works for all modes
  const queuedMessagesSelector = useMemo(
    () => selectQueuedMessages(storeContextKey),
    [storeContextKey],
  );
  const queuedMessages = useChatStore(queuedMessagesSelector);
  const agentStatusSelector = useMemo(
    () => selectAgentStatus(storeContextKey),
    [storeContextKey],
  );
  const agentStatus = useChatStore(agentStatusSelector);
  const activeAgentRunIdSelector = useMemo(
    () => selectActiveAgentRunId(storeContextKey),
    [storeContextKey],
  );
  const activeAgentRunId = useChatStore(activeAgentRunIdSelector);
  const activeAgentRunHarnessSelector = useMemo(
    () => selectActiveAgentRunHarness(storeContextKey),
    [storeContextKey],
  );
  const activeAgentRunHarness = useChatStore(activeAgentRunHarnessSelector);
  const agentActivityLabelSelector = useMemo(
    () => selectAgentActivityLabel(storeContextKey),
    [storeContextKey],
  );
  const agentActivityLabel = useChatStore(agentActivityLabelSelector);
  const isAgentRunning = agentStatus !== "idle"; // backward-compat boolean (agent process alive)
  const lastAgentEventTsSelector = useMemo(
    () => selectLastAgentEventTimestamp(storeContextKey),
    [storeContextKey],
  );
  const lastAgentEventTs = useChatStore(lastAgentEventTsSelector);
  const toolCallStartTimesSelector = useMemo(
    () => selectToolCallStartTimes(storeContextKey),
    [storeContextKey],
  );
  const toolCallStartTimes = useChatStore(toolCallStartTimesSelector);
  const isSendingSelector = useMemo(
    () => selectIsSending(storeContextKey),
    [storeContextKey],
  );
  const effectiveModelSelector = useMemo(
    () => selectEffectiveModel(storeContextKey),
    [storeContextKey],
  );
  const effectiveModel = useChatStore(effectiveModelSelector);

  // Timeout warning state — track dismissed bash tool call ID
  const [dismissedTimeoutCallId, setDismissedTimeoutCallId] = useState<
    string | null
  >(null);
  const activeBashCall = streamingToolCalls.find(
    (tc) => tc.name.toLowerCase() === "bash",
  );
  const bashStartTime = activeBashCall
    ? toolCallStartTimes[activeBashCall.id]
    : undefined;
  const effectiveTimeoutMs = 600_000;
  const showTimeoutWarning =
    activeBashCall !== undefined &&
    bashStartTime !== undefined &&
    activeBashCall.id !== dismissedTimeoutCallId;

  // Auto-reset dismissed ID when the dismissed call is no longer active
  useEffect(() => {
    if (
      dismissedTimeoutCallId &&
      !streamingToolCalls.find((tc) => tc.id === dismissedTimeoutCallId)
    ) {
      setDismissedTimeoutCallId(null);
    }
  }, [streamingToolCalls, dismissedTimeoutCallId]);
  const isSending = useChatStore(isSendingSelector);
  const setAgentRunning = useChatStore((s) => s.setAgentRunning);

  // For execution/review mode, fetch conversations directly with specific context type
  const regularChatData = useChat(chatContext, {
    isVisible,
    storeKey: storeContextKey,
    disableAutoSelect: true,
    skipActiveConversationQuery: true,
    ...(sendOptions !== undefined ? { sendOptions } : {}),
  });

  // Single dynamic query for all agent contexts (execution/review/merge)
  // When currentContextType changes, the query key changes and a fresh fetch fires
  const isAgentContext = isExecutionMode || isReviewMode || isMergeMode;

  const agentConversationsQuery = useQuery({
    queryKey: chatKeys.conversationList(
      currentContextType,
      selectedTaskId ?? "",
    ),
    queryFn: () =>
      chatApi.listConversations(
        currentContextType as ContextType,
        selectedTaskId ?? "",
      ),
    enabled: isAgentContext && !!selectedTaskId,
    staleTime: 0,
  });

  // Use agent query for agent contexts, regular chat data otherwise
  const conversations = isAgentContext
    ? agentConversationsQuery
    : regularChatData.conversations;

  // Poll every 3s (up to 60s) when visible, non-agent context, and no conversations yet.
  // Drives the auto-select chain: invalidateQueries → React Query refetch → conversationsData updates → auto-select re-fires.
  const POLL_INTERVAL_MS = 3000;
  const POLL_MAX_MS = 60_000;
  useEffect(() => {
    if (!isVisible || isAgentContext) {
      pollStartRef.current = null;
      return;
    }
    if ((conversations.data?.length ?? 0) > 0) {
      pollStartRef.current = null;
      return;
    }
    pollStartRef.current = Date.now();
    const id = setInterval(() => {
      if (
        pollStartRef.current !== null &&
        Date.now() - pollStartRef.current >= POLL_MAX_MS
      ) {
        clearInterval(id);
        pollStartRef.current = null;
        return;
      }
      void queryClient.invalidateQueries({
        queryKey: chatKeys.conversationList(
          currentContextType,
          currentContextId,
        ),
      });
    }, POLL_INTERVAL_MS);
    return () => {
      clearInterval(id);
    };
  }, [
    isVisible,
    isAgentContext,
    conversations.data,
    queryClient,
    currentContextType,
    currentContextId,
  ]);

  // Auto-select the most recent conversation in execution/review/merge modes
  // Extract stable primitives from TanStack Query result to avoid re-render on every query object change
  const conversationsData = conversations.data;
  const conversationsLoading = conversations.isLoading;
  useEffect(() => {
    autoSelectConversation({
      data: conversationsData,
      isLoading: conversationsLoading,
    });
  }, [
    autoSelectConversation,
    conversationsData,
    conversationsLoading,
    isVisible,
  ]);

  const {
    sendMessage,
    switchConversation: handleSelectConversation,
    createConversation: handleNewConversation,
  } = regularChatData;

  // Load active transcript windows through the shared tail-window query. The
  // backend returns each newest window oldest-to-newest; older pages prepend.
  const primaryConversationHistory = useConversationTimelineWindow(
    activeConversationId,
    {
      enabled: !!activeConversationId,
      pageSize: 40,
    },
  );
  const primaryTimelineHasMessages = transcriptWindowHasMessages(
    primaryConversationHistory.data,
  );
  const primaryLogicalConversationHistory = useConversationHistoryWindow(
    activeConversationId,
    {
      enabled:
        !!activeConversationId &&
        !primaryConversationHistory.isLoading &&
        !primaryTimelineHasMessages,
      pageSize: 40,
    },
  );
  const primaryLegacyHasMessages = transcriptWindowHasMessages(
    primaryLogicalConversationHistory.data,
  );
  const shouldUsePrimaryLogicalHistory =
    !primaryTimelineHasMessages &&
    !primaryConversationHistory.isLoading &&
    primaryLegacyHasMessages;
  const shouldUsePrimaryOptimisticFallback =
    !!activeConversationId && isOptimisticConversationId(activeConversationId);
  const shouldUsePrimaryRegularFallback =
    !primaryTimelineHasMessages &&
    !primaryConversationHistory.isLoading &&
    !primaryLegacyHasMessages;

  const primaryConversationData = shouldUsePrimaryLogicalHistory
    ? primaryLogicalConversationHistory.data
    : (primaryConversationHistory.data ??
      (shouldUsePrimaryOptimisticFallback || shouldUsePrimaryRegularFallback
        ? regularChatData.messages.data
        : undefined));
  const primaryTranscriptWindow = shouldUsePrimaryLogicalHistory
    ? primaryLogicalConversationHistory
    : primaryConversationHistory;
  const activeTranscriptConversationId = activeConversationId;
  const activeTranscriptRequiresLogicalHistory = shouldUsePrimaryLogicalHistory;

  useEffect(() => {
    if (
      !isVisible ||
      !activeTranscriptConversationId ||
      !activeTranscriptRequiresLogicalHistory
    ) {
      return;
    }
    void queryClient.invalidateQueries({
      queryKey: chatKeys.conversationHistory(activeTranscriptConversationId),
    });
  }, [
    isVisible,
    activeTranscriptConversationId,
    activeTranscriptRequiresLogicalHistory,
    queryClient,
  ]);

  const currentPrimaryConversationData =
    activeConversationId &&
    primaryConversationData &&
    (!primaryConversationData.conversation?.id ||
      primaryConversationData.conversation.id === activeConversationId)
      ? primaryConversationData
      : null;
  // Check if active conversation belongs to current context (needed by recovery effects below)
  const activeConversationContext = currentPrimaryConversationData?.conversation ??
    conversationsData?.find(
      (conversation) => conversation.id === activeConversationId,
    );
  const isConversationInCurrentContext = useMemo(
    () =>
      Boolean(
        currentPrimaryConversationData &&
        !currentPrimaryConversationData.conversation,
      ) ||
      ((activeConversationContext?.contextType === currentContextType ||
        (currentContextType === "task" &&
          activeConversationContext?.contextType === "task_execution")) &&
        activeConversationContext?.contextId === currentContextId),
    [
      currentPrimaryConversationData,
      activeConversationContext?.contextType,
      activeConversationContext?.contextId,
      currentContextType,
      currentContextId,
    ],
  );

  // Fetch agent run status for the active conversation
  const agentRunQuery = useQuery({
    queryKey: chatKeys.agentRun(activeConversationId ?? ""),
    queryFn: () =>
      activeConversationId
        ? chatApi.getAgentRunStatus(activeConversationId)
        : null,
    enabled: !!activeConversationId,
    staleTime: 5000,
  });
  const persistedStreamingContentBlocks = useMemo(
    () => projectPersistedStreamingContentBlocks(
      currentPrimaryConversationData?.messages ?? [],
      agentRunQuery.data?.id ?? undefined,
    ),
    [currentPrimaryConversationData?.messages, agentRunQuery.data?.id],
  );

  // Recovery and polling effects (extracted to hook)
  const { isStreamingHydrated } = useChatRecovery({
    activeConversationId,
    storeContextKey,
    currentContextType,
    currentContextId: agentProcessContextId,
    isHistoryMode,
    isAgentContext,
    isAgentRunning,
    isGenerating: agentStatus === "generating",
    isConversationInCurrentContext,
    agentRunStatus: agentRunQuery.data?.status ?? undefined,
    ...(agentRunQuery.data?.id != null ? { activeAgentRunId: agentRunQuery.data.id } : {}),
    isVisible,
    persistedStreamingContentBlocks,
    isTimelineHydrated: currentPrimaryConversationData != null,
    setStreamingTasks,
    setStreamingToolCalls,
    setStreamingContentBlocks,
    setAgentRunning,
    selectedTaskId: selectedTaskId ?? undefined,
    ideationSessionId,
    projectId,
    effectiveStatus,
  });

  // Track dismissed error banners by run ID
  const [dismissedErrorId, setDismissedErrorId] = useState<string | null>(null);
  const failedRun =
    agentRunQuery.data?.status === "failed" ? agentRunQuery.data : null;
  const showFailedBanner =
    failedRun && failedRun.errorMessage && failedRun.id !== dismissedErrorId;

  // Memoize failedRun prop to avoid creating a new object reference each render,
  // which would bust ChatMessageList's virtuosoComponents useMemo via the failedRun dep.
  const failedRunProp = useMemo(
    () =>
      showFailedBanner && failedRun
        ? { id: failedRun.id, errorMessage: failedRun.errorMessage! }
        : null,
    [showFailedBanner, failedRun],
  );

  const virtuosoRef = useRef<VirtuosoHandle>(null);
  const effectiveConversationId = activeConversationId;
  const composerDraftKey = effectiveConversationId
    ? `conversation:${effectiveConversationId}`
    : null;
  const composerDraft = useChatStore(selectComposerDraft(composerDraftKey));
  const setComposerDraftContent = useChatStore(
    (s) => s.setComposerDraftContent,
  );
  const clearComposerDraft = useChatStore((s) => s.clearComposerDraft);

  // File attachments - use effectiveConversationId for attachment association
  // Only enable attachments when there's an active conversation (not in history mode)
  const {
    attachments,
    uploadFiles,
    removeAttachment,
    clearAttachments,
    uploading,
  } = useChatAttachments(effectiveConversationId ?? "", {
    draftKey: composerDraftKey,
  });
  const activeConversationMeta = useMemo(() => {
    const queriedConversation = currentPrimaryConversationData?.conversation;

    if (queriedConversation) {
      return queriedConversation;
    }

    return (
      conversationsData?.find(
        (conversation) => conversation.id === effectiveConversationId,
      ) ?? null
    );
  }, [
    currentPrimaryConversationData?.conversation,
    conversationsData,
    effectiveConversationId,
  ]);
  const activeConversationListMeta = useMemo(
    () =>
      conversationsData?.find(
        (conversation) => conversation.id === effectiveConversationId,
      ) ?? null,
    [conversationsData, effectiveConversationId],
  );
  const activeRunDelivery = resolveChatInputDelivery(activeAgentRunHarness);
  const recoveryDelivery = resolveChatInputDelivery(
    activeConversationListMeta?.providerHarness,
  );
  const shouldHideQueuedMessages =
    (!isHistoryMode &&
      activeAgentRunId !== undefined &&
      isAgentRunning &&
      activeRunDelivery === "interactive") ||
    (!isHistoryMode &&
      activeAgentRunId === undefined &&
      activeAgentRunHarness === undefined &&
      agentRunQuery.data?.status === "running" &&
      recoveryDelivery === "interactive");
  const presentedQueuedMessages = shouldHideQueuedMessages
    ? queuedMessages.filter((message) => message.source === "backend")
    : queuedMessages;
  const hasPresentedQueuedMessages = presentedQueuedMessages.length > 0;
  const personaChipProjectName = useProjectStore(
    (state) => state.projects[currentContextId]?.name,
  );
  const personaAttributedRun = useMemo<PersonaAttributedRun | null>(() => {
    if (agentRunQuery.data) {
      return agentRunQuery.data;
    }
    if (
      isAgentRunning ||
      !activeConversationMeta?.lastRunPersonaRunId ||
      !activeConversationMeta.lastRunPersonaSlug ||
      activeConversationMeta.lastRunPersonaInjected == null
    ) {
      return null;
    }
    return {
      id: activeConversationMeta.lastRunPersonaRunId,
      personaId: activeConversationMeta.lastRunPersonaId ?? null,
      personaSlug: activeConversationMeta.lastRunPersonaSlug,
      personaVersion: activeConversationMeta.lastRunPersonaVersion ?? null,
      personaInjected: activeConversationMeta.lastRunPersonaInjected,
      personaSkippedReason:
        activeConversationMeta.lastRunPersonaSkippedReason ?? null,
    };
  }, [agentRunQuery.data, activeConversationMeta, isAgentRunning]);

  // Memoize messagesData to avoid dependency chain issues in useEffect hooks
  // No time-based filtering needed - we switch context types based on historical state
  const messagesData = useMemo(() => {
    return activeConversationId &&
      isConversationInCurrentContext &&
      currentPrimaryConversationData
      ? currentPrimaryConversationData.messages
      : [];
  }, [
    activeConversationId,
    isConversationInCurrentContext,
    currentPrimaryConversationData,
  ]);

  // Loading state: show skeleton when conversations list is loading OR active conversation is loading
  const isConversationsLoading = conversations.isLoading;
  const isActiveConversationLoading = activeConversationId
    ? (primaryConversationHistory.isLoading ||
        (shouldUsePrimaryLogicalHistory &&
          primaryLogicalConversationHistory.isLoading) ||
        (shouldUsePrimaryLogicalHistory && !primaryConversationData)) &&
      !primaryConversationData
    : false;
  const isLoading = isConversationsLoading || isActiveConversationLoading;
  const transcriptConversationId =
    effectiveConversationId ?? activeConversationId ?? null;

  useLayoutEffect(() => {
    setTranscriptPaintCoverConversationId(transcriptConversationId);
  }, [transcriptConversationId]);

  const handleTranscriptInitialPaintReady = useCallback(
    (conversationId: string) => {
      setTranscriptPaintCoverConversationId((current) =>
        current === conversationId ? null : current,
      );
    },
    [],
  );

  // Debug logging for history mode
  logger.debug("[IntegratedChatPanel] Context mode:", {
    isHistoryMode,
    effectiveStatus,
    isExecutionMode,
    isReviewMode,
    taskHistoryState,
  });

  const {
    handleSend: handleSendBase,
    handleEditLastQueued,
    handleDeleteQueuedMessage,
    handleSendQueuedMessageNow,
    handleEditQueuedMessage,
    handleStopAgent,
  } = useChatActions({
    contextType: currentContextType,
    contextId: currentContextId,
    queueContextId: agentProcessContextId,
    storeContextKey,
    selectedTaskId: selectedTaskId ?? undefined,
    ideationSessionId,
    sendMessage,
    activeConversationId: effectiveConversationId,
    sendOptions,
    messageCount: messagesData.length,
    onUserMessageSent,
    onPersonaUnavailable: handlePersonaUnavailable,
  });

  // Wrap handleSend to include attachment IDs and clear attachments after send.
  // Auto-scroll on new user messages is handled by ChatMessageList (see its
  // "new user message → scrollToBottom" effect).
  const handleSend = useCallback(
    async (
      message: string,
      options?: {
        folderReferences?: MessageFolderReference[];
        projectReferences?: ComposerProjectReference[];
        integrationReferences?: ComposerIntegrationReference[];
        artifactReferences?: ComposerArtifactReference[];
        excerptReferences?: ComposerExcerptReference[];
        capabilityIntent?: CapabilityIntent | null;
        teamIntent?: TeamIntent | null;
        teamMessageTarget?: TeamMessageTarget | null;
      },
    ) => {
      personaRetryAttemptRef.current = { message, options };
      const attachmentIds = attachments.map((a) => a.id);
      logger.debug("[ChatScroll] handleSend firing", {
        hasAttachments: attachmentIds.length > 0,
      });
      await handleSendBase(
        message,
        attachmentIds.length > 0 ? attachmentIds : undefined,
        options,
      );
      if (composerDraftKey) {
        clearComposerDraft(composerDraftKey);
      } else if (attachmentIds.length > 0) {
        clearAttachments();
      }
    },
    [
      attachments,
      clearAttachments,
      clearComposerDraft,
      composerDraftKey,
      handleSendBase,
    ],
  );

  // Wrapper for handleEditLastQueued that uses the same queue projection as the UI.
  const handleEditLastQueuedWrapper = () => {
    handleEditLastQueued(presentedQueuedMessages);
  };

  const handleRemovePersonaAndRetry = useCallback(async () => {
    const retryAttempt = personaRetryAttemptRef.current;
    if (!effectiveConversationId || !retryAttempt || isRetryingPersonaSend) {
      return;
    }

    setIsRetryingPersonaSend(true);
    try {
      await switchConversationPersona.mutateAsync({
        conversationId: effectiveConversationId,
        personaId: null,
      });
      setPersonaUnavailableError(null);
      await handleSend(retryAttempt.message, retryAttempt.options);
    } catch (error) {
      setPersonaUnavailableError(
        extractErrorMessage(error, "Could not remove the unavailable persona."),
      );
    } finally {
      setIsRetryingPersonaSend(false);
    }
  }, [
    effectiveConversationId,
    handleSend,
    isRetryingPersonaSend,
    switchConversationPersona,
  ]);

  // Handle stopping agent - clear streaming state
  const handleStopAgentWrapper = useCallback(async () => {
    await handleStopAgent();
    setStreamingToolCalls((prev) => (prev.length === 0 ? prev : []));
    setStreamingContentBlocks((prev) => (prev.length === 0 ? prev : []));
    setStreamingTasks((prev) => (prev.size === 0 ? prev : new Map()));
  }, [
    handleStopAgent,
    setStreamingToolCalls,
    setStreamingContentBlocks,
    setStreamingTasks,
  ]);

  useChatEvents({
    activeConversationId: effectiveConversationId,
    activeAgentRunId:
      activeAgentRunId ??
      (agentRunQuery.data?.status === "running" ? agentRunQuery.data.id : null),
    contextId: currentContextId,
    contextType: currentContextType,
    streamingToolCalls,
    streamingContentBlocks,
    streamingTasks,
    setStreamingToolCalls,
    setStreamingContentBlocks,
    setStreamingTasks,
    setIsFinalizing,
    storeKey: storeContextKey,
  });

  const bridgedQuestionSessionIds = useMemo(() => {
    const unique: string[] = [];
    for (const id of additionalQuestionSessionIds ?? []) {
      if (id && id !== currentContextId && !unique.includes(id)) {
        unique.push(id);
      }
    }
    return unique.slice(0, 2);
  }, [additionalQuestionSessionIds, currentContextId]);

  // Ask-user-question hooks keep a fixed order so the conversation and planning
  // bridges can appear or disappear without changing the hook call count.
  const primaryQuestionState = useAskUserQuestion(currentContextId);
  const conversationQuestionState = useAskUserQuestion(
    bridgedQuestionSessionIds[0],
  );
  const planningQuestionState = useAskUserQuestion(
    bridgedQuestionSessionIds[1],
  );
  const questionCandidates = [
    primaryQuestionState,
    conversationQuestionState,
    planningQuestionState,
  ];
  const questionState =
    questionCandidates.find((state) => state.activeQuestion) ??
    questionCandidates.find((state) => state.answeredQuestion) ??
    primaryQuestionState;
  const {
    activeQuestion,
    answeredQuestion,
    submitAnswer,
    dismissQuestion,
    clearAnswered,
    isLoading: isSubmittingAnswer,
  } = questionState;
  const handleSubmitQuestionAnswer = useCallback(
    async (
      response: AskUserQuestionResponse,
    ): Promise<SubmitQuestionAnswerResult> => {
      const question = activeQuestion ?? null;
      const result = await submitAnswer(response);
      if (question && result.success) {
        await onQuestionAnswered?.(question, response, result);
      }
      return result;
    },
    [activeQuestion, onQuestionAnswered, submitAnswer],
  );

  // Question UI state — chip selection, input sync, question-aware send
  const {
    selectedOptions,
    questionInputValue,
    setQuestionInputValue,
    handleChipClick,
    handleMatchedOptions,
    handleQuestionSend,
    handleQuestionSkip,
    handleQuestionOptionSubmit,
  } = useQuestionInput({
    activeQuestion: activeQuestion ?? null,
    submitAnswer: handleSubmitQuestionAnswer,
    handleSend,
  });
  const automationProposalApplyIndex =
    automationProposalApplyOptionIndex(activeQuestion);
  const questionBannerAction =
    automationProposalApplyIndex >= 0 && activeQuestion
      ? {
          label:
            activeQuestion.options[automationProposalApplyIndex]?.label ??
            "Update automation",
          pendingLabel: "Applying...",
          onClick: () => {
            void handleQuestionOptionSubmit(automationProposalApplyIndex);
          },
          disabled: isSubmittingAnswer,
          isPending: isSubmittingAnswer,
        }
      : planApprovalAction;

  // Handler for opening a child ideation run without leaving the parent chat.
  const handleNavigateToChildSession = useCallback(
    async (childSessionId: string) => {
      if (onChildSessionNavigate) {
        await onChildSessionNavigate(childSessionId);
        return;
      }
      setChildSessionModalId(childSessionId);
    },
    [onChildSessionNavigate],
  );

  // Hydrate effectiveModel from HTTP session data for inactive ideation sessions.
  // This covers the case where the user opens a past session that was never live
  // in the current app session — the store will be empty but the DB has the model.
  useEffect(() => {
    if (!ideationSessionId) return;
    const session = ideationSessionsById[ideationSessionId];
    const lastEffectiveModel = session?.lastEffectiveModel;
    if (!lastEffectiveModel) return;
    const model = {
      id: lastEffectiveModel,
      label: getModelLabel(lastEffectiveModel),
    };
    useChatStore.getState().setEffectiveModel(storeContextKey, model);
  }, [ideationSessionId, ideationSessionsById, storeContextKey]);

  // Backfill effectiveModel from agentRunQuery for execution/review/merge contexts on reopen/refresh.
  // Guard: skip if live agent:run_started already populated the store, or if modelId is null.
  const agentRunModelId = agentRunQuery.data?.modelId ?? null;
  const agentRunModelLabel = agentRunQuery.data?.modelLabel ?? null;
  useEffect(() => {
    if (!agentRunModelId) return;
    if (useChatStore.getState().effectiveModel[storeContextKey]) return;
    useChatStore.getState().setEffectiveModel(storeContextKey, {
      id: agentRunModelId,
      label: agentRunModelLabel ?? getModelLabel(agentRunModelId),
    });
  }, [storeContextKey, agentRunModelId, agentRunModelLabel]);

  // Final fallback: derive runtime context from the latest non-user message.
  // Covers child chats (ideation/verification opened from Agents) where the
  // ideation store hasn't loaded the session and there's no agent run query.
  // Roles vary ("assistant", "orchestrator", "agent", etc.), so anything
  // non-user that carries the field is fair game.
  const latestRuntimeFromMessages = useMemo(() => {
    let modelId: string | null = null;
    let providerHarness: string | null = null;
    for (let i = messagesData.length - 1; i >= 0; i -= 1) {
      const msg = messagesData[i];
      if (!msg) continue;
      if (msg.role === "user") continue;
      if (!modelId && msg.effectiveModelId) modelId = msg.effectiveModelId;
      if (!providerHarness && msg.providerHarness)
        providerHarness = msg.providerHarness;
      if (modelId && providerHarness) break;
    }
    return { modelId, providerHarness };
  }, [messagesData]);
  useEffect(() => {
    const { modelId } = latestRuntimeFromMessages;
    if (!modelId) return;
    if (useChatStore.getState().effectiveModel[storeContextKey]) return;
    useChatStore.getState().setEffectiveModel(storeContextKey, {
      id: modelId,
      label: getModelLabel(modelId),
    });
  }, [storeContextKey, latestRuntimeFromMessages]);
  const fallbackProviderHarness = latestRuntimeFromMessages.providerHarness;

  // Handle Escape key to close panel
  useEffect(() => {
    if (!onClose) return;

    const handleKeyDown = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        onClose();
      }
    };

    document.addEventListener("keydown", handleKeyDown);
    return () => document.removeEventListener("keydown", handleKeyDown);
  }, [onClose]);

  // Sort messages by createdAt always. Secondary sort by id provides stable
  // tiebreaking when timestamps are equal (e.g. optimistic + DB messages share ms).
  const sortedMessages = useMemo(() => {
    return (
      [...messagesData]
        .filter(isVisibleChatMessage)
        .sort((a, b) => {
          if (a.timelineSequence != null && b.timelineSequence != null) {
            return a.timelineSequence - b.timelineSequence;
          }
          const timeDiff =
            new Date(a.createdAt).getTime() - new Date(b.createdAt).getTime();
          if (timeDiff !== 0) return timeDiff;
          return a.id < b.id ? -1 : a.id > b.id ? 1 : 0;
        })
    );
  }, [messagesData]);
  const hasPersistedStreamingTimelineItems = useMemo(
    () =>
      sortedMessages.some((message) => message.timelineStatus === "streaming"),
    [sortedMessages],
  );
  const hasClientLiveStreamingState =
    streamingToolCalls.length > 0 ||
    (streamingContentBlocks?.length ?? 0) > 0 ||
    streamingTasks.size > 0;
  const supplementalStreamingContentBlocks = useMemo(
    () => renderTranscriptBlocks(applyTranscriptInput(
      applyTranscriptInput(createLiveTranscriptState(), {
        kind: "persisted", runId: null, blocks: persistedStreamingContentBlocks,
      }),
      { kind: "live", runId: null, blocks: streamingContentBlocks ?? [] },
    )),
    [persistedStreamingContentBlocks, streamingContentBlocks],
  );
  const persistedStreamingToolIds = useMemo(
    () => new Set(persistedStreamingContentBlocks.flatMap((block) =>
      block.type === "tool_use" ? [block.toolCall.id] : []
    )),
    [persistedStreamingContentBlocks],
  );
  const supplementalStreamingToolCalls = useMemo(
    () => streamingToolCalls.filter((toolCall) => !persistedStreamingToolIds.has(toolCall.id)),
    [persistedStreamingToolIds, streamingToolCalls],
  );
  const shouldUsePersistedStreamingTimelineItems =
    hasPersistedStreamingTimelineItems &&
    (!hasClientLiveStreamingState || !isStreamingHydrated);
  const statsFallbackMessages = useMemo(
    () =>
      effectiveConversationId
        ? getCachedConversationMessages(queryClient, effectiveConversationId)
        : sortedMessages,
    [effectiveConversationId, queryClient, sortedMessages],
  );

  // Status badge helpers - disabled in history mode (no live agent)
  // isAgentActive: only true when actively generating (not waiting_for_input)
  const isAgentActive =
    !isHistoryMode && (isSending || agentStatus === "generating");
  const agentType: AgentType = isHistoryMode
    ? "idle"
    : isExecutionMode
      ? AGENT_WORKER
      : isReviewMode
        ? AGENT_REVIEWER
        : isMergeMode
          ? AGENT_MERGER
          : isSending || agentStatus === "generating"
            ? "agent"
            : "idle";
  const showPersonaChip =
    currentContextType === "project" &&
    featureFlags.agentPersonas === true &&
    activeConversationMeta?.agentMode !== "persona_builder" &&
    effectiveConversationId !== null;
  const activePersonaOptions = useMemo(
    () => [
      {
        id: "__no_persona__",
        label: "No persona",
        description: "Use the conversation's normal agent instructions.",
      },
      ...personas
        .filter(
          (persona) =>
            persona.status === "active" &&
            (persona.projectId === null || persona.projectId === projectId),
        )
        .map((persona) => ({
          id: persona.id,
          label: persona.name,
          description:
            persona.projectId === null ? "Global persona" : "Project persona",
        })),
    ],
    [personas, projectId],
  );
  const selectComposerPersona = useCallback(
    async (nextValue: string) => {
      if (!effectiveConversationId || switchConversationPersona.isPending) return;
      const nextPersonaId = nextValue === "__no_persona__" ? null : nextValue;
      if (nextPersonaId === (activeConversationMeta?.personaId ?? null)) return;
      if (isAgentRunning) {
        const confirmed = await confirmPersonaChange({
          title: "Change persona?",
          description:
            "Changing the persona stops the current run. Conversation history is preserved and the next message resumes the same session.",
          confirmText: "Change persona",
        });
        if (!confirmed) return;
      }
      try {
        await switchConversationPersona.mutateAsync({
          conversationId: effectiveConversationId,
          personaId: nextPersonaId,
        });
      } catch (error) {
        toast.error(
          extractErrorMessage(error, "Could not change the conversation persona."),
        );
      }
    },
    [
      activeConversationMeta?.personaId,
      confirmPersonaChange,
      effectiveConversationId,
      isAgentRunning,
      switchConversationPersona,
    ],
  );
  const composerPersona: ComposerRuntimePersonaField | undefined =
    showPersonaChip
      ? {
          value: activeConversationMeta?.personaId ?? "__no_persona__",
          onValueChange: selectComposerPersona,
          options: activePersonaOptions,
          disabled: switchConversationPersona.isPending,
          testId: "agent-composer-persona",
          footerAction: (
            <button
              type="button"
              className="w-full rounded px-2 py-1.5 text-left text-xs font-medium text-[var(--accent-primary)] hover:bg-[var(--bg-hover)]"
              onClick={() => openModal("settings", { section: "personas" })}
            >
              Manage personas
            </button>
          ),
        }
      : undefined;
  const personaChip =
    showPersonaChip && effectiveConversationId ? (
      <PersonaChip
        conversationId={effectiveConversationId}
        projectId={currentContextId}
        projectName={personaChipProjectName ?? "Project"}
        personaId={activeConversationMeta?.personaId}
        isAgentRunning={isAgentRunning}
        {...(onBuildPersona ? { onBuildPersona } : {})}
        lastRunPersonaId={
          personaAttributedRun?.personaId ??
          activeConversationMeta?.lastRunPersonaId ??
          null
        }
        lastRunPersonaSlug={personaAttributedRun?.personaSlug ?? null}
        lastRunPersonaVersion={personaAttributedRun?.personaVersion ?? null}
        lastRunPersonaInjected={
          personaAttributedRun?.personaInjected ?? null
        }
        lastRunPersonaSkippedReason={
          personaAttributedRun?.personaSkippedReason ?? null
        }
      />
    ) : undefined;

  // Empty state: only show when we KNOW there are no messages (not while loading)
  // Also don't show empty if conversations are loading - we might auto-select one
  const hasLiveTranscriptState =
    hasClientLiveStreamingState ||
    isSending ||
    agentStatus === "generating" ||
    isFinalizing;
  const hasNoConversations =
    !isConversationsLoading &&
    !activeConversationId &&
    (conversations.data?.length ?? 0) === 0;
  const hasEmptyConversation =
    !isLoading &&
    Boolean(activeConversationId) &&
    sortedMessages.length === 0 &&
    !hasLiveTranscriptState;
  const isEmpty = hasNoConversations || hasEmptyConversation;

  // Recency guard: suppress PreviousRunBanner if the agent was active within the last 10s.
  // Aligned with agentRunQuery.staleTime (10s) to avoid banner flash during run_completed transition.
  const [isRecentlyActive, setIsRecentlyActive] = useState(false);
  useEffect(() => {
    if (lastAgentEventTs <= 0) {
      setIsRecentlyActive(false);
      return;
    }
    const elapsed = Date.now() - lastAgentEventTs;
    if (elapsed >= 10_000) {
      setIsRecentlyActive(false);
      return;
    }
    setIsRecentlyActive(true);
    const timer = setTimeout(
      () => setIsRecentlyActive(false),
      10_000 - elapsed,
    );
    return () => clearTimeout(timer);
  }, [lastAgentEventTs]);
  const conversationContentShellClassName = cn(
    "w-full",
    contentWidthClassName ? ["mx-auto", contentWidthClassName] : undefined,
  );
  const transcriptTopInsetClassName = undefined;

  return (
    <>
      <style>{animationStyles}</style>
      <RecoveryPromptDialog
        surface="chat"
        taskId={selectedTaskId ?? undefined}
      />
      {/* Outer container — fills to layout edges. Phase 1 region border
         on [data-testid="integrated-chat-panel"] separates chat from
         main content, so no floating-card chrome needed here. */}
      <div
        data-testid="integrated-chat-panel"
        className="h-full flex flex-col overflow-hidden"
      >
        {/* Inner surface — flat with blur, no perimeter or radius. */}
        <div
          className="relative flex-1 flex flex-col overflow-hidden"
          style={
            surfaceBackground === "transparent"
              ? { background: "transparent" }
              : {
                  background:
                    surfaceBackground ?? withAlpha("var(--bg-surface)", 92),
                  backdropFilter: "blur(20px) saturate(180%)",
                  WebkitBackdropFilter: "blur(20px) saturate(180%)",
                }
          }
        >
          {/* Header — theme-agnostic subtle tint keeps the embedded chrome
             aligned across themes.
             Previous bg-base@50 produced visible seam on Dark (lum=25 vs
             body lum=30) and collapsed to pure black on HC. Using a tint
             derived from text-primary keeps a consistent 2% brighter band
             across all three themes. */}
          <div
            data-testid="integrated-chat-header"
            className="flex items-center justify-between h-11 px-3 shrink-0 gap-3"
            style={{
              backgroundColor:
                "color-mix(in srgb, var(--text-primary) 2%, transparent)",
              borderBottom: "1px solid var(--overlay-faint)",
            }}
          >
            {headerContent ?? (
              <ContextIndicator
                context={chatContext}
                isExecutionMode={isExecutionMode}
                isReviewMode={isReviewMode}
                isMergeMode={isMergeMode}
              />
            )}

            {/* Provider-context chips rendered inline next to the
                ConversationSelector — per 2026-04-19 feedback, the CODEX
                badge / model / effort / stats popover live in the header
                row, not in a separate toolbar strip below. */}
            {!hideHeaderSessionControls && (
              <div className="ml-auto flex items-center gap-2 min-w-0">
                <ChatSessionChips
                  contextType={currentContextType as ContextType}
                  contextId={ideationSessionId || selectedTaskId || null}
                  isAgentActive={isAgentActive}
                  conversationId={effectiveConversationId}
                  providerHarness={
                    activeConversationMeta?.providerHarness ?? null
                  }
                  providerSessionId={
                    activeConversationMeta?.providerSessionId ?? null
                  }
                  upstreamProvider={
                    activeConversationMeta?.upstreamProvider ?? null
                  }
                  providerProfile={
                    activeConversationMeta?.providerProfile ?? null
                  }
                  fallbackConversation={activeConversationMeta}
                  fallbackMessages={statsFallbackMessages}
                  {...(effectiveModel !== undefined
                    ? { modelDisplay: effectiveModel }
                    : {})}
                />

                {/* Conversation Selector */}
                <ConversationSelector
                  contextType={
                    ideationSessionId
                      ? "ideation"
                      : isMergeMode
                        ? "merge"
                        : isExecutionMode
                          ? "task_execution"
                          : isReviewMode
                            ? "review"
                            : selectedTaskId
                              ? "task"
                              : "project"
                  }
                  contextId={currentContextId}
                  conversations={conversations.data ?? []}
                  activeConversationId={activeConversationId}
                  onSelectConversation={handleSelectConversation}
                  onNewConversation={handleNewConversation}
                  isLoading={conversations.isLoading}
                />
              </div>
            )}
          </div>
          {headerSubContent ?? null}

          {/* Session Toolbar — houses StatusActivityBadge + optional back
              action. Provider-context chips are now rendered inline in
              the integrated-chat-header (above), so suppress them here
              via `hideProviderContext` to avoid duplication. */}
          {!hideSessionToolbar && (
            <ChatSessionToolbar
              isAgentActive={isAgentActive}
              agentType={agentType}
              contextType={currentContextType as ContextType}
              contextId={ideationSessionId || selectedTaskId || null}
              agentStatus={isHistoryMode ? "idle" : agentStatus}
              storeKey={storeContextKey}
              conversationId={effectiveConversationId}
              providerHarness={activeConversationMeta?.providerHarness ?? null}
              providerSessionId={
                activeConversationMeta?.providerSessionId ?? null
              }
              upstreamProvider={
                activeConversationMeta?.upstreamProvider ?? null
              }
              providerProfile={activeConversationMeta?.providerProfile ?? null}
              fallbackConversation={activeConversationMeta}
              fallbackMessages={statsFallbackMessages}
              hideProviderContext
              {...(toolbarBackAction !== undefined
                ? { backAction: toolbarBackAction }
                : {})}
              {...(effectiveModel !== undefined
                ? { modelDisplay: effectiveModel }
                : {})}
              {...(!renderComposer && personaChip !== undefined
                ? {
                    personaChip,
                  }
                : {})}
            />
          )}

          {/* Timeout Warning Banner — shown when bash tool call approaches timeout */}
          {showTimeoutWarning && (
            <TimeoutWarning
              toolCallStartTime={bashStartTime!}
              effectiveTimeoutMs={effectiveTimeoutMs}
              onDismiss={() => setDismissedTimeoutCallId(activeBashCall!.id)}
            />
          )}

          {/* Messages Area — wrapped with navigation context for child session widgets */}
          <ChildSessionNavigationContext.Provider
            value={handleNavigateToChildSession}
          >
            <div
              ref={containerRef}
              className="relative flex-1 min-h-0 flex flex-col"
            >
            {isLoading ? (
              <div
                className="flex-1 flex items-center justify-center"
                data-testid="integrated-chat-messages"
                style={{ paddingBottom: "var(--chat-bottom-inset, 0px)" }}
              >
                <LoadingState />
              </div>
            ) : isEmpty ? (
              <div
                className="flex-1 flex items-center justify-center"
                data-testid="integrated-chat-messages"
                style={{ paddingBottom: "var(--chat-bottom-inset, 0px)" }}
              >
                {emptyState ??
                  (isHistoryMode && !hasHistoryConversation ? (
                    <HistoryEmptyState />
                  ) : (
                    <EmptyState />
                  ))}
              </div>
            ) : (
              <ChatMessageList
                ref={virtuosoRef}
                messages={sortedMessages}
                conversationId={effectiveConversationId}
                initialPaintCoverKey={
                  transcriptPaintCoverConversationId ===
                  transcriptConversationId
                    ? transcriptPaintCoverConversationId
                    : null
                }
                onInitialPaintReady={handleTranscriptInitialPaintReady}
                firstItemIndex={primaryTranscriptWindow.loadedStartIndex}
                failedRun={failedRunProp}
                onDismissFailedRun={setDismissedErrorId}
                isSending={isSending}
                isAgentRunning={agentStatus === "generating"}
                typingIndicatorLabel={agentActivityLabel}
                streamingToolCalls={
                  shouldUsePersistedStreamingTimelineItems
                    ? []
                    : supplementalStreamingToolCalls
                }
                streamingTasks={
                  shouldUsePersistedStreamingTimelineItems
                    ? new Map()
                    : streamingTasks
                }
                streamingContentBlocks={
                  shouldUsePersistedStreamingTimelineItems
                    ? []
                    : supplementalStreamingContentBlocks
                }
                scrollToTimestamp={
                  isHistoryMode ? taskHistoryState?.timestamp : null
                }
                isFinalizing={isFinalizing}
                providerHarness={
                  activeConversationMeta?.providerHarness ?? null
                }
                providerSessionId={
                  activeConversationMeta?.providerSessionId ?? null
                }
                agentRun={personaAttributedRun}
                personaRuns={activeConversationMeta?.personaRuns ?? []}
                agentPersonasEnabled={featureFlags.agentPersonas === true}
                contentWidthClassName={contentWidthClassName}
                topInsetClassName={transcriptTopInsetClassName}
                hasOlderMessages={primaryTranscriptWindow.hasOlderMessages}
                isFetchingOlderMessages={primaryTranscriptWindow.isFetchingOlderMessages}
                onLoadOlderMessages={primaryTranscriptWindow.fetchOlderMessages}
                registerBottomSpacer={registerTranscriptSpacer}
              />
            )}

            <div
              ref={chromeRef}
              data-testid="chat-below-transcript-chrome"
              className="absolute inset-x-0 bottom-0 z-20 flex flex-col overflow-y-auto"
              style={{ maxHeight: "min(60%, 100%)" }}
            >
              <div
                className="absolute inset-0 -z-10"
                aria-hidden
                style={
                  surfaceBackground === "transparent"
                    ? {
                        backgroundColor: "var(--bg-base)",
                      }
                    : {
                        backgroundColor:
                          "color-mix(in srgb, var(--bg-surface) 92%, transparent)",
                        backdropFilter: "blur(20px) saturate(180%)",
                        WebkitBackdropFilter: "blur(20px) saturate(180%)",
                      }
                }
              />
              {/* Child Session Notification - shows when follow-up is created (ideation mode only) */}
              {ideationSessionId && !isHistoryMode && (
                <div className="px-3">
                  <div className={conversationContentShellClassName}>
                    <ChildSessionNotification sessionId={ideationSessionId} />
                  </div>
                </div>
              )}
              <ChildSessionTranscriptModal
                sessionId={childSessionModalId}
                open={!!childSessionModalId}
                onOpenChange={(isOpen) => {
                  if (!isOpen) {
                    setChildSessionModalId(null);
                  }
                }}
              />
              {/* Previous Run Banner - shown when viewing stale agent conversation */}
              {isAgentContext &&
                !isHistoryMode &&
                agentStatus === "idle" &&
                agentRunQuery.data?.status !== "running" &&
                !isSending &&
                sortedMessages.length > 0 &&
                !isRecentlyActive && (
                  <div className="px-3">
                    <div className={conversationContentShellClassName}>
                      <PreviousRunBanner
                        agentRunStatus={agentRunQuery.data?.status ?? null}
                        contextType={
                          isMergeMode
                            ? "merge"
                            : isReviewMode
                              ? "review"
                              : "execution"
                        }
                      />
                    </div>
                  </div>
                )}

              {/* Input Area — same theme-agnostic tint as header for symmetric
             chrome rhythm. Previous bg-base@50 collapsed on HC and shaded
             darker than body on Dark, producing a three-tier sandwich. */}
              <div
                data-testid="chat-input-container"
                className={inputContainerClassName ?? ""}
                style={
                  inputContainerClassName
                    ? undefined
                    : {
                        backgroundColor:
                          "color-mix(in srgb, var(--text-primary) 2%, transparent)",
                      }
                }
              >
                <div
                  data-testid="integrated-chat-input-shell"
                  className={conversationContentShellClassName}
                >
                  {/* Queued Messages - unified queue with context-aware keys */}
                  {hasPresentedQueuedMessages && (
                    <div className="p-3 pb-0">
                      <QueuedMessageList
                        messages={presentedQueuedMessages}
                        onEdit={handleEditQueuedMessage}
                        onDelete={handleDeleteQueuedMessage}
                        onSendNow={handleSendQueuedMessageNow}
                      />
                    </div>
                  )}

                  {/* Question Input Banner - renders above ChatInput when question is active */}
                  {(activeQuestion || answeredQuestion) && (
                    <QuestionInputBanner
                      key={activeQuestion?.requestId ?? "answered"}
                      question={activeQuestion ?? null}
                      selectedIndices={selectedOptions}
                      onChipClick={handleChipClick}
                      onSkip={handleQuestionSkip}
                      onDismiss={dismissQuestion}
                      answeredValue={answeredQuestion}
                      onDismissAnswered={clearAnswered}
                      {...(questionBannerAction !== undefined && {
                        planApprovalAction: questionBannerAction,
                      })}
                    />
                  )}

                  {personaUnavailableError && (
                    <div className="px-3 pt-3">
                      <PersonaUnavailableNotice
                        message={personaUnavailableError}
                        onRemoveAndRetry={() => {
                          void handleRemovePersonaAndRetry();
                        }}
                        onOpenPersonas={() =>
                          openModal("settings", { section: "personas" })
                        }
                        disabled={isRetryingPersonaSend}
                      />
                    </div>
                  )}

                  {/* Chat Input — wrapper padding matches ExecutionControlBar's
                  outer `p-2` so the composer aligns across the split pane. */}
                  <div className="p-2 empty:p-0">
                    {renderComposer ? (
                      renderComposer({
                        onSend: activeQuestion
                          ? handleQuestionSend
                          : handleSend,
                        onStop: handleStopAgentWrapper,
                        agentStatus,
                        isSending: isSending || isSubmittingAnswer,
                        hasQueuedMessages: hasPresentedQueuedMessages,
                        onEditLastQueued: handleEditLastQueuedWrapper,
                        isReadOnly: isHistoryMode,
                        placeholder:
                          getContextConfig(currentContextType).placeholder,
                        autoFocus: autoFocusInput,
                        enableAttachments:
                          !!effectiveConversationId && !isHistoryMode,
                        attachments,
                        onFilesSelected: uploadFiles,
                        onRemoveAttachment: removeAttachment,
                        attachmentsUploading: uploading,
                        effectiveModel,
                        providerHarness:
                          activeConversationMeta?.providerHarness ??
                          fallbackProviderHarness ??
                          null,
                        ...(composerPersona !== undefined
                          ? { persona: composerPersona }
                          : {}),
                        ...(activeQuestion
                          ? {
                              value: questionInputValue,
                              onChange: setQuestionInputValue,
                              questionMode: {
                                optionCount: activeQuestion.options.length,
                                multiSelect: activeQuestion.multiSelect,
                                onMatchedOptions: handleMatchedOptions,
                              },
                            }
                          : composerDraftKey
                            ? {
                                value: composerDraft?.content ?? "",
                                onChange: (value: string) =>
                                  setComposerDraftContent(
                                    composerDraftKey,
                                    value,
                                  ),
                              }
                            : {}),
                      })
                    ) : (
                      <ChatInput
                        onSend={
                          activeQuestion ? handleQuestionSend : handleSend
                        }
                        onStop={handleStopAgentWrapper}
                        agentStatus={agentStatus}
                        isSending={isSending || isSubmittingAnswer}
                        hasQueuedMessages={hasPresentedQueuedMessages}
                        onEditLastQueued={handleEditLastQueuedWrapper}
                        isReadOnly={isHistoryMode}
                        placeholder={
                          getContextConfig(currentContextType).placeholder
                        }
                        showHelperText={showHelperTextAlways}
                        {...(activeQuestion
                          ? {
                              value: questionInputValue,
                              onChange: setQuestionInputValue,
                              questionMode: {
                                optionCount: activeQuestion.options.length,
                                multiSelect: activeQuestion.multiSelect,
                                onMatchedOptions: handleMatchedOptions,
                              },
                            }
                          : composerDraftKey
                            ? {
                                value: composerDraft?.content ?? "",
                                onChange: (value: string) =>
                                  setComposerDraftContent(
                                    composerDraftKey,
                                    value,
                                  ),
                              }
                            : {})}
                        autoFocus={autoFocusInput}
                        enableAttachments={
                          !!effectiveConversationId && !isHistoryMode
                        }
                        attachments={attachments}
                        onFilesSelected={uploadFiles}
                        onRemoveAttachment={removeAttachment}
                        {...(personaChip !== undefined
                          ? { personaControl: personaChip }
                          : {})}
                      />
                    )}
                  </div>
                </div>
              </div>
            </div>
            </div>
          </ChildSessionNavigationContext.Provider>
        </div>
      </div>
      <PersonaConfirmationDialog {...personaConfirmationDialogProps} />
    </>
  );
}
