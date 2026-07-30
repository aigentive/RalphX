import {
  formatToolActivitySummary,
  type ToolActivitySummary,
} from "./tool-activity-summary";
import { TranscriptGroupToggle } from "./TranscriptGroupToggle";

interface ToolActivityGroupToggleProps {
  groupKey: string;
  summary: ToolActivitySummary;
  isExpanded: boolean;
  onToggle: React.MouseEventHandler<HTMLButtonElement>;
}

export function ToolActivityGroupToggle({
  groupKey,
  summary,
  isExpanded,
  onToggle,
}: ToolActivityGroupToggleProps) {
  const sentence = formatToolActivitySummary(summary);
  return <TranscriptGroupToggle groupKey={groupKey} sentence={sentence} isExpanded={isExpanded}
    onToggle={onToggle} testId="tool-call-group-toggle" groupDataAttribute="data-chat-tool-call-group-key"
    detailsLabel="tool details" />;
}
