import { formatDuration } from "@/components/tasks/detail-views/shared/DurationDisplay";
import type { ThinkingGroupAggregate } from "./thinking-group";

export function formatThinkingSummary(
  isSettled: boolean,
  durationMs?: number,
  estimatedTokens?: number,
  reasoningTokens?: number,
): string {
  if (!isSettled) {
    return estimatedTokens != null
      ? `Agent thinking… · ~${estimatedTokens.toLocaleString()} tokens`
      : "Agent thinking…";
  }
  if (durationMs != null) return `Agent thought for ${formatDuration(Math.round(durationMs / 1000))}`;
  if (reasoningTokens != null) {
    return `Agent thought · ~${reasoningTokens.toLocaleString()} reasoning tokens`;
  }
  return "Agent thought";
}

export function formatThinkingGroupSummary(aggregate: ThinkingGroupAggregate): string {
  const summary = formatThinkingSummary(
    aggregate.isSettled,
    aggregate.totalDurationMs,
    aggregate.estimatedTokens,
    aggregate.reasoningTokens,
  );
  return aggregate.isSettled && aggregate.segmentCount > 1
    ? `${summary} · ${aggregate.segmentCount} steps`
    : summary;
}
