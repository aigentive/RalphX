import type {
  ChatMessageResponse,
  ConversationStatsResponse,
} from "@/api/chat";
import type { ChatConversation } from "@/types/chat-conversation";
import { isProviderRole } from "@/lib/chat/provider-role";

function isProviderMessage(message: ChatMessageResponse): boolean {
  return isProviderRole(message.role);
}

function hasUsage(message: ChatMessageResponse): boolean {
  return (
    message.inputTokens != null ||
    message.outputTokens != null ||
    message.cacheCreationTokens != null ||
    message.cacheReadTokens != null ||
    message.estimatedUsd != null
  );
}

function hasAttribution(message: ChatMessageResponse): boolean {
  return (
    message.providerHarness != null ||
    message.providerSessionId != null ||
    message.upstreamProvider != null ||
    message.providerProfile != null ||
    message.effectiveModelId != null ||
    message.effectiveEffort != null ||
    message.logicalEffort != null
  );
}

function providerMessageIdentity(message: ChatMessageResponse): string {
  if (message.timelineSequence != null && message.parentMessageId) {
    return message.parentMessageId;
  }
  return message.id;
}

function collapseProviderMessageBlocks(
  messages: ChatMessageResponse[],
): ChatMessageResponse[] {
  const byMessage = new Map<string, ChatMessageResponse>();

  for (const message of messages) {
    const key = providerMessageIdentity(message);
    const existing = byMessage.get(key);
    if (!existing) {
      byMessage.set(key, message);
      continue;
    }

    const existingScore =
      (hasUsage(existing) ? 2 : 0) + (hasAttribution(existing) ? 1 : 0);
    const candidateScore =
      (hasUsage(message) ? 2 : 0) + (hasAttribution(message) ? 1 : 0);
    if (candidateScore > existingScore) {
      byMessage.set(key, message);
    }
  }

  return Array.from(byMessage.values());
}

function buildUsageTotals(messages: ChatMessageResponse[]) {
  return messages.reduce(
    (totals, message) => ({
      inputTokens: totals.inputTokens + (message.inputTokens ?? 0),
      outputTokens: totals.outputTokens + (message.outputTokens ?? 0),
      cacheCreationTokens:
        totals.cacheCreationTokens + (message.cacheCreationTokens ?? 0),
      cacheReadTokens: totals.cacheReadTokens + (message.cacheReadTokens ?? 0),
      estimatedUsd:
        totals.estimatedUsd == null && message.estimatedUsd == null
          ? null
          : (totals.estimatedUsd ?? 0) + (message.estimatedUsd ?? 0),
    }),
    {
      inputTokens: 0,
      outputTokens: 0,
      cacheCreationTokens: 0,
      cacheReadTokens: 0,
      estimatedUsd: null as number | null,
    },
  );
}

function buildUsageBuckets(
  messages: ChatMessageResponse[],
  keyFn: (message: ChatMessageResponse) => string | null,
) {
  const buckets = new Map<
    string,
    {
      count: number;
      usage: ReturnType<typeof buildUsageTotals>;
    }
  >();

  for (const message of messages) {
    const key = keyFn(message);
    if (!key) continue;

    const existing = buckets.get(key) ?? {
      count: 0,
      usage: {
        inputTokens: 0,
        outputTokens: 0,
        cacheCreationTokens: 0,
        cacheReadTokens: 0,
        estimatedUsd: null as number | null,
      },
    };

    existing.count += 1;
    existing.usage.inputTokens += message.inputTokens ?? 0;
    existing.usage.outputTokens += message.outputTokens ?? 0;
    existing.usage.cacheCreationTokens += message.cacheCreationTokens ?? 0;
    existing.usage.cacheReadTokens += message.cacheReadTokens ?? 0;
    existing.usage.estimatedUsd =
      existing.usage.estimatedUsd == null && message.estimatedUsd == null
        ? null
        : (existing.usage.estimatedUsd ?? 0) + (message.estimatedUsd ?? 0);
    buckets.set(key, existing);
  }

  return Array.from(buckets.entries())
    .map(([key, value]) => ({
      key,
      count: value.count,
      usage: value.usage,
    }))
    .sort(
      (a, b) =>
        b.usage.inputTokens - a.usage.inputTokens ||
        b.count - a.count ||
        a.key.localeCompare(b.key),
    );
}

export function buildFallbackConversationStats(
  conversation: ChatConversation | null | undefined,
  messages: ChatMessageResponse[] | null | undefined,
): ConversationStatsResponse | null {
  if (!conversation) {
    return null;
  }

  const providerMessages = collapseProviderMessageBlocks(
    (messages ?? []).filter(isProviderMessage),
  );
  const providerMessagesWithUsage = providerMessages.filter(hasUsage);
  const providerMessagesWithAttribution = providerMessages.filter(hasAttribution);
  const effectiveUsageTotals = buildUsageTotals(providerMessagesWithUsage);

  return {
    conversationId: conversation.id,
    contextType: conversation.contextType,
    contextId: conversation.contextId,
    providerHarness: conversation.providerHarness ?? null,
    upstreamProvider: conversation.upstreamProvider ?? null,
    providerProfile: conversation.providerProfile ?? null,
    messageUsageTotals: effectiveUsageTotals,
    runUsageTotals: {
      inputTokens: 0,
      outputTokens: 0,
      cacheCreationTokens: 0,
      cacheReadTokens: 0,
      estimatedUsd: null,
    },
    effectiveUsageTotals,
    usageCoverage: {
      providerMessageCount: providerMessages.length,
      providerMessagesWithUsage: providerMessagesWithUsage.length,
      runCount: 0,
      runsWithUsage: 0,
      effectiveTotalsSource:
        providerMessagesWithUsage.length > 0 ? "messages" : "none",
    },
    attributionCoverage: {
      providerMessageCount: providerMessages.length,
      providerMessagesWithAttribution: providerMessagesWithAttribution.length,
      runCount: 0,
      runsWithAttribution: 0,
    },
    byHarness: buildUsageBuckets(
      providerMessagesWithUsage,
      (message) => message.providerHarness ?? conversation.providerHarness ?? null,
    ),
    byUpstreamProvider: buildUsageBuckets(
      providerMessagesWithUsage,
      (message) =>
        message.upstreamProvider ?? conversation.upstreamProvider ?? null,
    ),
    byModel: buildUsageBuckets(
      providerMessagesWithUsage,
      (message) => message.effectiveModelId ?? null,
    ),
    byEffort: buildUsageBuckets(
      providerMessagesWithUsage,
      (message) => message.effectiveEffort ?? message.logicalEffort ?? null,
    ),
  };
}
