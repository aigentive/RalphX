import { formatDuration } from "@/components/tasks/detail-views/shared/DurationDisplay";

export function formatThinkingSummary(isSettled: boolean, durationMs?: number): string {
  if (!isSettled) return "Agent thinking…";
  if (durationMs != null) return `Agent thought for ${formatDuration(Math.round(durationMs / 1000))}`;
  return "Agent thought";
}
