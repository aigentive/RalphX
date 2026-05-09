/**
 * useChatPanelContext - Context management for IntegratedChatPanel
 *
 * Handles:
 * - Computing chat context based on selected task/ideation session
 * - Computing store context key for queue/agent state operations
 * - Context change effects (clearing old conversation, cache invalidation)
 * - Auto-selecting conversations in execution/review modes
 */

import { useState, useRef, useEffect, useMemo, useCallback } from "react";
import { toast } from "sonner";
import { useQueryClient } from "@tanstack/react-query";
import { useChatStore, getContextKey } from "@/stores/chatStore";
import { useTeamStore } from "@/stores/teamStore";
import { useIdeationStore } from "@/stores/ideationStore";
import { buildStoreKey } from "@/lib/chat-context-registry";
import { chatKeys } from "@/hooks/useChat";
import type { ChatContext } from "@/types/chat";
import type { ContextType } from "@/types/chat-conversation";
import type { ToolCall } from "@/components/Chat/ToolCallIndicator";
import type { StreamingTask, StreamingContentBlock } from "@/types/streaming-task";

interface UseChatPanelContextProps {
  projectId: string;
  ideationSessionId: string | undefined;
  selectedTaskId: string | undefined;
  isExecutionMode: boolean;
  isReviewMode: boolean;
  isMergeMode: boolean;
  isHistoryMode: boolean;
  /** Override conversation ID for history mode - forces selection of specific conversation */
  overrideConversationId?: string | undefined;
  /** Override the store key used for queue/running state. */
  storeContextKeyOverride?: string | undefined;
  /** Override agent run ID for history mode - used for scroll positioning */
  overrideAgentRunId?: string | undefined;
  /** Whether this panel is currently visible — re-triggers autoSelectConversation on false→true transition */
  isVisible?: boolean;
}

interface ConversationData {
  id: string;
  lastMessageAt?: string | null;
  createdAt: string;
}

interface ConversationsQueryResult {
  data?: ConversationData[] | undefined;
  isLoading: boolean;
}

export function useChatPanelContext({
  projectId,
  ideationSessionId,
  selectedTaskId,
  isExecutionMode,
  isReviewMode,
  isMergeMode,
  overrideConversationId,
  storeContextKeyOverride,
  overrideAgentRunId,
  isVisible = true,
}: UseChatPanelContextProps) {
  const queryClient = useQueryClient();
  const setActiveConversation = useChatStore((s) => s.setActiveConversation);
  const clearMessages = useChatStore((s) => s.clearMessages);
  const setSending = useChatStore((s) => s.setSending);

  // Streaming tool calls - accumulated during agent execution
  const [streamingToolCalls, setStreamingToolCalls] = useState<ToolCall[]>([]);

  // Streaming content blocks - text and tool calls interleaved in order for real-time display
  const [streamingContentBlocks, setStreamingContentBlocks] = useState<StreamingContentBlock[]>([]);

  // Streaming tasks - subagent Task tool calls during agent execution
  const [streamingTasks, setStreamingTasks] = useState<Map<string, StreamingTask>>(new Map());

  // Finalizing state - true between agent:message_created (clears streaming) and query refetch completing
  // Keeps the last-assistant-message filter active to prevent text duplication flash
  const [isFinalizing, setIsFinalizing] = useState(false);

  // Build chat context based on selected task or ideation session
  const chatContext: ChatContext = useMemo(() => {
    if (ideationSessionId) {
      return {
        view: "ideation",
        projectId,
        ideationSessionId,
      };
    }
    if (selectedTaskId) {
      return {
        view: "task_detail",
        projectId,
        selectedTaskId,
      };
    }
    return {
      view: "kanban",
      projectId,
    };
  }, [selectedTaskId, projectId, ideationSessionId]);

  // Compute store context key for queue/agent state operations
  // Uses context-aware keys via registry: "task_execution:id", "review:id", "merge:id", or standard keys
  const storeContextKey = useMemo(() => {
    if (storeContextKeyOverride) {
      return storeContextKeyOverride;
    }
    if (isMergeMode && selectedTaskId) {
      return buildStoreKey("merge", selectedTaskId);
    }
    if (isExecutionMode && selectedTaskId) {
      return buildStoreKey("task_execution", selectedTaskId);
    }
    if (isReviewMode && selectedTaskId) {
      return buildStoreKey("review", selectedTaskId);
    }
    return getContextKey(chatContext);
  }, [
    storeContextKeyOverride,
    isMergeMode,
    isExecutionMode,
    isReviewMode,
    selectedTaskId,
    chatContext,
  ]);

  // Active conversation ID scoped to this panel's storeContextKey. When the
  // caller owns a specific conversation, the override is authoritative on the
  // first render instead of waiting for the store sync effect.
  const storedActiveConversationId = useChatStore((s) => s.activeConversationIds[storeContextKey] ?? null);
  const activeConversationId = overrideConversationId ?? storedActiveConversationId;

  // Context key for tracking changes
  const contextKey = ideationSessionId
    ? `ideation:${ideationSessionId}`
    : selectedTaskId
      ? `${isMergeMode ? "merge" : isExecutionMode ? "execution" : isReviewMode ? "review" : "task"}:${selectedTaskId}`
      : (storeContextKeyOverride ?? `project:${projectId}`);

  // Initialize with empty string to ensure cleanup runs on first mount
  const prevContextKeyRef = useRef("");
  const prevContextTypeRef = useRef<{ type: string; id: string } | null>(null);
  // Track previous storeContextKey to clear the OLD key (not the new one) on context change
  const prevStoreContextKeyRef = useRef("");

  // Track latest storeContextKey in a ref so the unmount cleanup always clears the correct key
  const storeContextKeyRef = useRef(storeContextKey);
  useEffect(() => {
    storeContextKeyRef.current = storeContextKey;
  }, [storeContextKey]);

  // Cleanup on unmount: clear isSending for the current context key
  // agentStatus is intentionally NOT cleared here — it is owned by useGlobalAgentLifecycle
  // and must survive unmount/remount cycles (e.g., PlanningView key={session.id} switches).
  useEffect(() => {
    return () => {
      setSending(storeContextKeyRef.current, false);
    };
  // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // Auto-select tracking
  const hasAutoSelectedRef = useRef(false);

  // Track previous visibility to detect false→true transitions
  const prevIsVisibleRef = useRef(false);

  // Determine current context type and ID for validation
  // Declared here (before visibility effect) to avoid temporal dead zone when used in deps array
  const currentContextType: ContextType = ideationSessionId
    ? "ideation"
    : selectedTaskId
      ? (isMergeMode ? "merge" : isExecutionMode ? "task_execution" : isReviewMode ? "review" : "task")
      : "project";
  const currentContextId = ideationSessionId || selectedTaskId || projectId;

  // Re-trigger autoSelectConversation when panel becomes visible again, and invalidate
  // the conversation list so new conversations created while hidden are discovered.
  // Both operations happen atomically in the same useEffect to avoid the race where
  // hasAutoSelectedRef resets (allows re-select) but the list is still stale (selects wrong conv).
  useEffect(() => {
    if (!prevIsVisibleRef.current && isVisible) {
      hasAutoSelectedRef.current = false;
      // Invalidate conversation list on false→true transition (defense-in-depth: conversations
      // created while panel was hidden won't appear until stale time expires without this).
      void queryClient.invalidateQueries({
        queryKey: chatKeys.conversationList(currentContextType, currentContextId),
      });
    }
    prevIsVisibleRef.current = isVisible;
  }, [isVisible, currentContextType, currentContextId, queryClient]);

  // Handle context changes
  useEffect(() => {
    if (prevContextKeyRef.current !== contextKey) {
      // Context changed - capture OLD context from refs BEFORE updating them
      // CRITICAL: use prevStoreContextKeyRef.current (OLD key) to read the OLD conversation.
      // storeContextKey here is already the NEW key — reading it would get null for the new slot.
      const currentConversationId = useChatStore.getState().activeConversationIds[prevStoreContextKeyRef.current];
      const oldContext = prevContextTypeRef.current;

      // DO NOT clear active conversation here - let autoSelectConversation handle
      // the transition atomically to avoid transient empty state

      // Clear streaming state (functional updaters to avoid new-ref re-renders when already empty)
      setStreamingToolCalls(prev => prev.length === 0 ? prev : []);
      setStreamingContentBlocks(prev => prev.length === 0 ? prev : []);
      setStreamingTasks(prev => prev.size === 0 ? prev : new Map());
      setIsFinalizing(false);

      // Cancel and remove the old conversation's agent run query to prevent
      // stale cached data from triggering recovery effects in the new context
      if (currentConversationId) {
        queryClient.cancelQueries({
          queryKey: chatKeys.agentRun(currentConversationId),
        });
        queryClient.removeQueries({
          queryKey: chatKeys.agentRun(currentConversationId),
        });
      }

      // Clear the query cache for the old conversation to prevent stale data
      if (currentConversationId) {
        queryClient.removeQueries({
          queryKey: chatKeys.conversation(currentConversationId),
        });
      }

      // Also clear the old context's conversation list to prevent initialization
      // from picking up stale conversations
      if (oldContext) {
        queryClient.removeQueries({
          queryKey: chatKeys.conversationList(oldContext.type as ContextType, oldContext.id),
        });
      }

      // Clear messages from Zustand store for the old context to free memory
      if (prevContextKeyRef.current) {
        clearMessages(prevContextKeyRef.current);
      }

      // Clear isSending for the OLD store context key on context switch.
      // agentStatus is intentionally NOT cleared here — it is owned by useGlobalAgentLifecycle
      // (agent:run_completed / agent:stopped events). Context switch must not override it.
      if (prevStoreContextKeyRef.current) {
        setSending(prevStoreContextKeyRef.current, false);

        // Read pending plan BEFORE clearing — used for toast notification below
        const oldPendingPlan = useTeamStore.getState().pendingPlans[prevStoreContextKeyRef.current];

        // Notify user if they're switching away from a session with a pending team plan approval
        if (oldPendingPlan) {
          const sessionId = oldPendingPlan.originContextId;
          toast("Team plan approval still pending — switch back to approve", {
            duration: 5000,
            action: {
              label: "Go back",
              onClick: () => { useIdeationStore.getState().setActiveSession(sessionId); },
            },
          });
        }

        // Clear frontend pending plan state only — backend plan survives for re-discovery
        // when user returns to this session (via useTeamEvents Effect 3 hydration).
        // Backend TTL (14 min) is the safety net for true abandonment.
        useTeamStore.getState().clearPendingPlan(prevStoreContextKeyRef.current);
      }

      // Reset auto-select flag when context changes
      hasAutoSelectedRef.current = false;

      // Update refs with NEW context AFTER cleanup
      prevContextKeyRef.current = contextKey;
      prevStoreContextKeyRef.current = storeContextKey;
      const newContextType = ideationSessionId
        ? "ideation"
        : selectedTaskId
          ? (isMergeMode ? "merge" : isExecutionMode ? "task_execution" : isReviewMode ? "review" : "task")
          : "project";
      const newContextId = ideationSessionId || selectedTaskId || projectId;
      prevContextTypeRef.current = { type: newContextType, id: newContextId };
    }
  }, [contextKey, storeContextKey, setActiveConversation, queryClient, clearMessages, setSending, ideationSessionId, selectedTaskId, projectId, isMergeMode, isExecutionMode, isReviewMode]);

  // Track previous override conversation ID to detect changes
  const prevOverrideConversationIdRef = useRef<string | undefined>(undefined);

  // Handle override conversation selection (for history mode)
  useEffect(() => {
    if (overrideConversationId !== prevOverrideConversationIdRef.current) {
      prevOverrideConversationIdRef.current = overrideConversationId;

      if (overrideConversationId) {
        // In history mode with a specific conversation - select it
        setActiveConversation(storeContextKey, overrideConversationId);
        hasAutoSelectedRef.current = true; // Prevent auto-select from overriding
      }
    }
  }, [overrideConversationId, setActiveConversation, storeContextKey]);

  // Auto-select the most recent conversation for execution/review/merge modes
  const autoSelectConversation = useCallback((
    conversations: ConversationsQueryResult,
  ) => {
    const isAgentContext = isMergeMode || isExecutionMode || isReviewMode;

    // Agent contexts (merge/execution) always create fresh conversations per attempt.
    // Sort by createdAt so the newest conversation wins even before it has messages.
    // Non-agent contexts sort by lastMessageAt to surface the most active conversation.
    const sortConversations = (items: ConversationData[]) =>
      [...items].sort((a, b) => {
        const aTime = isAgentContext ? a.createdAt : (a.lastMessageAt || a.createdAt);
        const bTime = isAgentContext ? b.createdAt : (b.lastMessageAt || b.createdAt);
        return new Date(bTime).getTime() - new Date(aTime).getTime();
      });

    // Wait for conversations to load before any validation/selection
    if (conversations.isLoading) {
      return;
    }

    // Explicit conversation owners such as history and Agents archived/session
    // lists must not be replaced by the active-list auto-selection path.
    if (overrideConversationId) {
      return;
    }

    // Read activeConversationId from store snapshot at call-time (not from closure)
    // to avoid including it in useCallback deps — which would cause self-invalidation
    // when autoSelectConversation calls setActiveConversation.
    const currentActiveId = useChatStore.getState().activeConversationIds[storeContextKey];

    // CRITICAL: Check for stale activeConversationId FIRST, before checking hasAutoSelectedRef.
    // If current conversation is stale but new context has conversations, directly select replacement.
    if (currentActiveId && conversations.data) {
      const belongsToContext = conversations.data.length > 0
        ? conversations.data.some(c => c.id === currentActiveId)
        : false;

      if (!belongsToContext) {
        // Current conversation is stale
        if (conversations.data.length > 0) {
          // New context has conversations - directly select most recent
          const sorted = sortConversations(conversations.data);
          const mostRecent = sorted[0];
          if (mostRecent) {
            hasAutoSelectedRef.current = true;
            setActiveConversation(storeContextKey, mostRecent.id);
          }
        } else {
          // Empty list — conversation may be freshly created and list hasn't
          // refetched yet. Don't clear; isConversationInCurrentContext guard
          // prevents wrong-context messages, and auto-select will run when
          // the list populates.
          return;
        }
        return;
      }
    }

    if (isAgentContext && conversations.data && conversations.data.length > 0) {
      const sorted = sortConversations(conversations.data);
      const mostRecent = sorted[0];
      if (mostRecent && mostRecent.id !== currentActiveId) {
        hasAutoSelectedRef.current = true;
        setActiveConversation(storeContextKey, mostRecent.id);
        return;
      }
    }

    // Reset the flag if we're in execution/review mode but have no active conversation
    if (!currentActiveId && hasAutoSelectedRef.current) {
      hasAutoSelectedRef.current = false;
    }

    // Only auto-select once per context change
    if (hasAutoSelectedRef.current) {
      return;
    }

    if (!currentActiveId && conversations.data && conversations.data.length > 0) {
      const sorted = sortConversations(conversations.data);
      const mostRecent = sorted[0];

      if (mostRecent) {
        hasAutoSelectedRef.current = true;
        setActiveConversation(storeContextKey, mostRecent.id);
      }
    }
  }, [isMergeMode, isExecutionMode, isReviewMode, overrideConversationId, setActiveConversation, storeContextKey]);

  return {
    chatContext,
    storeContextKey,
    contextKey,
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
    /** Override agent run ID for scroll positioning in history mode */
    overrideAgentRunId,
  };
}
