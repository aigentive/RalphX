import { useEffect } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { z } from "zod";

import { invalidateConversationDataQueries } from "@/hooks/useChat";
import { useEventBus } from "@/providers/EventProvider";
import { agentSidebarConversationKeys } from "./useAgentSidebarPublicationGroup";
import { agentConversationKeys } from "./useProjectAgentConversations";

const AgentConversationTitleUpdatedSchema = z.object({
  conversationId: z.string(),
  contextType: z.string(),
  contextId: z.string(),
  title: z.string(),
});

export function useAgentConversationTitleEvents(projectId: string | null | undefined) {
  const bus = useEventBus();
  const queryClient = useQueryClient();

  useEffect(() => {
    if (!projectId) {
      return;
    }

    return bus.subscribe<unknown>("agent:conversation_title_updated", (payload) => {
      const parsed = AgentConversationTitleUpdatedSchema.safeParse(payload);
      if (!parsed.success) {
        return;
      }
      if (
        parsed.data.contextType !== "project" ||
        parsed.data.contextId !== projectId
      ) {
        return;
      }

      invalidateConversationDataQueries(queryClient, parsed.data.conversationId);
      void queryClient.invalidateQueries({
        queryKey: agentConversationKeys.project(projectId),
      });
      void queryClient.invalidateQueries({
        queryKey: agentSidebarConversationKeys.all,
      });
    });
  }, [bus, projectId, queryClient]);
}
