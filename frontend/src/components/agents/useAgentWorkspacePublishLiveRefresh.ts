import { useEffect, useRef } from "react";
import { useQueryClient, type QueryClient } from "@tanstack/react-query";

import { buildStoreKey } from "@/lib/chat-context-registry";
import { useChatStore } from "@/stores/chatStore";

import { agentWorkspaceKeys } from "./agentWorkspaceQueries";

export const PUBLISH_LIVE_REFRESH_INTERVAL_MS = 2_500;

function invalidatePublishLiveQueries(
  queryClient: QueryClient,
  conversationId: string,
) {
  void queryClient.invalidateQueries({
    queryKey: agentWorkspaceKeys.review(conversationId),
  });
  void queryClient.invalidateQueries({
    queryKey: agentWorkspaceKeys.changeSummary(conversationId),
  });
  void queryClient.invalidateQueries({
    queryKey: agentWorkspaceKeys.diff(conversationId),
  });
  void queryClient.invalidateQueries({
    queryKey: agentWorkspaceKeys.commits(conversationId),
  });
}

/**
 * Keeps an open publish surface live while the owning workspace conversation is
 * generating: bounded polling (invalidation only refetches mounted queries)
 * plus one final refresh when the run settles so terminal commits/PR state land
 * without reopening the tab. Polling stops as soon as the agent goes idle.
 */
export function useAgentWorkspacePublishLiveRefresh(
  conversationId: string | null,
): boolean {
  const queryClient = useQueryClient();
  const storeKey = conversationId
    ? buildStoreKey("project", conversationId)
    : null;
  const isGenerating = useChatStore((state) =>
    storeKey ? (state.agentStatus[storeKey] ?? "idle") === "generating" : false,
  );
  const generatingConversationRef = useRef<string | null>(null);

  useEffect(() => {
    if (!conversationId) {
      return;
    }
    if (!isGenerating) {
      if (generatingConversationRef.current === conversationId) {
        generatingConversationRef.current = null;
        invalidatePublishLiveQueries(queryClient, conversationId);
      }
      return;
    }
    generatingConversationRef.current = conversationId;
    const interval = window.setInterval(() => {
      invalidatePublishLiveQueries(queryClient, conversationId);
    }, PUBLISH_LIVE_REFRESH_INTERVAL_MS);
    return () => window.clearInterval(interval);
  }, [conversationId, isGenerating, queryClient]);

  return isGenerating;
}
