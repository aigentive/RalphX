import { useQuery } from "@tanstack/react-query";

import { chatApi } from "@/api/chat";

import {
  AGENT_WORKSPACE_FRESHNESS_STALE_MS,
  agentWorkspaceKeys,
} from "./agentWorkspaceQueries";

interface AgentWorkspaceFullFreshnessOptions {
  enabled?: boolean | undefined;
}

/** Shared owner for the authoritative remote + worktree freshness read. */
export function useAgentWorkspaceFullFreshness(
  conversationId: string | null | undefined,
  options: AgentWorkspaceFullFreshnessOptions = {},
) {
  return useQuery({
    queryKey: agentWorkspaceKeys.scopedFreshness(conversationId, "full"),
    queryFn: () =>
      chatApi.getAgentConversationWorkspaceFreshness(conversationId!, {
        scope: "full",
      }),
    enabled: (options.enabled ?? true) && Boolean(conversationId),
    staleTime: AGENT_WORKSPACE_FRESHNESS_STALE_MS,
  });
}
