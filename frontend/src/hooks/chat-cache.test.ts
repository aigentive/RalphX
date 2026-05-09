import type { InfiniteData } from "@tanstack/react-query";
import { describe, expect, it } from "vitest";
import type {
  ChatMessageResponse,
  ConversationMessagesPageResponse,
} from "@/api/chat";
import type { ChatConversation } from "@/types/chat-conversation";
import {
  appendMessageIfMissing,
  appendMessageToConversationHistory,
  createOptimisticUserMessage,
  removeMessageFromConversationHistory,
  replaceMatchingOptimisticMessage,
} from "./chat-cache";

const conversation: ChatConversation = {
  id: "conv-1",
  contextType: "ideation",
  contextId: "session-1",
  providerSessionId: null,
  providerHarness: null,
  claudeSessionId: null,
  title: "Conversation",
  messageCount: 1,
  lastMessageAt: "2026-01-01T00:00:00Z",
  createdAt: "2026-01-01T00:00:00Z",
  updatedAt: "2026-01-01T00:00:00Z",
};

function message(overrides: Partial<ChatMessageResponse> = {}): ChatMessageResponse {
  return {
    id: "message-1",
    conversationId: "conv-1",
    sessionId: null,
    projectId: null,
    taskId: null,
    role: "user",
    content: "hello",
    metadata: null,
    parentMessageId: null,
    createdAt: "2026-01-01T00:00:00Z",
    toolCalls: null,
    contentBlocks: null,
    sender: null,
    ...overrides,
  };
}

function history(
  pages: ConversationMessagesPageResponse[]
): InfiniteData<ConversationMessagesPageResponse> {
  return {
    pages,
    pageParams: pages.map((page) => page.offset),
  };
}

function page(
  messages: ChatMessageResponse[],
  overrides: Partial<ConversationMessagesPageResponse> = {}
): ConversationMessagesPageResponse {
  return {
    conversation,
    messages,
    limit: 40,
    offset: 0,
    totalMessageCount: messages.length,
    hasOlder: false,
    ...overrides,
  };
}

describe("chat-cache helpers", () => {
  it("creates fully-shaped optimistic user messages", () => {
    const optimistic = createOptimisticUserMessage({
      conversationId: "conv-1",
      content: "queued locally",
      createdAt: "2026-02-03T04:05:06Z",
    });

    expect(optimistic).toMatchObject({
      conversationId: "conv-1",
      role: "user",
      content: "queued locally",
      createdAt: "2026-02-03T04:05:06Z",
      sessionId: null,
      toolCalls: null,
      contentBlocks: null,
      sender: null,
    });
    expect(optimistic.id).toMatch(/^optimistic:conv-1:2026-02-03T04:05:06Z:/);
  });

  it("replaces a matching optimistic message when the backend message arrives", () => {
    const optimistic = message({
      id: "optimistic:conv-1:pending",
      content: "same text",
    });
    const backend = message({
      id: "message-backend",
      content: "same text",
      createdAt: "2026-01-01T00:00:05Z",
    });

    expect(replaceMatchingOptimisticMessage([optimistic], backend)).toEqual([backend]);
  });

  it("keeps existing messages unchanged when the incoming id is already present", () => {
    const existing = message({ id: "message-existing" });
    const messages = [existing];

    expect(replaceMatchingOptimisticMessage(messages, existing)).toBe(messages);
    expect(appendMessageIfMissing(messages, existing)).toBe(messages);
  });

  it("appends messages when no duplicate or optimistic match exists", () => {
    const existing = message({ id: "message-existing" });
    const incoming = message({ id: "message-incoming", content: "new text" });

    expect(replaceMatchingOptimisticMessage([existing], incoming)).toEqual([
      existing,
      incoming,
    ]);
    expect(appendMessageIfMissing([existing], incoming)).toEqual([
      existing,
      incoming,
    ]);
  });

  it("appends to the newest history page and leaves older pages intact", () => {
    const existing = message({ id: "message-existing" });
    const incoming = message({ id: "message-incoming", content: "next" });
    const older = page([message({ id: "message-older" })], { offset: 40 });
    const data = history([page([existing]), older]);

    const updated = appendMessageToConversationHistory(data, incoming);

    expect(updated?.pages[0]?.messages).toEqual([existing, incoming]);
    expect(updated?.pages[0]?.totalMessageCount).toBe(2);
    expect(updated?.pages[1]).toBe(older);
  });

  it("does not append duplicate history messages", () => {
    const existing = message({ id: "message-existing" });
    const data = history([page([existing])]);

    expect(appendMessageToConversationHistory(data, existing)).toBe(data);
  });

  it("supports optimistic history appends without content-based replacement", () => {
    const first = message({
      id: "optimistic:conv-1:first",
      content: "repeat",
    });
    const second = message({
      id: "optimistic:conv-1:second",
      content: "repeat",
    });
    const data = history([page([first])]);

    const updated = appendMessageToConversationHistory(data, second, {
      replaceOptimistic: false,
    });

    expect(updated?.pages[0]?.messages).toEqual([first, second]);
    expect(updated?.pages[0]?.totalMessageCount).toBe(2);
  });

  it("returns empty or malformed history data unchanged", () => {
    const incoming = message({ id: "message-incoming" });
    const emptyHistory = history([]);

    expect(appendMessageToConversationHistory(undefined, incoming)).toBeUndefined();
    expect(appendMessageToConversationHistory(emptyHistory, incoming)).toBe(emptyHistory);
    expect(removeMessageFromConversationHistory(undefined, incoming.id)).toBeUndefined();
    expect(removeMessageFromConversationHistory(emptyHistory, incoming.id)).toBe(emptyHistory);
  });

  it("removes messages from history and decrements affected page totals", () => {
    const keep = message({ id: "message-keep" });
    const remove = message({ id: "message-remove" });
    const older = page([message({ id: "message-older" })], { offset: 40 });
    const data = history([page([keep, remove]), older]);

    const updated = removeMessageFromConversationHistory(data, "message-remove");

    expect(updated?.pages[0]?.messages).toEqual([keep]);
    expect(updated?.pages[0]?.totalMessageCount).toBe(1);
    expect(updated?.pages[1]).toBe(older);
  });

  it("returns original history when removal does not find a matching message", () => {
    const data = history([page([message({ id: "message-existing" })])]);

    expect(removeMessageFromConversationHistory(data, "missing")).toBe(data);
  });
});
