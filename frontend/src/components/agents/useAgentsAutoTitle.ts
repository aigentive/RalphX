import { useCallback, useRef } from "react";

import { chatApi } from "@/api/chat";
import { ideationApi } from "@/api/ideation";

import type { AgentConversation } from "./agentConversations";
import {
  deriveAgentTitleFromMessages,
  isDefaultAgentTitle,
} from "./agentTitle";

interface UseAgentsAutoTitleArgs {
  findConversationById: (conversationId: string) => AgentConversation | null;
  invalidateProjectConversations: (targetProjectId: string) => Promise<unknown>;
}

export function useAgentsAutoTitle({
  findConversationById,
  invalidateProjectConversations,
}: UseAgentsAutoTitleArgs) {
  const autoTitleStateRef = useRef<
    Map<string, { messages: string[]; lastTitle: string | null }>
  >(new Map());
  const handleAutoManagedTitle = useCallback(
    ({
      content,
      conversationId,
      targetProjectId,
      shouldSpawnSessionNamer,
      providerHarness,
    }: {
      content: string;
      conversationId: string;
      targetProjectId: string | null;
      shouldSpawnSessionNamer: boolean;
      providerHarness?: string | null;
    }) => {
      const conversation = findConversationById(conversationId);
      const titleIsAutoManaged =
        isDefaultAgentTitle(conversation?.title) ||
        autoTitleStateRef.current.get(conversationId)?.lastTitle === conversation?.title;
      if (!titleIsAutoManaged) {
        return;
      }

      const state = autoTitleStateRef.current.get(conversationId) ?? {
        messages: [],
        lastTitle: null,
      };
      const isFirstTrackedMessage = state.messages.length === 0;
      if (shouldSpawnSessionNamer && isFirstTrackedMessage) {
        const namerProviderHarness = providerHarness ?? conversation?.providerHarness ?? null;
        void chatApi
          .spawnConversationSessionNamer(conversationId, content, namerProviderHarness)
          .catch(() => {
            // Session namer is best-effort; local auto-titling remains as fallback.
          });
      }

      if (state.messages.length >= 3) {
        return;
      }

      state.messages = [...state.messages, content].slice(0, 3);
      const nextTitle = deriveAgentTitleFromMessages(state.messages);
      if (!nextTitle || nextTitle === conversation?.title || nextTitle === state.lastTitle) {
        autoTitleStateRef.current.set(conversationId, state);
        return;
      }

      state.lastTitle = nextTitle;
      autoTitleStateRef.current.set(conversationId, state);
      const titleUpdate =
        conversation?.contextType === "ideation"
          ? Promise.all([
              chatApi.updateConversationTitle(conversationId, nextTitle),
              ideationApi.sessions.updateTitle(conversation.contextId, nextTitle),
            ])
          : chatApi.updateConversationTitle(conversationId, nextTitle);
      void titleUpdate
        .then(() => {
          const projectId = conversation?.projectId ?? targetProjectId;
          if (projectId) {
            void invalidateProjectConversations(projectId);
          }
        })
        .catch(() => {
          // Auto-titling is best-effort; manual title editing remains available.
        });
    },
    [findConversationById, invalidateProjectConversations]
  );

  const clearAutoManagedTitle = useCallback((conversationId: string) => {
    autoTitleStateRef.current.delete(conversationId);
  }, []);

  return {
    clearAutoManagedTitle,
    handleAutoManagedTitle,
  };
}
