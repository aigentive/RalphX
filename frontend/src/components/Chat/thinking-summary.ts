import { formatDuration } from "@/components/tasks/detail-views/shared/DurationDisplay";
import type { ThinkingGroupAggregate } from "./thinking-group";

export function formatThinkingSummary(
  isSettled: boolean,
  durationMs?: number,
  estimatedTokens?: number,
): string {
  if (!isSettled) {
    return estimatedTokens != null
      ? `Agent thinking… · ~${estimatedTokens.toLocaleString()} tokens`
      : "Agent thinking…";
  }
  if (durationMs != null) return `Agent thought for ${formatDuration(Math.round(durationMs / 1000))}`;
  return "Agent thought";
}

export function formatThinkingGroupSummary(aggregate: ThinkingGroupAggregate): string {
  const summary = formatThinkingSummary(
    aggregate.isSettled,
    aggregate.totalDurationMs,
    aggregate.estimatedTokens,
  );
  return aggregate.isSettled && aggregate.segmentCount > 1
    ? `${summary} · ${aggregate.segmentCount} steps`
    : summary;
}
