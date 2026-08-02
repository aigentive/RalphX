import { formatDuration } from "@/components/tasks/detail-views/shared/DurationDisplay";

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
