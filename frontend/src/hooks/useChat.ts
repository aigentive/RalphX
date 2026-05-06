/**
 * useChat hook - TanStack Query wrapper for context-aware chat
 *
 * Provides hooks for fetching and sending chat messages based on context.
 * Supports conversation management, agent run status, and real-time updates.
 */

import {
  useInfiniteQuery,
  useMutation,
  useQuery,
  useQueryClient,
  type InfiniteData,
  type QueryClient,
} from "@tanstack/react-query";
import { useEffect, useCallback, useMemo, useRef } from "react";
import {
  chatApi,
  type ChatMessageResponse,
  type ConversationMessagesPageResponse,
  type SendAgentMessageResult,
} from "@/api/chat";
import type { ChatContext } from "@/types/chat";
import type { ChatConversation, AgentRun, ContextType } from "@/types/chat-conversation";
import { useChatStore } from "@/stores/chatStore";
import { buildStoreKey } from "@/lib/chat-context-registry";
import { ideationKeys } from "./useIdeation";
import { useAgentEvents } from "./useAgentEvents";

/**
 * Query key factory for chat
 */
export const chatKeys = {
  all: ["chat"] as const,
  messages: () => [...chatKeys.all, "messages"] as const,
  conversations: () => [...chatKeys.all, "conversations"] as const,
  conversation: (conversationId: string) =>
    [...chatKeys.conversations(), conversationId] as const,
  conversationHistory: (conversationId: string) =>
    [...chatKeys.conversation(conversationId), "history"] as const,
  conversationList: (contextType: ContextType, contextId: string) =>
    [...chatKeys.conversations(), contextType, contextId] as const,
  agentRun: (conversationId: string) =>
    [...chatKeys.all, "agent-run", conversationId] as const,
  // Legacy keys for backward compatibility
  sessionMessages: (sessionId: string) =>
    [...chatKeys.messages(), "session", sessionId] as const,
  projectMessages: (projectId: string) =>
    [...chatKeys.messages(), "project", projectId] as const,
  taskMessages: (taskId: string) =>
    [...chatKeys.messages(), "task", taskId] as const,
};

type ConversationQueryData = {
  conversation: ChatConversation;
  messages: ChatMessageResponse[];
};

export type ConversationHistoryWindowData = ConversationQueryData & {
  totalMessageCount: number;
  loadedStartIndex: number;
};

function getConversationMessagesFromHistoryData(
  data: InfiniteData<ConversationMessagesPageResponse> | undefined
): ConversationHistoryWindowData | undefined {
  if (!data || data.pages.length === 0) {
    return undefined;
  }

  const [newestPage] = data.pages;
  if (!newestPage) {
    return undefined;
  }
  const messages = data.pages
    .slice()
    .reverse()
    .flatMap((page) => page.messages);

  return {
    conversation: newestPage.conversation,
    messages,
    totalMessageCount: newestPage.totalMessageCount,
    loadedStartIndex: Math.max(0, newestPage.totalMessageCount - messages.length),
  };
}

export function getCachedConversationMessages(
  queryClient: QueryClient,
  conversationId: string
): ChatMessageResponse[] {
  const fullConversation = queryClient.getQueryData<ConversationQueryData>(
    chatKeys.conversation(conversationId)
  );
  const historyConversation = getConversationMessagesFromHistoryData(
    queryClient.getQueryData<InfiniteData<ConversationMessagesPageResponse>>(
      chatKeys.conversationHistory(conversationId)
    )
  );

  const mergedMessages = new Map<string, ChatMessageResponse>();
  for (const message of fullConversation?.messages ?? []) {
    mergedMessages.set(message.id, message);
  }
  for (const message of historyConversation?.messages ?? []) {
    mergedMessages.set(message.id, message);
  }

  return Array.from(mergedMessages.values());
}

export function invalidateConversationDataQueries(
  queryClient: QueryClient,
  conversationId: string
) {
  queryClient.invalidateQueries({
    queryKey: chatKeys.conversation(conversationId),
    exact: true,
  });
  queryClient.invalidateQueries({
    queryKey: chatKeys.conversationHistory(conversationId),
  });
}

/**
 * Get context type and ID from ChatContext
 *
 * NOTE: This function currently doesn't distinguish between 'task', 'task_execution', and 'review'
 * context types when view='task_detail'. Components like TaskChatPanel handle this distinction
 * by directly querying conversations with the appropriate contextType based on task state.
 */
function getContextTypeAndId(context: ChatContext): {
  contextType: ContextType;
  contextId: string;
} {
  switch (context.view) {
    case "ideation":
      if (!context.ideationSessionId) {
        throw new Error("Ideation context requires ideationSessionId");
      }
      return { contextType: "ideation", contextId: context.ideationSessionId };
    case "task_detail":
      if (!context.selectedTaskId) {
        throw new Error("Task detail context requires selectedTaskId");
      }
      // Returns 'task' contextType by default. Components should query conversations
      // with 'task_execution' or 'review' contextType directly when needed based on task state.
      return { contextType: "task", contextId: context.selectedTaskId };
    case "kanban":
      if (context.selectedTaskId) {
        return { contextType: "task", contextId: context.selectedTaskId };
      }
      return { contextType: "project", contextId: context.projectId };
    default:
      return { contextType: "project", contextId: context.projectId };
  }
}

/**
 * Hook to fetch conversations for a context
 */
export function useConversations(context: ChatContext) {
  const { contextType, contextId } = getContextTypeAndId(context);

  return useQuery<ChatConversation[], Error>({
    queryKey: chatKeys.conversationList(contextType, contextId),
    queryFn: () => chatApi.listConversations(contextType, contextId),
    staleTime: 0,
  });
}

/**
 * Hook to fetch a single conversation with messages
 */
export function useConversation(
  conversationId: string | null,
  options?: { enabled?: boolean }
) {
  const query = useQuery<
    ConversationQueryData,
    Error
  >({
    queryKey: chatKeys.conversation(conversationId ?? ""),
    queryFn: () => {
      if (!conversationId) {
        throw new Error("Conversation ID is required");
      }
      return chatApi.getConversation(conversationId);
    },
    enabled: (options?.enabled ?? true) && !!conversationId,
  });

  return query;
}

export function useConversationHistoryWindow(
  conversationId: string | null,
  options?: { enabled?: boolean; pageSize?: number }
) {
  const pageSize = options?.pageSize ?? 40;
  const query = useInfiniteQuery<
    ConversationMessagesPageResponse,
    Error,
    InfiniteData<ConversationMessagesPageResponse>,
    ReturnType<typeof chatKeys.conversationHistory>,
    number
  >({
    queryKey: chatKeys.conversationHistory(conversationId ?? ""),
    queryFn: ({ pageParam }) => {
      if (!conversationId) {
        throw new Error("Conversation ID is required");
      }
      return chatApi.getConversationMessagesPage(
        conversationId,
        pageSize,
        pageParam
      );
    },
    enabled: (options?.enabled ?? true) && !!conversationId,
    initialPageParam: 0,
    getNextPageParam: (lastPage) => {
      if (!lastPage.hasOlder) {
        return undefined;
      }
      return lastPage.offset + lastPage.messages.length;
    },
    staleTime: 30 * 1000,
  });

  const data = useMemo(
    () => getConversationMessagesFromHistoryData(query.data),
    [query.data]
  );

  const fetchOlderMessages = useCallback(async () => {
    if (!query.hasNextPage || query.isFetchingNextPage) {
      return;
    }
    await query.fetchNextPage();
  }, [query]);

  return {
    ...query,
    data,
    loadedStartIndex: data?.loadedStartIndex ?? 0,
    hasOlderMessages: query.hasNextPage ?? false,
    isFetchingOlderMessages: query.isFetchingNextPage,
    fetchOlderMessages,
  };
}

/**
 * Hook to fetch agent run status for a conversation
 */
export function useAgentRunStatus(conversationId: string | null) {
  return useQuery<AgentRun | null, Error>({
    queryKey: chatKeys.agentRun(conversationId ?? ""),
    queryFn: () => {
      if (!conversationId) {
        return null;
      }
      return chatApi.getAgentRunStatus(conversationId);
    },
    enabled: !!conversationId,
    refetchInterval: (query) => {
      // Poll every 2 seconds if agent is running
      const agentRun = query.state.data;
      return agentRun?.status === "running" ? 2000 : false;
    },
    // Prevent excessive refetching when not polling
    staleTime: 10 * 1000, // 10 seconds
    refetchOnWindowFocus: false,
    refetchOnMount: "always", // Always check on mount for initial state
  });
}

/**
 * Hook for chat functionality with context-aware messaging
 *
 * @param context - The chat context
 * @returns Object with messages query, sendMessage mutation, and conversation management
 *
 * @example
 * ```tsx
 * const {
 *   messages,
 *   conversations,
 *   activeConversation,
 *   agentRunStatus,
 *   sendMessage,
 *   switchConversation,
 *   createConversation,
 * } = useChat({
 *   view: "ideation",
 *   projectId: "project-123",
 *   ideationSessionId: "session-123",
 * });
 * ```
 */
export function useChat(
  context: ChatContext,
  options?: {
    isVisible?: boolean;
    storeKey?: string;
    disableAutoSelect?: boolean;
    skipActiveConversationQuery?: boolean;
    sendOptions?: {
      conversationId?: string | null;
      providerHarness?: string | null;
      modelId?: string | null;
      logicalEffort?: string | null;
    };
  }
) {
  const queryClient = useQueryClient();
  const { contextType, contextId } = getContextTypeAndId(context);
  const contextKey = buildStoreKey(contextType, contextId);
  // effectiveStoreKey: caller-provided storeKey takes precedence over the internally derived contextKey.
  // This is critical when IntegratedChatPanel uses execution-mode-aware storeKeys (e.g., "task_execution:id")
  // while chatContext is still view="task_detail" (which would derive "task:id" internally).
  const effectiveStoreKey = options?.storeKey ?? contextKey;
  const disableAutoSelect = options?.disableAutoSelect ?? false;

  const activeConversationId = useChatStore((s) => s.activeConversationIds[effectiveStoreKey] ?? null);
  const setActiveConversation = useChatStore((s) => s.setActiveConversation);
  const setAgentRunning = useChatStore((s) => s.setAgentRunning);
  const setSending = useChatStore((s) => s.setSending);

  // Fetch conversations for this context
  const conversations = useConversations(context);

  // Fetch the active transcript as a newest-message window. The returned
  // message order is chronological inside the loaded window.
  const activeConversation = useConversationHistoryWindow(activeConversationId, {
    enabled: !(options?.skipActiveConversationQuery ?? false),
    pageSize: 40,
  });

  // Fetch agent run status
  const agentRunStatus = useAgentRunStatus(activeConversationId);

  // Update agent running state when status changes
  // NOTE: This only sets to true on initial load (when backend shows agent is running).
  // The false state is handled by the agent:run_completed event (or agent:turn_completed in interactive mode) to avoid race conditions.
  // Track previous contextKey to detect session switches and skip stale recovery
  const prevContextKeyRef = useRef(contextKey);

  const isRunning = agentRunStatus.data?.status === "running";
  const isFailed = agentRunStatus.data?.status === "failed";
  const errorMessage = agentRunStatus.data?.errorMessage;

  useEffect(() => {
    const contextChanged = prevContextKeyRef.current !== effectiveStoreKey;
    prevContextKeyRef.current = effectiveStoreKey;

    // On context change, skip recovery — useChatPanelContext cleanup handles clearing.
    // Without this guard, stale cached isRunning from the old conversation overrides
    // the cleanup and permanently sticks the new session in "agent responding" state.
    if (contextChanged) {
      return;
    }

    // Normal recovery: sync UI with backend state (e.g., page refresh with running agent)
    // Don't set to false here - let the agent:run_completed event (or agent:turn_completed in interactive mode) handle that
    if (isRunning) {
      setAgentRunning(effectiveStoreKey, true);
    }
  }, [effectiveStoreKey, isRunning, setAgentRunning]);

  // Show error toast when a failed run is detected (e.g., when user comes back)
  // Track which errors we've shown to avoid duplicate toasts
  const shownErrorRef = useRef<string | null>(null);
  useEffect(() => {
    if (isFailed && errorMessage && shownErrorRef.current !== agentRunStatus.data?.id) {
      // Mark this error as shown
      shownErrorRef.current = agentRunStatus.data?.id ?? null;

      // Only show toast when panel is visible (prevents duplicate toasts in dual-panel mode)
      if (options?.isVisible === false) return;

      // Import toast dynamically to avoid circular deps
      import("sonner").then(({ toast }) => {
        toast.error("Previous agent run failed", {
          description: errorMessage.slice(0, 200),
          duration: 10000,
        });
      });
    }
  }, [isFailed, errorMessage, agentRunStatus.data?.id, options?.isVisible]);

  // Send message mutation
  const sendMessage = useMutation<SendAgentMessageResult, Error, { content: string; attachmentIds?: string[]; target?: string }>({
    mutationFn: async ({ content, attachmentIds, target }) => {
      if (options?.sendOptions) {
        return chatApi.sendAgentMessage(
          contextType,
          contextId,
          content,
          attachmentIds,
          target,
          options.sendOptions
        );
      }

      return chatApi.sendAgentMessage(
        contextType,
        contextId,
        content,
        attachmentIds,
        target
      );
    },
    onMutate: () => {
      setSending(effectiveStoreKey, true);
    },
    onSettled: () => {
      setSending(effectiveStoreKey, false);
    },
    onSuccess: () => {
      // Invalidate active conversation to refetch messages
      if (activeConversationId) {
        invalidateConversationDataQueries(queryClient, activeConversationId);
      }

      // Invalidate conversations list to update message counts
      queryClient.invalidateQueries({
        queryKey: chatKeys.conversationList(contextType, contextId),
      });

      // If in ideation context, also invalidate session data
      if (context.view === "ideation" && context.ideationSessionId) {
        queryClient.invalidateQueries({
          queryKey: ideationKeys.sessionWithData(context.ideationSessionId),
        });
      }
    },
    onError: () => {
      // Reset agent running state on error
      setAgentRunning(effectiveStoreKey, false);
    },
  });

  // Create new conversation mutation
  const createConversationMutation = useMutation<ChatConversation, Error, void>(
    {
      mutationFn: async () => {
        return chatApi.createConversation(contextType, contextId);
      },
      onSuccess: (newConversation) => {
        // Set as active conversation
        setActiveConversation(effectiveStoreKey, newConversation.id);

        // Invalidate conversations list
        queryClient.invalidateQueries({
          queryKey: chatKeys.conversationList(contextType, contextId),
        });
      },
    }
  );

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
    await createConversationMutation.mutateAsync();
  }, [createConversationMutation]);

  // Subscribe to agent events for real-time updates
  // Pass effectiveStoreKey so setActiveConversation writes to the correct scoped slot.
  useAgentEvents(activeConversationId, effectiveStoreKey);

  // Initialize active conversation if none is set
  // Use a ref to track initialization and prevent infinite loops
  const hasInitializedRef = useRef(false);

  useEffect(() => {
    // Skip auto-select when caller manages active conversation selection externally
    if (disableAutoSelect) return;

    // Only initialize once per context change
    if (hasInitializedRef.current) {
      return;
    }

    if (!activeConversationId && conversations.data && conversations.data.length > 0) {
      // IMPORTANT: Create a copy before sorting to avoid mutating React Query's cached data
      const sorted = [...conversations.data].sort((a, b) => {
        const aTime = a.lastMessageAt || a.createdAt;
        const bTime = b.lastMessageAt || b.createdAt;
        return new Date(bTime).getTime() - new Date(aTime).getTime();
      });
      const mostRecent = sorted[0];

      if (mostRecent) {
        hasInitializedRef.current = true;
        setActiveConversation(effectiveStoreKey, mostRecent.id);
      }
    }
  }, [activeConversationId, conversations.data, setActiveConversation, effectiveStoreKey, disableAutoSelect]);

  // Reset initialization flag when context changes
  useEffect(() => {
    hasInitializedRef.current = false;
  }, [contextType, contextId]);

  return {
    // Messages from active conversation
    messages: activeConversation,
    // All conversations for this context
    conversations,
    // Active conversation data
    activeConversation,
    // Agent run status
    agentRunStatus,
    // Mutations
    sendMessage,
    // Conversation management
    switchConversation,
    createConversation,
    // Effective store key for active conversation operations (caller-provided storeKey or derived contextKey)
    contextKey: effectiveStoreKey,
    // Context info
    contextType,
    contextId,
  };
}
