import type { InfiniteData } from "@tanstack/react-query";
import type {
  ChatMessageResponse,
  ConversationMessagesPageResponse,
} from "@/api/chat";

export type ConversationHistoryCacheData = InfiniteData<ConversationMessagesPageResponse>;

export function createOptimisticUserMessage({
  conversationId,
  content,
  createdAt = new Date().toISOString(),
}: {
  conversationId: string;
  content: string;
  createdAt?: string;
}): ChatMessageResponse {
  return {
    id: `optimistic:${conversationId}:${createdAt}:${Math.random().toString(36).slice(2)}`,
    conversationId,
    sessionId: null,
    projectId: null,
    taskId: null,
    role: "user",
    content,
    metadata: null,
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

  const optimisticIndex = messages.findIndex(
    (item) =>
      item.id.startsWith("optimistic:") &&
      item.conversationId === message.conversationId &&
      item.role === message.role &&
      item.content === message.content
  );

  if (optimisticIndex === -1) {
    return [...messages, message];
  }

  const nextMessages = [...messages];
  nextMessages[optimisticIndex] = message;
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
