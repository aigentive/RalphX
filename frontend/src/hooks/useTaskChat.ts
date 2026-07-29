/**
 * useTaskChat hook - Context-aware chat for task-related conversations
 *
 * A dedicated hook that properly handles task, task_execution, and review context types.
 * Unlike useChat which always returns contextType="task" for task_detail views,
 * this hook takes the context type explicitly and fetches the correct conversations.
 *
 * This simplifies TaskChatPanel by removing 3-way branching logic for different modes.
 */

import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { useEffect, useCallback, useRef, useMemo } from "react";
import { chatApi, type SendAgentMessageResult, type ChatMessageResponse } from "@/api/chat";
import type { ChatConversation, AgentRun } from "@/types/chat-conversation";
import { useChatStore } from "@/stores/chatStore";
import { buildStoreKey } from "@/lib/chat-context-registry";
import { logger } from "@/lib/logger";
import {
  chatKeys,
  invalidateConversationDataQueries,
  useConversationHistoryWindow,
} from "./useChat";
import { useAgentEvents } from "./useAgentEvents";
import { useTaskStateTransitions } from "./useTaskStateTransitions";

/**
 * Task-specific context types
 * - task: Regular task discussion/planning
 * - task_execution: Worker execution conversation
 * - review: Review process conversation
 * - merge: Merge agent conflict resolution conversation
 */
export type TaskContextType =
  | "task"
  | "task_execution"
  | "review"
  | "merge"
  | "branch_update";

/**
 * Hook for task-specific chat functionality with context-aware messaging
 *
 * Unlike the general useChat hook, this hook:
 * - Takes the context type explicitly (not derived from view)
 * - Uses the correct context type for conversation queries
 * - Resets active conversation when context type changes
 * - Builds context keys in the format `${contextType}:${taskId}`
 * - Supports historical message filtering via historicalStatus
 *
 * @param taskId - The task ID
 * @param contextType - The context type (task, task_execution, or review)
 * @param historicalStatus - Optional status to filter messages by time period
 * @returns Object with conversations, messages, loading state, and actions
 *
 * @example
 * ```tsx
 * const {
 *   conversations,
 *   activeConversation,
 *   messages,
 *   isLoading,
 *   sendMessage,
 *   switchConversation,
 *   createConversation,
 *   contextKey,
 *   isHistoricalMode,
 * } = useTaskChat(taskId, "review", "executing");
 * ```
 */
export function useTaskChat(taskId: string, contextType: TaskContextType, historicalStatus?: string, storeKey?: string) {
  const queryClient = useQueryClient();
  const contextKey = buildStoreKey(contextType, taskId);
  // effectiveStoreKey: caller-provided storeKey takes precedence over internally derived contextKey.
  // Consistent with useChat pattern — allows callers to pass an execution-mode-aware key.
  // In most cases storeKey === contextKey since contextType is passed explicitly.
  const effectiveStoreKey = storeKey ?? contextKey;
  const isHistoricalMode = !!historicalStatus;
  logger.debug(`[useTaskChat] taskId=${taskId}, contextType=${contextType}, contextKey=${contextKey}, historicalStatus=${historicalStatus}`);

  // Fetch state transitions for historical message filtering
  const stateTransitions = useTaskStateTransitions(isHistoricalMode ? taskId : undefined);

  const activeConversationId = useChatStore((s) => s.activeConversationIds[effectiveStoreKey] ?? null);
  const setActiveConversation = useChatStore((s) => s.setActiveConversation);
  const setAgentRunning = useChatStore((s) => s.setAgentRunning);

  // Fetch conversations for this specific context type and task
  const conversations = useQuery<ChatConversation[], Error>({
    queryKey: chatKeys.conversationList(contextType, taskId),
    queryFn: async () => {
      logger.debug(`[useTaskChat] Fetching conversations: contextType=${contextType}, taskId=${taskId}`);
      const result = await chatApi.listConversations(contextType, taskId);
      logger.debug(`[useTaskChat] Fetched ${result.length} conversations`);
      return result;
    },
  });

  // Fetch active conversation with a tail-window transcript.
  const activeConversation = useConversationHistoryWindow(activeConversationId, {
    pageSize: 40,
  });

  // Fetch agent run status for the active conversation
  const agentRunStatus = useQuery<AgentRun | null, Error>({
    queryKey: chatKeys.agentRun(activeConversationId ?? ""),
    queryFn: () => {
      if (!activeConversationId) {
        return null;
      }
      return chatApi.getAgentRunStatus(activeConversationId);
    },
    enabled: !!activeConversationId,
    refetchInterval: (query) => {
      // Poll every 2 seconds if agent is running, and keep polling while the query is
      // FAILING: on an error with no prior success `state.data` is undefined, which used to
      // read as "not running" and stopped the poll permanently.
      if (query.state.status === "error") return 2000;
      const agentRun = query.state.data;
      return agentRun?.status === "running" ? 2000 : false;
    },
  });

  // Subscribe to agent events for real-time updates
  // Pass effectiveStoreKey so setActiveConversation writes to the correct scoped slot.
  useAgentEvents(activeConversationId, effectiveStoreKey);

  // Track previous context to detect changes
  const prevContextRef = useRef<string | null>(null);

  // Reset activeConversationId and clear stale agent state when context type or task changes
  useEffect(() => {
    const currentContext = effectiveStoreKey;
    if (prevContextRef.current !== null && prevContextRef.current !== currentContext) {
      // Context changed - clear agent state on OLD context key and reset conversation
      // This prevents stale isAgentRunning entries from previous context types
      setAgentRunning(prevContextRef.current, false);
      setActiveConversation(prevContextRef.current, null);
    }
    prevContextRef.current = currentContext;
  }, [effectiveStoreKey, setActiveConversation, setAgentRunning]);

  // Auto-select the most recent conversation for this context
  // Use a ref to track initialization and prevent infinite loops
  const hasAutoSelectedRef = useRef(false);

  useEffect(() => {
    // Reset auto-select flag when context changes
    hasAutoSelectedRef.current = false;
  }, [effectiveStoreKey]);

  useEffect(() => {
    // CRITICAL: Check for stale activeConversationId FIRST, before checking hasAutoSelectedRef.
    // The activeConversationId in the store is global - it persists across context switches.
    // If it doesn't belong to the current context's conversations, it's stale and must be reset.
    if (activeConversationId && conversations.data && conversations.data.length > 0) {
      const belongsToContext = conversations.data.some(c => c.id === activeConversationId);
      if (!belongsToContext) {
        logger.debug(`[useTaskChat] Stale activeConversationId=${activeConversationId} not in context ${effectiveStoreKey}, resetting`);
        // Reset both the ID and the flag so auto-select can run
        hasAutoSelectedRef.current = false;
        setActiveConversation(effectiveStoreKey, null);
        return; // Will re-run on next render with null activeConversationId
      }
    }

    // Only auto-select once per context
    if (hasAutoSelectedRef.current) {
      return;
    }

    if (!activeConversationId && conversations.data && conversations.data.length > 0) {
      // Agent contexts (merge/execution) create fresh conversations per attempt.
      // Sort by createdAt so the newest conversation wins even before it has messages.
      const isAgentContext = contextType === "merge" || contextType === "task_execution";
      const sorted = [...conversations.data].sort((a, b) => {
        const aTime = isAgentContext ? a.createdAt : (a.lastMessageAt || a.createdAt);
        const bTime = isAgentContext ? b.createdAt : (b.lastMessageAt || b.createdAt);
        return new Date(bTime).getTime() - new Date(aTime).getTime();
      });
      const mostRecent = sorted[0];

      if (mostRecent) {
        logger.debug(`[useTaskChat] Auto-selecting conversation ${mostRecent.id} for context ${effectiveStoreKey}`);
        hasAutoSelectedRef.current = true;
        setActiveConversation(effectiveStoreKey, mostRecent.id);
      }
    }
  }, [activeConversationId, conversations.data, setActiveConversation, effectiveStoreKey, contextType]);

  // Sync agent running state based on backend status
  const isRunning = agentRunStatus.data?.status === "running";

  useEffect(() => {
    // Only set to true based on backend status (for initial load recovery)
    // Don't set to false here - let the agent:run_completed event (or agent:turn_completed in interactive mode) handle that
    if (isRunning) {
      setAgentRunning(effectiveStoreKey, true);
    }
  }, [effectiveStoreKey, isRunning, setAgentRunning]);

  // Unified loading state
  const isLoading =
    conversations.isLoading ||
    (activeConversation.isPending && !!activeConversationId) ||
    (!activeConversationId && conversations.data && conversations.data.length > 0) ||
    (isHistoricalMode && stateTransitions.isLoading);

  // Filter messages by historical status time period
  const filteredMessages = useMemo((): ChatMessageResponse[] => {
    const allMessages = activeConversation.data?.messages ?? [];

    // If not in historical mode, return all messages
    if (!isHistoricalMode || !historicalStatus) {
      return allMessages;
    }

    // Need state transitions to determine time range
    if (!stateTransitions.data || stateTransitions.data.length === 0) {
      return allMessages;
    }

    // Find the time range when the task was in the historical status
    // First, find when the task entered this status
    const entryTransition = stateTransitions.data.find(t => t.toStatus === historicalStatus);
    if (!entryTransition) {
      // Status never entered, show no messages
      return [];
    }

    const startTime = new Date(entryTransition.timestamp).getTime();

    // Find when the task left this status (next transition after entry)
    const transitionIndex = stateTransitions.data.findIndex(t => t.toStatus === historicalStatus);
    const exitTransition = stateTransitions.data[transitionIndex + 1];
    // If no exit transition exists, the task is still in this state - include all messages after entry
    // Use Number.MAX_SAFE_INTEGER as a stable "infinity" value for comparison
    const endTime = exitTransition ? new Date(exitTransition.timestamp).getTime() : Number.MAX_SAFE_INTEGER;

    // Filter messages within this time range
    return allMessages.filter(msg => {
      const msgTime = new Date(msg.createdAt).getTime();
      return msgTime >= startTime && msgTime <= endTime;
    });
  }, [activeConversation.data?.messages, isHistoricalMode, historicalStatus, stateTransitions.data]);

  // Send message mutation
  const sendMessage = useMutation<SendAgentMessageResult, Error, string>({
    mutationFn: async (content: string) => {
      // Set agent running immediately so subsequent messages get queued
      setAgentRunning(contextKey, true);
      return chatApi.sendAgentMessage(contextType, taskId, content);
    },
    onSuccess: (result) => {
      // Invalidate active conversation to refetch messages
      if (activeConversationId) {
        invalidateConversationDataQueries(queryClient, activeConversationId);
      }

      // Invalidate conversations list to update message counts
      queryClient.invalidateQueries({
        queryKey: chatKeys.conversationList(contextType, taskId),
      });

      // If this is a new conversation, set it as active
      if (result.isNewConversation) {
        setActiveConversation(effectiveStoreKey, result.conversationId);
      }
    },
    onError: () => {
      // Reset agent running state on error
      setAgentRunning(effectiveStoreKey, false);
    },
  });

  // Switch conversation
  const switchConversation = useCallback(
    (conversationId: string) => {
      setActiveConversation(effectiveStoreKey, conversationId);

      // Invalidate the conversation query to ensure fresh data is fetched
      invalidateConversationDataQueries(queryClient, conversationId);
    },
    [setActiveConversation, queryClient, effectiveStoreKey]
  );

  // Create new conversation
  const createConversation = useCallback(async () => {
    const newConversation = await chatApi.createConversation(contextType, taskId);
    setActiveConversation(effectiveStoreKey, newConversation.id);

    // Invalidate conversations list
    queryClient.invalidateQueries({
      queryKey: chatKeys.conversationList(contextType, taskId),
    });

    return newConversation;
  }, [contextType, taskId, setActiveConversation, queryClient, effectiveStoreKey]);

  return {
    // Data
    conversations,
    activeConversation,
    messages: filteredMessages,
    agentRunStatus,
    // State
    isLoading,
    isHistoricalMode,
    activeConversationId,
    contextKey: effectiveStoreKey,
    contextType,
    // Actions
    sendMessage,
    switchConversation,
    createConversation,
  };
}
