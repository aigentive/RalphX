/**
 * Chat store using Zustand with immer middleware
 *
 * Manages chat panel state for the frontend. Messages are stored in a
 * Record keyed by context key (e.g., "session:abc", "task:def", "project:xyz")
 * for efficient lookup by context.
 */

import { create } from "zustand";
import { immer } from "zustand/middleware/immer";
import type { ChatMessage } from "@/types/ideation";
import type { ChatContext } from "@/types/chat";
import type { ModelDisplay } from "@/types/chat-conversation";
import { buildStoreKey } from "@/lib/chat-context-registry";

// ============================================================================
// Agent Status Type
// ============================================================================

/**
 * Tri-state agent status for interactive sessions.
 * - "idle" — no agent running (default, not stored in record)
 * - "generating" — agent is actively producing output
 * - "waiting_for_input" — agent finished a turn, waiting for user input
 */
export type AgentStatus = "idle" | "generating" | "waiting_for_input";

// ============================================================================
// Types
// ============================================================================

/**
 * A queued message that will be sent when the agent finishes
 *
 * The ID is shared between frontend and backend for reliable sync.
 * Frontend generates the ID and sends it to the backend, ensuring
 * both sides can reference the same message by ID.
 */
export interface QueuedMessage {
  /** Message ID (shared between frontend and backend) */
  id: string;
  /** Message content */
  content: string;
  /** When the message was queued */
  createdAt: string;
  /** Whether this message is currently being edited */
  isEditing: boolean;
}

// ============================================================================
// State Interface
// ============================================================================

interface ChatState {
  /** Messages indexed by context key for efficient lookup */
  messages: Record<string, ChatMessage[]>;
  /** Current chat context (view, selected items) */
  context: ChatContext | null;
  /** Loading state for async operations */
  isLoading: boolean;
  /** Active conversation IDs scoped per context key (e.g., "session:abc-123") */
  activeConversationIds: Record<string, string | null>;
  /** Active agent run IDs scoped per context key. Used to ignore stale terminal events. */
  activeAgentRunIds: Record<string, string>;
  /** Messages queued to send when agent finishes, keyed by context key (e.g., "task:id", "task_execution:id", "review:id") */
  queuedMessages: Record<string, QueuedMessage[]>;
  /** Agent status keyed by context key. Absent = "idle". Values: "generating" | "waiting_for_input" */
  agentStatus: Record<string, AgentStatus>;
  /** Whether a message is currently being sent, keyed by context key */
  isSending: Record<string, boolean>;
  /** Whether a team is active for a context key (enables team UI) */
  isTeamActive: Record<string, boolean>;
  /** Last agent event timestamp per context key — used by watchdog for stuck-generating recovery */
  lastAgentEventTimestamp: Record<string, number>;
  /** Tool call start timestamps: storeKey → { toolCallId → epoch ms } — for elapsed timer display */
  toolCallStartTimes: Record<string, Record<string, number>>;
  /** Last tool call completion timestamp per storeKey (epoch ms) — for grace period check in watchdog */
  lastToolCallCompletionTimestamp: Record<string, number>;
  /** Per-tool-call completion timestamps: storeKey → { toolCallId → epoch ms } — for final duration display in widgets */
  toolCallCompletionTimestamps: Record<string, Record<string, number>>;
  /** Effective model per context key (id + label). Transient — NOT persisted. */
  effectiveModel: Record<string, ModelDisplay>;
}

// ============================================================================
// Actions Interface
// ============================================================================

interface ChatActions {
  /** Set the current chat context */
  setContext: (context: ChatContext | null) => void;
  /** Add a message to a context */
  addMessage: (contextKey: string, message: ChatMessage) => void;
  /** Set all messages for a context */
  setMessages: (contextKey: string, messages: ChatMessage[]) => void;
  /** Clear messages for a context */
  clearMessages: (contextKey: string) => void;
  /** Set loading state */
  setLoading: (isLoading: boolean) => void;
  /** Set the active conversation ID for a specific context key */
  setActiveConversation: (storeKey: string, conversationId: string | null) => void;
  /** Set the active agent run ID for a specific context key */
  setActiveAgentRun: (storeKey: string, runId: string) => void;
  /** Clear the active agent run ID, optionally only if it matches the expected run ID */
  clearActiveAgentRun: (storeKey: string, expectedRunId?: string | null) => void;
  /** Set agent status for a context (tri-state: "idle" | "generating" | "waiting_for_input") */
  setAgentStatus: (contextKey: string, status: AgentStatus) => void;
  /** Backward-compat wrapper: true → "generating", false → "idle" */
  setAgentRunning: (contextKey: string, isRunning: boolean) => void;
  /** Set whether a message is currently being sent for a context */
  setSending: (contextKey: string, isSending: boolean) => void;
  /** Clear all agent running states for a task (all context types) */
  clearAgentRunningForTask: (taskId: string) => void;
  /** Queue a message to be sent when the agent finishes */
  queueMessage: (contextKey: string, content: string, clientId?: string) => void;
  /** Edit a queued message */
  editQueuedMessage: (contextKey: string, id: string, content: string) => void;
  /** Delete a queued message */
  deleteQueuedMessage: (contextKey: string, id: string) => void;
  /** Process the queue (send first message and remove from queue) */
  processQueue: (contextKey: string) => Promise<void>;
  /** Start editing a queued message */
  startEditingQueuedMessage: (contextKey: string, id: string) => void;
  /** Stop editing a queued message */
  stopEditingQueuedMessage: (contextKey: string, id: string) => void;
  /** Set team active state for a context */
  setTeamActive: (contextKey: string, isActive: boolean) => void;
  /** Update last agent event timestamp for watchdog tracking */
  updateLastAgentEvent: (key: string) => void;
  /** Record start timestamp for a tool call (for elapsed timer display) */
  setToolCallStartTime: (storeKey: string, toolCallId: string, timestamp: number) => void;
  /** Remove start timestamp when a tool call completes */
  removeToolCallStartTime: (storeKey: string, toolCallId: string) => void;
  /** Clear all tool call start times for a storeKey (on run_completed) */
  clearToolCallStartTimes: (storeKey: string) => void;
  /** Record last tool call completion timestamp for grace period tracking */
  setLastToolCallCompletionTimestamp: (storeKey: string, timestamp: number) => void;
  /** Record per-tool-call completion timestamp for final duration display in widgets */
  setToolCallCompletionTimestamp: (storeKey: string, toolCallId: string, timestamp: number) => void;
  /** Clear all per-tool-call completion timestamps for a storeKey (on run_completed) */
  clearToolCallCompletionTimestamps: (storeKey: string) => void;
  /** Set the effective model for a context key (from agent:run_started event or session HTTP response) */
  setEffectiveModel: (storeKey: string, model: ModelDisplay) => void;
}

// ============================================================================
// Store Implementation
// ============================================================================

export const useChatStore = create<ChatState & ChatActions>()(
  immer((set, get) => ({
    // Initial state
    messages: {},
    context: null,
    isLoading: false,
    activeConversationIds: {},
    activeAgentRunIds: {},
    queuedMessages: {},
    agentStatus: {},
    isSending: {},
    isTeamActive: {},
    lastAgentEventTimestamp: {},
    toolCallStartTimes: {},
    lastToolCallCompletionTimestamp: {},
    toolCallCompletionTimestamps: {},
    effectiveModel: {},

    // Actions
    setContext: (context) =>
      set((state) => {
        state.context = context;
      }),

    addMessage: (contextKey, message) =>
      set((state) => {
        if (!state.messages[contextKey]) {
          state.messages[contextKey] = [];
        }
        state.messages[contextKey].push(message);
      }),

    setMessages: (contextKey, messages) =>
      set((state) => {
        state.messages[contextKey] = messages;
      }),

    clearMessages: (contextKey) =>
      set((state) => {
        delete state.messages[contextKey];
      }),

    setLoading: (isLoading) =>
      set((state) => {
        state.isLoading = isLoading;
      }),

    setActiveConversation: (storeKey, conversationId) =>
      set((state) => {
        if (state.activeConversationIds[storeKey] === conversationId) return;
        state.activeConversationIds[storeKey] = conversationId;
      }),

    setActiveAgentRun: (storeKey, runId) =>
      set((state) => {
        if (state.activeAgentRunIds[storeKey] === runId) return;
        state.activeAgentRunIds[storeKey] = runId;
      }),

    clearActiveAgentRun: (storeKey, expectedRunId) =>
      set((state) => {
        const activeRunId = state.activeAgentRunIds[storeKey];
        if (activeRunId == null) return;
        if (expectedRunId != null && activeRunId !== expectedRunId) return;
        delete state.activeAgentRunIds[storeKey];
      }),

    setAgentStatus: (contextKey, status) =>
      set((state) => {
        if (status === "idle") {
          if (!(contextKey in state.agentStatus) && !(contextKey in state.activeAgentRunIds)) {
            return; // already absent — no-op
          }
          delete state.agentStatus[contextKey];
          delete state.activeAgentRunIds[contextKey];
        } else {
          if (state.agentStatus[contextKey] === status) return; // already set — no-op
          state.agentStatus[contextKey] = status;
        }
      }),

    setAgentRunning: (contextKey, isRunning) =>
      set((state) => {
        if (isRunning) {
          if (state.agentStatus[contextKey] === "generating") return; // already generating — no-op
          state.agentStatus[contextKey] = "generating";
        } else {
          if (!(contextKey in state.agentStatus) && !(contextKey in state.activeAgentRunIds)) {
            return; // already absent — no-op
          }
          delete state.agentStatus[contextKey];
          delete state.activeAgentRunIds[contextKey];
        }
      }),

    setSending: (contextKey, isSending) =>
      set((state) => {
        if (isSending) {
          if (state.isSending[contextKey]) return; // already true — no-op
          state.isSending[contextKey] = true;
        } else {
          if (!(contextKey in state.isSending)) return; // already absent — no-op
          delete state.isSending[contextKey];
        }
      }),

    clearAgentRunningForTask: (taskId) =>
      set((state) => {
        // Clear all context keys ending with :taskId (task:id, task_execution:id, review:id)
        Object.keys(state.agentStatus).forEach((key) => {
          if (key.endsWith(`:${taskId}`)) {
            delete state.agentStatus[key];
          }
        });
        Object.keys(state.activeAgentRunIds).forEach((key) => {
          if (key.endsWith(`:${taskId}`)) {
            delete state.activeAgentRunIds[key];
          }
        });
      }),

    queueMessage: (contextKey, content, clientId) =>
      set((state) => {
        const id = clientId ?? `queued-${Date.now()}-${Math.random()}`;
        if (!state.queuedMessages[contextKey]) {
          state.queuedMessages[contextKey] = [];
        }
        // Duplicate-ID guard: if a message with this ID already exists, no-op
        if (clientId != null && state.queuedMessages[contextKey].some((m) => m.id === clientId)) {
          return;
        }
        const queuedMessage: QueuedMessage = {
          id,
          content,
          createdAt: new Date().toISOString(),
          isEditing: false,
        };
        state.queuedMessages[contextKey].push(queuedMessage);
      }),

    editQueuedMessage: (contextKey, id, content) =>
      set((state) => {
        const messages = state.queuedMessages[contextKey];
        if (messages) {
          const message = messages.find((m) => m.id === id);
          if (message) {
            message.content = content;
            message.isEditing = false;
          }
        }
      }),

    deleteQueuedMessage: (contextKey, id) =>
      set((state) => {
        if (state.queuedMessages[contextKey]) {
          state.queuedMessages[contextKey] = state.queuedMessages[
            contextKey
          ].filter((m) => m.id !== id);

          // Clean up empty arrays
          if (state.queuedMessages[contextKey].length === 0) {
            delete state.queuedMessages[contextKey];
          }
        }
      }),

    startEditingQueuedMessage: (contextKey, id) =>
      set((state) => {
        const messages = state.queuedMessages[contextKey];
        if (messages) {
          const message = messages.find((m) => m.id === id);
          if (message) {
            message.isEditing = true;
          }
        }
      }),

    stopEditingQueuedMessage: (contextKey, id) =>
      set((state) => {
        const messages = state.queuedMessages[contextKey];
        if (messages) {
          const message = messages.find((m) => m.id === id);
          if (message) {
            message.isEditing = false;
          }
        }
      }),

    setTeamActive: (contextKey, isActive) =>
      set((state) => {
        if (isActive) {
          if (state.isTeamActive[contextKey]) return; // already true — no-op
          state.isTeamActive[contextKey] = true;
        } else {
          if (!(contextKey in state.isTeamActive)) return; // already absent — no-op
          delete state.isTeamActive[contextKey];
        }
      }),

    updateLastAgentEvent: (key) =>
      set((state) => {
        state.lastAgentEventTimestamp[key] = Date.now();
      }),

    setToolCallStartTime: (storeKey, toolCallId, timestamp) =>
      set((state) => {
        if (!state.toolCallStartTimes[storeKey]) {
          state.toolCallStartTimes[storeKey] = {};
        }
        state.toolCallStartTimes[storeKey][toolCallId] = timestamp;
      }),

    removeToolCallStartTime: (storeKey, toolCallId) =>
      set((state) => {
        const times = state.toolCallStartTimes[storeKey];
        if (!times) return;
        delete times[toolCallId];
        if (Object.keys(times).length === 0) {
          delete state.toolCallStartTimes[storeKey];
        }
      }),

    clearToolCallStartTimes: (storeKey) =>
      set((state) => {
        delete state.toolCallStartTimes[storeKey];
      }),

    setLastToolCallCompletionTimestamp: (storeKey, timestamp) =>
      set((state) => {
        state.lastToolCallCompletionTimestamp[storeKey] = timestamp;
      }),

    setToolCallCompletionTimestamp: (storeKey, toolCallId, timestamp) =>
      set((state) => {
        if (!state.toolCallCompletionTimestamps[storeKey]) {
          state.toolCallCompletionTimestamps[storeKey] = {};
        }
        state.toolCallCompletionTimestamps[storeKey][toolCallId] = timestamp;
      }),

    clearToolCallCompletionTimestamps: (storeKey) =>
      set((state) => {
        delete state.toolCallCompletionTimestamps[storeKey];
      }),

    setEffectiveModel: (storeKey, model) =>
      set((state) => ({
        effectiveModel: { ...state.effectiveModel, [storeKey]: model },
      })),

    processQueue: async (contextKey) => {
      const state = get();
      const messages = state.queuedMessages[contextKey];
      if (!messages || messages.length === 0) {
        return;
      }

      // Remove the first message from the queue
      set((draft) => {
        if (draft.queuedMessages[contextKey]) {
          draft.queuedMessages[contextKey].shift();
          if (draft.queuedMessages[contextKey].length === 0) {
            delete draft.queuedMessages[contextKey];
          }
        }
      });
    },
  }))
);

// ============================================================================
// Context Key Helper
// ============================================================================

/**
 * Generate a context key from a ChatContext
 * Used to key messages by their source context
 *
 * Delegates to the chat-context-registry's buildStoreKey for consistent key formatting.
 */
export function getContextKey(context: ChatContext): string {
  if (context.view === "ideation" && context.ideationSessionId) {
    return buildStoreKey("ideation", context.ideationSessionId);
  }
  if (context.view === "task_detail" && context.selectedTaskId) {
    return buildStoreKey("task", context.selectedTaskId);
  }
  return buildStoreKey("project", context.projectId);
}

// ============================================================================
// Selectors (defined outside store for memoization)
// ============================================================================

/**
 * Select messages for a specific context key
 * @param contextKey - The context key to get messages for
 * @returns Selector function returning messages array
 */
export const selectMessagesForContext =
  (contextKey: string) =>
  (state: ChatState): ChatMessage[] =>
    state.messages[contextKey] ?? EMPTY_MESSAGES;

/**
 * Select message count for a specific context key
 * @param contextKey - The context key to count messages for
 * @returns Selector function returning message count
 */
export const selectMessageCount =
  (contextKey: string) =>
  (state: ChatState): number =>
    state.messages[contextKey]?.length ?? 0;

/**
 * Select queued messages for a specific context
 * @param contextKey - The context key to get queued messages for
 * @returns Selector function returning queued messages array
 */
export const selectQueuedMessages =
  (contextKey: string) =>
  (state: ChatState): QueuedMessage[] =>
    state.queuedMessages[contextKey] ?? EMPTY_QUEUED_MESSAGES;

/**
 * Select agent status for a context (tri-state)
 * @param contextKey - The context key to check
 * @returns Selector function returning AgentStatus ("idle" | "generating" | "waiting_for_input")
 */
export const selectAgentStatus =
  (contextKey: string) =>
  (state: ChatState): AgentStatus =>
    state.agentStatus[contextKey] ?? "idle";

/**
 * Select whether an agent is currently running for a context (backward-compat boolean).
 * Returns true for both "generating" and "waiting_for_input" (agent process alive).
 * @param contextKey - The context key to check
 * @returns Selector function returning agent running state
 */
export const selectIsAgentRunning =
  (contextKey: string) =>
  (state: ChatState): boolean =>
    contextKey in state.agentStatus;

/**
 * Select whether a message is currently being sent for a context
 * @param contextKey - The context key to check
 * @returns Selector function returning sending state
 */
export const selectIsSending =
  (contextKey: string) =>
  (state: ChatState): boolean =>
    state.isSending[contextKey] ?? false;

/**
 * Select active conversation ID for a specific context key.
 * Prefer inline usage: `useChatStore((s) => s.activeConversationIds[storeKey])`
 * Use this factory only when a named selector reference is required.
 * @param storeKey - The context key (e.g., "session:abc-123")
 * @returns Selector function returning scoped active conversation ID or undefined
 */
export const selectActiveConversationId =
  (storeKey: string) =>
  (state: ChatState): string | null | undefined =>
    state.activeConversationIds[storeKey];

/**
 * Select active agent run ID for a specific context key.
 * @param storeKey - The context key (e.g., "session:abc-123")
 * @returns Selector function returning scoped active agent run ID or undefined
 */
export const selectActiveAgentRunId =
  (storeKey: string) =>
  (state: ChatState): string | undefined =>
    state.activeAgentRunIds[storeKey];

/**
 * Select whether a team is active for a context
 * @param contextKey - The context key to check
 * @returns Selector function returning team active state
 */
export const selectIsTeamActive =
  (contextKey: string) =>
  (state: ChatState): boolean =>
    state.isTeamActive[contextKey] ?? false;

/**
 * Select last agent event timestamp for a context (for watchdog use)
 * @param contextKey - The context key to get timestamp for
 * @returns Selector function returning timestamp (ms) or 0 if never set
 */
export const selectLastAgentEventTimestamp =
  (contextKey: string) =>
  (state: ChatState): number =>
    state.lastAgentEventTimestamp[contextKey] ?? 0;

/**
 * Select tool call start timestamps for a context.
 * Uses a stable empty record to avoid infinite re-render loops from fresh `{}` fallbacks.
 * @param contextKey - The context key to get tool call timestamps for
 * @returns Selector function returning toolCallId -> start timestamp map
 */
export const selectToolCallStartTimes =
  (contextKey: string) =>
  (state: ChatState): Record<string, number> =>
    state.toolCallStartTimes[contextKey] ?? EMPTY_TOOL_CALL_START_TIMES;

/**
 * Select last tool call completion timestamp for a context (for grace period watchdog check).
 * @param contextKey - The context key to get timestamp for
 * @returns Selector function returning timestamp (ms) or 0 if never set
 */
export const selectLastToolCallCompletionTimestamp =
  (contextKey: string) =>
  (state: ChatState): number =>
    state.lastToolCallCompletionTimestamp[contextKey] ?? 0;

/**
 * Select per-tool-call completion timestamps for a context (for final duration display in widgets).
 * Uses a stable empty record to avoid infinite re-render loops from fresh `{}` fallbacks.
 * @param contextKey - The context key to get completion timestamps for
 * @returns Selector function returning toolCallId -> completion timestamp map
 */
export const selectToolCallCompletionTimestamps =
  (contextKey: string) =>
  (state: ChatState): Record<string, number> =>
    state.toolCallCompletionTimestamps[contextKey] ?? EMPTY_TOOL_CALL_START_TIMES;

/**
 * Select the effective model for a context key.
 * Returns undefined if no effective model has been set for this key.
 * @param storeKey - The context key (e.g., "session:abc-123")
 * @returns Selector function returning ModelDisplay or undefined
 */
export const selectEffectiveModel =
  (storeKey: string) =>
  (s: ChatState & ChatActions): ModelDisplay | undefined =>
    s.effectiveModel[storeKey];

// Stable empty arrays to avoid creating new references
const EMPTY_MESSAGES: ChatMessage[] = [];
const EMPTY_QUEUED_MESSAGES: QueuedMessage[] = [];
const EMPTY_TOOL_CALL_START_TIMES: Record<string, number> = Object.freeze({});

// Expose chat store to window in web mode for Playwright testing
if (typeof window !== "undefined" && !window.__TAURI_INTERNALS__) {
  window.__chatStore = useChatStore;
}
