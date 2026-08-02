import { TranscriptGroupToggle } from "./TranscriptGroupToggle";
import { formatThinkingSummary } from "./thinking-summary";

export function ThinkingGroupToggle({ groupKey, isExpanded, isSettled, durationMs, estimatedTokens, onToggle }: {
  groupKey: string; isExpanded: boolean; isSettled: boolean; durationMs?: number; estimatedTokens?: number;
  onToggle: React.MouseEventHandler<HTMLButtonElement>;
}) {
  return <TranscriptGroupToggle groupKey={groupKey} sentence={formatThinkingSummary(isSettled, durationMs, estimatedTokens)}
    isExpanded={isExpanded} onToggle={onToggle} testId="thinking-group-toggle"
    groupDataAttribute="data-chat-thinking-group-key" detailsLabel="thinking details" />;
}
