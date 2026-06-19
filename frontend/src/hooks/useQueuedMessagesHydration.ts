import { useEffect } from "react";
import { chatApi, type QueuedMessageResponse } from "@/api/chat";
import { useChatStore, type QueuedMessage } from "@/stores/chatStore";
import type { ContextType } from "@/types/chat-conversation";
import { logger } from "@/lib/logger";

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
    attachmentIds: [...(message.attachmentIds ?? [])],
  };
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

    let cancelled = false;
    void chatApi
      .getQueuedAgentMessages(contextType, contextId)
      .then((messages) => {
        if (cancelled) return;
        setQueuedMessages(storeContextKey, messages.map(toQueuedMessage));
      })
      .catch((error) => {
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
