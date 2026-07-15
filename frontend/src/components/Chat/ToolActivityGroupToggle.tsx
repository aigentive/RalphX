import { ChevronDown, ChevronRight } from "lucide-react";
import {
  formatToolActivitySummary,
  type ToolActivitySummary,
} from "./tool-activity-summary";

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
  const action = isExpanded ? "Collapse" : "Expand";

  return (
    <button
      type="button"
      data-testid="tool-call-group-toggle"
      data-chat-tool-call-group-key={groupKey}
      aria-expanded={isExpanded}
      aria-label={`${sentence} ${action} tool details.`}
      onClick={onToggle}
      className="inline-flex max-w-full items-center gap-1.5 rounded-md px-2 py-1 text-left text-[0.6875rem] font-medium transition-opacity hover:opacity-80"
      style={{
        backgroundColor: "var(--bg-elevated)",
        color: "var(--text-secondary)",
      }}
    >
      {isExpanded ? (
        <ChevronDown className="h-3 w-3 shrink-0" aria-hidden="true" />
      ) : (
        <ChevronRight className="h-3 w-3 shrink-0" aria-hidden="true" />
      )}
      <span>{sentence}</span>
    </button>
  );
}
