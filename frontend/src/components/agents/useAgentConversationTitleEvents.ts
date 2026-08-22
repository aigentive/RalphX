import { useEffect } from "react";
import { useQueryClient, type InfiniteData, type QueryClient } from "@tanstack/react-query";
import { z } from "zod";

import type {
  ConversationMessagesPageResponse,
  ConversationTimelinePageResponse,
} from "@/api/chat";
import { chatKeys, invalidateConversationDataQueries } from "@/hooks/useChat";
import { useEventBus } from "@/providers/EventProvider";
import type { ChatConversation } from "@/types/chat-conversation";
import { invalidateAgentSidebarConversations } from "@/hooks/agentSidebarConversationKeys";
import { agentConversationKeys } from "./useProjectAgentConversations";

const AgentConversationTitleUpdatedSchema = z.object({
  conversationId: z.string(),
  contextType: z.string(),
  contextId: z.string(),
  title: z.string(),
});

function patchConversationTitleInCache(
  queryClient: QueryClient,
  conversationId: string,
  title: string,
) {
  const updatedAt = new Date().toISOString();
  const patch = (item: ChatConversation): ChatConversation =>
    item.id === conversationId ? { ...item, title, updatedAt } : item;

  queryClient.setQueryData<{ conversation: ChatConversation; messages: unknown[] }>(
    chatKeys.conversation(conversationId),
    (old) => old ? { ...old, conversation: patch(old.conversation) } : old,
  );

  queryClient.setQueryData<ChatConversation | null>(
    chatKeys.conversationSummary(conversationId),
    (old) => old ? patch(old) : old,
  );

  queryClient.setQueryData<InfiniteData<ConversationMessagesPageResponse>>(
    chatKeys.conversationHistory(conversationId),
    (old) =>
      old
        ? { ...old, pages: old.pages.map((p) => ({ ...p, conversation: patch(p.conversation) })) }
        : old,
  );

  queryClient.setQueryData<InfiniteData<ConversationTimelinePageResponse>>(
    chatKeys.conversationTimeline(conversationId),
    (old) =>
      old
        ? { ...old, pages: old.pages.map((p) => ({ ...p, conversation: patch(p.conversation) })) }
        : old,
  );

  for (const query of queryClient.getQueryCache().findAll({ queryKey: agentConversationKeys.all })) {
    queryClient.setQueryData(query.queryKey, (old: unknown) => {
      if (!old || typeof old !== "object") return old;
      const data = old as { pages?: Array<{ conversations?: ChatConversation[] }> };
      if (!Array.isArray(data.pages)) return old;
      return {
        ...data,
        pages: data.pages.map((page) => ({
          ...page,
          conversations: page.conversations?.map(patch),
        })),
      };
    });
  }
}

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
      const { conversationId, title } = parsed.data;
      patchConversationTitleInCache(queryClient, conversationId, title);
      invalidateConversationDataQueries(queryClient, conversationId);
      void queryClient.invalidateQueries({
        queryKey: agentConversationKeys.project(projectId),
      });
      void invalidateAgentSidebarConversations(queryClient);
    });
  }, [bus, projectId, queryClient]);
}
