import { describe, expect, it } from "vitest";
import type { ChatMessageResponse } from "@/api/chat";
import type { ChatConversation } from "@/types/chat-conversation";
import { buildFallbackConversationStats } from "./conversation-stats";

const createdAt = "2026-05-10T12:00:00.000Z";

function conversation(overrides: Partial<ChatConversation> = {}): ChatConversation {
  return {
    id: "conversation-1",
    contextType: "ideation",
    contextId: "project-1",
    claudeSessionId: null,
    providerSessionId: "thread-1",
    providerHarness: "codex",
    upstreamProvider: "openai",
    providerProfile: "default",
    agentMode: "ideation",
    title: "Conversation",
    messageCount: 2,
    lastMessageAt: createdAt,
    createdAt,
    updatedAt: createdAt,
    archivedAt: null,
    ...overrides,
  };
}

function providerMessage(
  overrides: Partial<ChatMessageResponse> = {},
): ChatMessageResponse {
  return {
    id: "message-1",
    sessionId: null,
    projectId: null,
    taskId: null,
    role: "orchestrator",
    content: "",
    metadata: null,
    parentMessageId: null,
    conversationId: "conversation-1",
    toolCalls: null,
    contentBlocks: null,
    sender: null,
    createdAt,
    ...overrides,
  };
}

describe("buildFallbackConversationStats", () => {
  it("returns null without a conversation", () => {
    expect(buildFallbackConversationStats(null, [])).toBeNull();
  });

  it("deduplicates normalized timeline blocks before summing provider usage", () => {
    const stats = buildFallbackConversationStats(conversation(), [
      providerMessage({
        id: "block:message-1:0",
        parentMessageId: "message-1",
        timelineSequence: 1,
        providerHarness: "codex",
      }),
      providerMessage({
        id: "block:message-1:1",
        parentMessageId: "message-1",
        timelineSequence: 2,
        inputTokens: 25,
        outputTokens: 5,
        estimatedUsd: 0.01,
      }),
    ]);

    expect(stats?.usageCoverage.providerMessageCount).toBe(1);
    expect(stats?.usageCoverage.providerMessagesWithUsage).toBe(1);
    expect(stats?.messageUsageTotals).toEqual({
      inputTokens: 25,
      outputTokens: 5,
      cacheCreationTokens: 0,
      cacheReadTokens: 0,
      estimatedUsd: 0.01,
    });
    expect(stats?.byHarness).toMatchObject([
      {
        key: "codex",
        count: 1,
      },
    ]);
  });
});
