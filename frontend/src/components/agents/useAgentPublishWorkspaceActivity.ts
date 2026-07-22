import { useEffect, useRef } from "react";
import {
  type QueryClient,
  type QueryKey,
  useQuery,
  useQueryClient,
} from "@tanstack/react-query";

import { diffApi } from "@/api/diff";
import type { AgentConversationRuntimeStatus } from "@/api/chat";

import {
  ACTIVE_AGENT_WORKSPACE_REFRESH_MS,
  AGENT_WORKSPACE_STALE_MS,
  agentWorkspaceKeys,
} from "./agentWorkspaceQueries";
import { useAgentConversationRuntimeStatus } from "./useAgentConversationRuntimeStatus";

interface AgentPublishWorkspaceActivityOptions {
  conversationId: string | null;
  reviewEnabled: boolean;
  liveRefreshEnabled: boolean;
}

interface PreviousRunActivity {
  conversationId: string;
  isRunActive: boolean;
}

function hasActiveRuntime(
  status: AgentConversationRuntimeStatus | null | undefined,
): boolean {
  return Boolean(
    status?.isRunning ||
      status?.agentStatus === "generating" ||
      status?.items.some((item) => item.agentStatus === "generating"),
  );
}

async function refetchAfterCurrentRequest(
  queryClient: QueryClient,
  queryKey: QueryKey,
): Promise<void> {
  const hadRequestInFlight = queryClient.isFetching({ queryKey }) > 0;
  await queryClient.refetchQueries(
    { queryKey, type: "active" },
    { cancelRefetch: false },
  );
  if (hadRequestInFlight) {
    await queryClient.refetchQueries(
      { queryKey, type: "active" },
      { cancelRefetch: false },
    );
  }
}

async function settleAgentPublishWorkspaceActivity(
  queryClient: QueryClient,
  conversationId: string,
): Promise<void> {
  await Promise.all([
    refetchAfterCurrentRequest(
      queryClient,
      agentWorkspaceKeys.workspace(conversationId),
    ),
    refetchAfterCurrentRequest(
      queryClient,
      agentWorkspaceKeys.publicationEvents(conversationId),
    ),
    refetchAfterCurrentRequest(
      queryClient,
      agentWorkspaceKeys.review(conversationId),
    ),
    refetchAfterCurrentRequest(
      queryClient,
      agentWorkspaceKeys.changeSummary(conversationId),
    ),
    refetchAfterCurrentRequest(
      queryClient,
      agentWorkspaceKeys.diff(conversationId),
    ),
    refetchAfterCurrentRequest(
      queryClient,
      agentWorkspaceKeys.commits(conversationId),
    ),
  ]);
}

export function useAgentPublishWorkspaceActivity({
  conversationId,
  reviewEnabled,
  liveRefreshEnabled,
}: AgentPublishWorkspaceActivityOptions) {
  const queryClient = useQueryClient();
  const previousActivityRef = useRef<PreviousRunActivity | null>(null);
  const runtimeStatusQuery = useAgentConversationRuntimeStatus(conversationId, {
    enabled: Boolean(conversationId && liveRefreshEnabled),
    mirrorToVisibleChatStatus: false,
  });
  const isRunActive =
    liveRefreshEnabled && hasActiveRuntime(runtimeStatusQuery.data);
  const reviewQuery = useQuery({
    queryKey: agentWorkspaceKeys.review(conversationId),
    queryFn: () => diffApi.getAgentConversationWorkspaceReview(conversationId!),
    enabled: Boolean(conversationId && reviewEnabled),
    staleTime: 2_000,
    refetchInterval: isRunActive ? ACTIVE_AGENT_WORKSPACE_REFRESH_MS : false,
  });
  const changeSummaryQuery = useQuery({
    queryKey: agentWorkspaceKeys.changeSummary(conversationId),
    queryFn: () =>
      diffApi.getAgentConversationWorkspaceChangeSummary(conversationId!),
    enabled: Boolean(conversationId && liveRefreshEnabled),
    staleTime: AGENT_WORKSPACE_STALE_MS,
    refetchInterval: isRunActive ? ACTIVE_AGENT_WORKSPACE_REFRESH_MS : false,
  });

  useEffect(() => {
    if (!conversationId) {
      previousActivityRef.current = null;
      return;
    }

    const previousActivity = previousActivityRef.current;
    previousActivityRef.current = { conversationId, isRunActive };
    if (
      previousActivity?.conversationId !== conversationId ||
      !previousActivity.isRunActive ||
      isRunActive
    ) {
      return;
    }

    void settleAgentPublishWorkspaceActivity(queryClient, conversationId);
  }, [conversationId, isRunActive, queryClient]);

  return {
    changeSummaryQuery,
    isRunActive,
    reviewQuery,
    runtimeStatusQuery,
  };
}
