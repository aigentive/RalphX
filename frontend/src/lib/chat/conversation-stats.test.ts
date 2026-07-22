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

  it("deduplicates only matching composite timeline identities", () => {
    const stats = buildFallbackConversationStats(conversation(), [
      providerMessage({
        id: "block:message-1:0",
        parentMessageId: "message-1",
        timelineSequence: 1,
        providerHarness: "codex",
      }),
      providerMessage({
        id: "block:message-1:0:usage",
        parentMessageId: "message-1",
        timelineSequence: 1,
        providerHarness: "codex",
        inputTokens: 25,
        outputTokens: 5,
        estimatedUsd: 0.01,
      }),
      providerMessage({
        id: "block:message-1:1",
        parentMessageId: "message-1",
        timelineSequence: 2,
        providerHarness: "codex",
        inputTokens: 10,
        outputTokens: 2,
      }),
    ]);

    expect(stats?.usageCoverage.providerMessageCount).toBe(2);
    expect(stats?.usageCoverage.providerMessagesWithUsage).toBe(2);
    expect(stats?.messageUsageTotals).toEqual({
      inputTokens: 35,
      outputTokens: 7,
      cacheCreationTokens: 0,
      cacheReadTokens: 0,
      processedTokens: 42,
      estimatedUsd: 0.01,
    });
    expect(stats?.byHarness).toMatchObject([
      {
        key: "codex",
        count: 2,
      },
    ]);
  });

  it("keeps ordinary parent-linked messages distinct and applies provider token policy", () => {
    const stats = buildFallbackConversationStats(conversation(), [
      providerMessage({
        id: "reply-1",
        parentMessageId: "prompt-1",
        providerHarness: "codex",
        inputTokens: 100,
        outputTokens: 10,
        cacheReadTokens: 90,
        usageProvenance: "provider_turn_delta",
      }),
      providerMessage({
        id: "reply-2",
        parentMessageId: "prompt-1",
        providerHarness: "codex",
        inputTokens: 200,
        outputTokens: 20,
        cacheReadTokens: 180,
        usageProvenance: "provider_turn_delta",
      }),
    ]);

    expect(stats?.usageCoverage.providerMessageCount).toBe(2);
    expect(stats?.effectiveUsageTotals.processedTokens).toBe(330);
    expect(stats?.effectiveUsageTotals.cacheReadTokens).toBe(270);
  });

  it("counts cumulative baselines as uncounted captures in totals and buckets", () => {
    const stats = buildFallbackConversationStats(conversation(), [
      providerMessage({
        id: "baseline",
        usageProvenance: "cumulative_baseline_only",
      }),
      providerMessage({
        id: "fallback",
        providerHarness: "codex",
        inputTokens: 50,
        outputTokens: 5,
        usageProvenance: "provider_snapshot_fallback",
      }),
    ]);

    expect(stats?.effectiveUsageTotals.processedTokens).toBeNull();
    expect(stats?.usageCoverage.providerMessagesWithUsage).toBe(2);
    expect(stats?.usageCoverage.fallbackEstimatedSampleCount).toBe(1);
    expect(stats?.usageCoverage.uncountedSampleCount).toBe(1);
    expect(stats?.byHarness).toMatchObject([
      {
        key: "codex",
        count: 2,
        usage: { processedTokens: null },
      },
    ]);
  });

  it("does not guess processed totals from the conversation harness", () => {
    const stats = buildFallbackConversationStats(conversation(), [
      providerMessage({
        providerHarness: null,
        inputTokens: 100,
        outputTokens: 20,
        usageProvenance: "provider_turn_delta",
      }),
    ]);

    expect(stats?.effectiveUsageTotals.processedTokens).toBeNull();
    expect(stats?.usageCoverage.uncountedSampleCount).toBe(1);
    expect(stats?.usageCoverage.effectiveMessageConversationCount).toBe(0);
    expect(stats?.usageCoverage.effectiveTotalsSource).toBe("none");
  });
});
