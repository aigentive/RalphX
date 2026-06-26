import { useQuery } from "@tanstack/react-query";

import {
  chatApi,
  type AgentConversationIssue,
} from "@/api/chat";

const AGENT_CONVERSATION_ISSUES_STALE_TIME_MS = 5_000;

export const agentConversationIssueKeys = {
  list: (conversationId: string | null) =>
    ["agents", "conversation-issues", conversationId, "open"] as const,
};

interface UseAgentConversationIssuesOptions {
  enabled?: boolean;
}

export function useAgentConversationIssues(
  conversationId: string | null,
  options: UseAgentConversationIssuesOptions = {},
) {
  const enabled = options.enabled ?? true;
  return useQuery({
    queryKey: agentConversationIssueKeys.list(conversationId),
    queryFn: () => chatApi.listAgentConversationIssues(conversationId!),
    enabled: enabled && Boolean(conversationId),
    staleTime: AGENT_CONVERSATION_ISSUES_STALE_TIME_MS,
  });
}

export function hasOpenAgentConversationIssues(
  issues: readonly AgentConversationIssue[] | null | undefined,
): boolean {
  return (issues?.length ?? 0) > 0;
}
