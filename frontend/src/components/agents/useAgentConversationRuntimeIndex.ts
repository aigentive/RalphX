import { useQuery } from "@tanstack/react-query";

import { chatApi, type AgentConversationRuntimeIndexResponse } from "@/api/chat";

export const agentConversationRuntimeIndexKeys = {
  all: ["agents", "conversation-runtime-index"] as const,
  detail: (conversationId: string | null | undefined) =>
    [...agentConversationRuntimeIndexKeys.all, conversationId ?? "none"] as const,
};

interface UseAgentConversationRuntimeIndexOptions {
  enabled?: boolean;
}

function hasActiveRuntimeRow(
  index: AgentConversationRuntimeIndexResponse | null | undefined,
) {
  return (
    index?.rows.some((row) =>
      row.lifecycle === "running" ||
      row.lifecycle === "waiting" ||
      row.lifecycle === "queued",
    ) ?? false
  );
}

export function useAgentConversationRuntimeIndex(
  conversationId: string | null | undefined,
  options: UseAgentConversationRuntimeIndexOptions = {},
) {
  const enabled = Boolean(conversationId) && (options.enabled ?? true);

  return useQuery<AgentConversationRuntimeIndexResponse | null, Error>({
    queryKey: agentConversationRuntimeIndexKeys.detail(conversationId),
    queryFn: async () => {
      if (!conversationId) return null;
      return chatApi.getAgentConversationRuntimeIndex(conversationId);
    },
    enabled,
    staleTime: 2_000,
    refetchInterval: (query) =>
      hasActiveRuntimeRow(query.state.data) ? 5_000 : false,
    refetchOnWindowFocus: enabled,
  });
}
