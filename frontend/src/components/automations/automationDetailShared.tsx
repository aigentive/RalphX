import { useState, type ReactNode } from "react";
import { ChevronDown, ChevronUp } from "lucide-react";

import { Button } from "@/components/ui/button";
import { StatusPill, type StatusPillTone } from "@/components/ui/status-pill";

function statusTone(status: string): StatusPillTone {
  if (["active", "running", "published", "merged", "completed", "done", "executing"].includes(status)) {
    return "success";
  }
  if (["paused", "failed", "agent_failed", "pr_closed", "attention"].includes(status)) {
    return "warning";
  }
  if (["stopped", "cancelled"].includes(status)) {
    return "error";
  }
  return "neutral";
}

/** Compatibility shim over the design-system {@link StatusPill}. */
export function Pill({ label, status }: { label: string; status: string }) {
  return <StatusPill label={label} size="md" tone={statusTone(status)} />;
}

export function Section({
  title,
  children,
  testId,
}: {
  title: string;
  children: ReactNode;
  testId?: string;
}) {
  return (
    <section
      className="rounded-md p-4"
      style={{
        backgroundColor: "var(--bg-surface)",
        borderColor: "var(--border-default)",
        borderStyle: "solid",
        borderWidth: "1px",
      }}
      {...(testId ? { "data-testid": testId } : {})}
    >
      <h2 className="text-sm font-semibold" style={{ color: "var(--text-primary)" }}>
        {title}
      </h2>
      <div className="mt-3">{children}</div>
    </section>
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
