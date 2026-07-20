import { useCallback } from "react";
import type { QueryClient } from "@tanstack/react-query";

import { attentionKeys } from "@/hooks/useAttentionItems";
import { chatKeys } from "@/hooks/useChat";
import { ideationKeys } from "@/hooks/useIdeation";
import { notificationKeys } from "@/hooks/useNotificationHistory";

import { archivedConversationCountKey } from "./useArchivedConversationCounts";
import {
  agentConversationKeys,
} from "./useProjectAgentConversations";
import { agentSidebarConversationKeys } from "./useAgentSidebarPublicationGroup";

export function useAgentConversationInvalidation(queryClient: QueryClient) {
  return useCallback(
    async (targetProjectId: string) => {
      await Promise.all([
        queryClient.invalidateQueries({
          queryKey: agentConversationKeys.project(targetProjectId),
        }),
        queryClient.invalidateQueries({
          queryKey: agentSidebarConversationKeys.all,
        }),
        queryClient.invalidateQueries({
          queryKey: chatKeys.conversationList("project", targetProjectId),
        }),
        queryClient.invalidateQueries({
          queryKey: archivedConversationCountKey(targetProjectId),
          refetchType: "active",
        }),
        queryClient.invalidateQueries({ queryKey: ideationKeys.sessions() }),
        queryClient.invalidateQueries({ queryKey: attentionKeys.all }),
        queryClient.invalidateQueries({ queryKey: notificationKeys.all }),
      ]);
    },
    [queryClient]
  );
}
