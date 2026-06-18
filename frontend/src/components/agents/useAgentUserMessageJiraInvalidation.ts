import { useCallback } from "react";
import type { QueryClient } from "@tanstack/react-query";

import { invalidateAgentConversationJiraIssue } from "./agentJiraIssueQueries";

interface UseAgentUserMessageJiraInvalidationArgs {
  queryClient: QueryClient;
  selectedConversationId: string | null;
}

interface AgentUserMessageSentEvent {
  content: string;
  result: {
    conversationId: string;
  };
}

export function useAgentUserMessageJiraInvalidation({
  queryClient,
  selectedConversationId,
}: UseAgentUserMessageJiraInvalidationArgs) {
  return useCallback(
    ({ result }: AgentUserMessageSentEvent) => {
      const conversationId = result.conversationId || selectedConversationId;
      if (!conversationId) {
        return;
      }
      void invalidateAgentConversationJiraIssue(queryClient, conversationId);
    },
    [queryClient, selectedConversationId],
  );
}
