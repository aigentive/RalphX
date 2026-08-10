import { useEffect } from "react";
import { chatApi, type QueuedMessageResponse } from "@/api/chat";
import { useChatStore, type QueuedMessage } from "@/stores/chatStore";
import type { ContextType } from "@/types/chat-conversation";
import { logger } from "@/lib/logger";
import {
  getTransportEnvironmentId,
  isRemoteEnvironmentId,
} from "@/lib/remote/active-environment";

interface UseQueuedMessagesHydrationOptions {
  contextType: ContextType;
  contextId: string;
  storeContextKey: string;
  enabled?: boolean;
}

function toQueuedMessage(message: QueuedMessageResponse): QueuedMessage {
  return {
    id: message.id,
    content: message.content,
    createdAt: message.createdAt,
    isEditing: message.isEditing,
    source: "backend",
    attachmentIds: [...(message.attachmentIds ?? [])],
    ...(message.composerSelectionSnapshot
      ? { composerSelectionSnapshot: message.composerSelectionSnapshot }
      : {}),
  };
}

export function loadQueuedMessagesForHydration(
  contextType: ContextType,
  contextId: string,
): Promise<QueuedMessageResponse[]> {
  if (isRemoteEnvironmentId(getTransportEnvironmentId())) {
    return chatApi.listRemoteQueuedAgentMessages(contextId);
  }
  return chatApi.getQueuedAgentMessages(contextType, contextId);
}

export function useQueuedMessagesHydration({
  contextType,
  contextId,
  storeContextKey,
  enabled = true,
}: UseQueuedMessagesHydrationOptions) {
  const setQueuedMessages = useChatStore((state) => state.setQueuedMessages);

  useEffect(() => {
    if (!enabled || !contextId || !storeContextKey) {
      return;
    }
    const isRemote = isRemoteEnvironmentId(getTransportEnvironmentId());
    const getQueuedAgentMessages = loadQueuedMessagesForHydration;
    if (typeof getQueuedAgentMessages !== "function") {
      logger.debug?.("[chat] Skipping queued message hydration; API is unavailable", {
        contextType,
        contextId,
        storeContextKey,
      });
      return;
    }

    let cancelled = false;
    void getQueuedAgentMessages(contextType, contextId)
      .then((messages) => {
        if (cancelled) return;
        setQueuedMessages(storeContextKey, messages.map(toQueuedMessage));
      })
      .catch((error) => {
        if (isRemote) {
          throw error;
        }
        logger.debug?.("[chat] Failed to hydrate queued messages", {
          contextType,
          contextId,
          storeContextKey,
          error,
        });
      });

    return () => {
      cancelled = true;
    };
  }, [contextType, contextId, enabled, setQueuedMessages, storeContextKey]);
}
