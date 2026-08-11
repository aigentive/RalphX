import type {
  ChatMessageResponse,
  ConversationStatsResponse,
} from "@/api/chat";
import type { ChatConversation } from "@/types/chat-conversation";
import { isProviderRole } from "@/lib/chat/provider-role";

function isProviderMessage(message: ChatMessageResponse): boolean {
  return isProviderRole(message.role);
}

function hasUsageCapture(message: ChatMessageResponse): boolean {
  return (
    message.usageProvenance != null ||
    message.inputTokens != null ||
    message.outputTokens != null ||
    message.cacheCreationTokens != null ||
    message.cacheReadTokens != null ||
    message.estimatedUsd != null
  );
}

function processedTokensForMessage(
  message: ChatMessageResponse,
): number | null {
  if (message.usageProvenance === "cumulative_baseline_only") {
    return null;
  }
  if (
    message.inputTokens == null &&
    message.outputTokens == null &&
    message.cacheCreationTokens == null &&
    message.cacheReadTokens == null
  ) {
    return null;
  }

  const harness = message.providerHarness;
  const base = (message.inputTokens ?? 0) + (message.outputTokens ?? 0);
  const processed = harness === "codex"
    ? base
    : harness === "claude"
      ? base + (message.cacheCreationTokens ?? 0) + (message.cacheReadTokens ?? 0)
      : null;
  return processed != null && Number.isSafeInteger(processed) ? processed : null;
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
    return `${message.parentMessageId}:${message.timelineSequence}`;
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
      (hasUsageCapture(existing) ? 2 : 0) + (hasAttribution(existing) ? 1 : 0);
    const candidateScore =
      (hasUsageCapture(message) ? 2 : 0) + (hasAttribution(message) ? 1 : 0);
    if (candidateScore > existingScore) {
      byMessage.set(key, message);
    }
  }

  return Array.from(byMessage.values());
}

function buildUsageTotals(
  messages: ChatMessageResponse[],
  hasUncountedSample = false,
) {
  let processedTokens = 0;
  let processedAvailable = messages.length > 0 && !hasUncountedSample;
  const totals = messages.reduce(
    (current, message) => {
      const sampleProcessed = processedTokensForMessage(message);
      if (sampleProcessed == null || !Number.isSafeInteger(processedTokens + sampleProcessed)) {
        processedAvailable = false;
      } else {
        processedTokens += sampleProcessed;
      }
      return {
        inputTokens: current.inputTokens + (message.inputTokens ?? 0),
        outputTokens: current.outputTokens + (message.outputTokens ?? 0),
        cacheCreationTokens:
          current.cacheCreationTokens + (message.cacheCreationTokens ?? 0),
        cacheReadTokens: current.cacheReadTokens + (message.cacheReadTokens ?? 0),
        estimatedUsd:
          current.estimatedUsd == null && message.estimatedUsd == null
            ? null
            : (current.estimatedUsd ?? 0) + (message.estimatedUsd ?? 0),
      };
    },
    {
      inputTokens: 0,
      outputTokens: 0,
      cacheCreationTokens: 0,
      cacheReadTokens: 0,
      estimatedUsd: null as number | null,
    },
  );
  return {
    ...totals,
    processedTokens: processedAvailable ? processedTokens : null,
  };
}

function buildUsageBuckets(
  messages: ChatMessageResponse[],
  keyFn: (message: ChatMessageResponse) => string | null,
) {
  const buckets = new Map<
    string,
    {
      messages: ChatMessageResponse[];
    }
  >();

  for (const message of messages) {
    const key = keyFn(message);
    if (!key) continue;

    const existing = buckets.get(key) ?? { messages: [] };
    existing.messages.push(message);
    buckets.set(key, existing);
  }

  return Array.from(buckets.entries())
    .map(([key, value]) => ({
      key,
      count: value.messages.length,
      usage: buildUsageTotals(value.messages),
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
  const providerMessagesWithUsage = providerMessages.filter(hasUsageCapture);
  const providerMessagesWithAttribution = providerMessages.filter(hasAttribution);
  const legacyEstimatedSampleCount = providerMessagesWithUsage.filter(
    (message) => message.usageProvenance == null,
  ).length;
  const fallbackEstimatedSampleCount = providerMessagesWithUsage.filter(
    (message) => message.usageProvenance === "provider_snapshot_fallback",
  ).length;
  const uncountedSampleCount = providerMessagesWithUsage.filter(
    (message) => processedTokensForMessage(message) == null,
  ).length;
  const usableUsageSampleCount =
    providerMessagesWithUsage.length - uncountedSampleCount;
  const effectiveUsageTotals = buildUsageTotals(
    providerMessagesWithUsage,
    uncountedSampleCount > 0,
  );

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
      processedTokens: null,
      estimatedUsd: null,
    },
    effectiveUsageTotals,
    usageCoverage: {
      providerMessageCount: providerMessages.length,
      providerMessagesWithUsage: providerMessagesWithUsage.length,
      runCount: 0,
      runsWithUsage: 0,
      effectiveRunConversationCount: 0,
      effectiveMessageConversationCount:
        usableUsageSampleCount > 0 ? 1 : 0,
      legacyEstimatedSampleCount,
      fallbackEstimatedSampleCount,
      uncountedSampleCount,
      effectiveTotalsSource:
        usableUsageSampleCount > 0 ? "messages" : "none",
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
