import type { InfiniteData } from "@tanstack/react-query";
import type {
  ChatMessageResponse,
  ConversationMessagesPageResponse,
} from "@/api/chat";

export type ConversationHistoryCacheData = InfiniteData<ConversationMessagesPageResponse>;

const OPTIMISTIC_MESSAGE_ID_PREFIX = "optimistic:";

type OptimisticMatchCandidate = {
  id: string;
  conversationId?: string | null;
  role: string;
  content: string;
};

/**
 * Matches an optimistic row against the message that just came back from the
 * backend. Optimistic ids are client-generated, so the backend id can never
 * identify the row it replaces — content identity is the only available key.
 */
export function matchesOptimisticMessage(
  candidate: OptimisticMatchCandidate,
  message: Pick<ChatMessageResponse, "conversationId" | "role" | "content">
): boolean {
  return (
    candidate.id.startsWith(OPTIMISTIC_MESSAGE_ID_PREFIX) &&
    (candidate.conversationId ?? null) === (message.conversationId ?? null) &&
    candidate.role === message.role &&
    candidate.content === message.content
  );
}

export function createOptimisticUserMessage({
  conversationId,
  content,
  metadata = null,
  createdAt = new Date().toISOString(),
}: {
  conversationId: string;
  content: string;
  metadata?: string | null;
  createdAt?: string;
}): ChatMessageResponse {
  return {
    id: `${OPTIMISTIC_MESSAGE_ID_PREFIX}${conversationId}:${createdAt}:${Math.random().toString(36).slice(2)}`,
    conversationId,
    sessionId: null,
    projectId: null,
    taskId: null,
    role: "user",
    content,
    metadata,
    parentMessageId: null,
    createdAt,
    toolCalls: null,
    contentBlocks: null,
    sender: null,
  };
}

export function replaceMatchingOptimisticMessage(
  messages: ChatMessageResponse[],
  message: ChatMessageResponse
) {
  if (messages.some((item) => item.id === message.id)) {
    return messages;
  }

  const optimisticIndex = messages.findIndex((item) =>
    matchesOptimisticMessage(item, message)
  );

  if (optimisticIndex === -1) {
    return [...messages, message];
  }

  const nextMessages = [...messages];
  const optimisticMessage = messages[optimisticIndex];
  nextMessages[optimisticIndex] = {
    ...message,
    ...(!message.attachments && optimisticMessage?.attachments
      ? { attachments: optimisticMessage.attachments }
      : {}),
    ...(!message.metadata && optimisticMessage?.metadata
      ? { metadata: optimisticMessage.metadata }
      : {}),
  };
  return nextMessages;
}

export function appendMessageIfMissing(
  messages: ChatMessageResponse[],
  message: ChatMessageResponse
) {
  if (messages.some((item) => item.id === message.id)) {
    return messages;
  }

  return [...messages, message];
}

export function appendMessageToConversationHistory(
  data: ConversationHistoryCacheData | undefined,
  message: ChatMessageResponse,
  options: { replaceOptimistic?: boolean } = {}
): ConversationHistoryCacheData | undefined {
  if (!data || !Array.isArray(data.pages) || data.pages.length === 0) {
    return data;
  }

  if (data.pages.some((page) => page.messages.some((item) => item.id === message.id))) {
    return data;
  }

  return {
    ...data,
    pages: data.pages.map((page, index) => {
      if (index !== 0) {
        return page;
      }
      const messages = options.replaceOptimistic === false
        ? appendMessageIfMissing(page.messages, message)
        : replaceMatchingOptimisticMessage(page.messages, message);
      return {
        ...page,
        messages,
        totalMessageCount:
          page.totalMessageCount + (messages.length > page.messages.length ? 1 : 0),
      };
    }),
  };
}

export function removeMessageFromConversationHistory(
  data: ConversationHistoryCacheData | undefined,
  messageId: string
): ConversationHistoryCacheData | undefined {
  if (!data || !Array.isArray(data.pages) || data.pages.length === 0) {
    return data;
  }

  let removed = false;
  const pages = data.pages.map((page) => {
    const messages = page.messages.filter((message) => message.id !== messageId);
    if (messages.length === page.messages.length) {
      return page;
    }
    removed = true;
    return {
      ...page,
      messages,
      totalMessageCount: Math.max(0, page.totalMessageCount - 1),
    };
  });

  return removed ? { ...data, pages } : data;
}
