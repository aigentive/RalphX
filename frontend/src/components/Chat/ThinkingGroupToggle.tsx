import { TranscriptGroupToggle } from "./TranscriptGroupToggle";
import { formatThinkingGroupSummary } from "./thinking-summary";

export function ThinkingGroupToggle({ groupKey, isExpanded, isSettled, durationMs, estimatedTokens, segmentCount = 1, onToggle }: {
  groupKey: string; isExpanded: boolean; isSettled: boolean; durationMs?: number; estimatedTokens?: number; segmentCount?: number;
  onToggle: React.MouseEventHandler<HTMLButtonElement>;
}) {
  return <TranscriptGroupToggle groupKey={groupKey} sentence={formatThinkingGroupSummary({
    isSettled,
    segmentCount,
    ...(durationMs != null ? { totalDurationMs: durationMs } : {}),
    ...(estimatedTokens != null ? { estimatedTokens } : {}),
  })}
    isExpanded={isExpanded} onToggle={onToggle} testId="thinking-group-toggle"
    groupDataAttribute="data-chat-thinking-group-key" detailsLabel="thinking details" />;
}
