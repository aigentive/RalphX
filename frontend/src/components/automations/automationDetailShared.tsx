import { useState } from "react";
import { ChevronDown, ChevronUp } from "lucide-react";

import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";

function statusClass(status: string): string {
  if (["active", "running", "published", "merged", "completed", "done"].includes(status)) {
    return "text-[var(--status-success)]";
  }
  if (["paused", "failed", "agent_failed", "pr_closed"].includes(status)) {
    return "text-[var(--status-warning)]";
  }
  if (["stopped", "cancelled"].includes(status)) {
    return "text-[var(--status-error)]";
  }
  return "text-[var(--text-secondary)]";
}

export function Pill({ label, status }: { label: string; status: string }) {
  return (
    <span
      className={cn(
        "inline-flex w-fit items-center rounded-full px-2 py-0.5 text-xs font-semibold",
        statusClass(status),
      )}
      style={{
        backgroundColor: "var(--bg-hover)",
        borderColor: "var(--border-default)",
        borderStyle: "solid",
        borderWidth: "1px",
      }}
    >
      {label}
    </span>
  );
}

function previewLines(text: string, maxLines: number): string {
  return text.split(/\r?\n/).slice(0, maxLines).join("\n");
}

export function ExpandableText({
  text,
  maxLines = 10,
  emptyLabel = "Not recorded",
}: {
  text: string | null;
  maxLines?: number;
  emptyLabel?: string;
}) {
  const [expanded, setExpanded] = useState(false);
  const value = text?.trim() || "";
  if (!value) {
    return <p className="text-sm" style={{ color: "var(--text-muted)" }}>{emptyLabel}</p>;
  }
  const lines = value.split(/\r?\n/);
  const expandable = lines.length > maxLines || value.length > 900;
  const renderedText = expanded || !expandable
    ? value
    : `${previewLines(value, maxLines)}\n...`;

  return (
    <div className="space-y-2">
      <pre
        className="whitespace-pre-wrap break-words rounded-md p-3 text-xs leading-5"
        style={{
          backgroundColor: "var(--bg-hover)",
          color: "var(--text-secondary)",
        }}
      >
        {renderedText}
      </pre>
      {expandable && (
        <Button
          type="button"
          variant="ghost"
          size="sm"
          className="gap-2"
          onClick={() => setExpanded((value) => !value)}
        >
          {expanded ? <ChevronUp className="h-4 w-4" /> : <ChevronDown className="h-4 w-4" />}
          {expanded ? "Collapse" : "Expand"}
        </Button>
      )}
    </div>
  );
}
