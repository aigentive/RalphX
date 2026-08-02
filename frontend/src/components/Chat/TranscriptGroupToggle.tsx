import { ChevronDown, ChevronRight } from "lucide-react";

interface TranscriptGroupToggleProps {
  groupKey: string;
  sentence: string;
  isExpanded: boolean;
  onToggle: React.MouseEventHandler<HTMLButtonElement>;
  testId: string;
  groupDataAttribute: "data-chat-tool-call-group-key" | "data-chat-thinking-group-key";
  detailsLabel: string;
}

export function TranscriptGroupToggle({
  groupKey, sentence, isExpanded, onToggle, testId, groupDataAttribute, detailsLabel,
}: TranscriptGroupToggleProps) {
  const action = isExpanded ? "Collapse" : "Expand";
  return (
    <button type="button" data-testid={testId} {...{ [groupDataAttribute]: groupKey }}
      aria-expanded={isExpanded} aria-label={`${sentence} ${action} ${detailsLabel}.`} onClick={onToggle}
      className="inline-flex max-w-full items-center gap-1.5 rounded-md px-2 py-1 text-left text-[0.6875rem] font-medium transition-opacity hover:opacity-80"
      style={{ backgroundColor: "var(--bg-elevated)", color: "var(--text-secondary)" }}>
      {isExpanded ? <ChevronDown className="h-3 w-3 shrink-0" aria-hidden="true" /> : <ChevronRight className="h-3 w-3 shrink-0" aria-hidden="true" />}
      <span>{sentence}</span>
    </button>
  );
}
