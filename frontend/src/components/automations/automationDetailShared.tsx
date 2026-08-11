import { useState, type ReactNode } from "react";
import { ChevronDown, ChevronUp } from "lucide-react";

import { Button } from "@/components/ui/button";
import { StatusPill, type StatusPillTone } from "@/components/ui/status-pill";
import { cn } from "@/lib/utils";

function statusTone(status: string): StatusPillTone {
  if (["active", "running", "in_progress", "executing"].includes(status)) {
    return "accent";
  }
  if (["published", "merged", "completed", "done", "success"].includes(status)) {
    return "success";
  }
  if (["paused", "awaiting_plan_approval", "revision_pending", "attention"].includes(status)) {
    return "warning";
  }
  if (["failed", "agent_failed", "pr_closed", "dropped"].includes(status)) {
    return "error";
  }
  return "neutral";
}

// eslint-disable-next-line react-refresh/only-export-components -- shared status mapping for automation detail surfaces.
export function statusDotColor(status: string): string {
  const tone = statusTone(status);
  if (tone === "accent") {
    return "var(--accent-primary)";
  }
  if (tone === "success") {
    return "var(--status-success, #2eb867)";
  }
  if (tone === "warning") {
    return "var(--status-warning, #f4c025)";
  }
  if (tone === "error") {
    return "var(--status-error, #dd3c3c)";
  }
  return "var(--text-subtle, #6a6a72)";
}

/** Compatibility shim over the design-system {@link StatusPill}. */
export function Pill({
  label,
  status,
  live = false,
}: {
  label: string;
  status: string;
  live?: boolean;
}) {
  return (
    <StatusPill
      label={label}
      size="md"
      tone={statusTone(status)}
      live={live}
    />
  );
}

export function FieldLabel({
  children,
  variant = "field",
  className,
}: {
  children: ReactNode;
  variant?: "field" | "group";
  className?: string;
}) {
  return (
    <span
      className={cn(
        "text-[0.6875rem] font-semibold uppercase tracking-[0.08em]",
        variant === "group" && "opacity-60",
        className,
      )}
      style={{
        color: variant === "group" ? "var(--text-secondary)" : "var(--text-muted)",
      }}
    >
      {children}
    </span>
  );
}

export function Section({
  title,
  headerRight,
  children,
  testId,
  background = "elevated",
}: {
  title: string;
  headerRight?: ReactNode;
  children: ReactNode;
  testId?: string;
  background?: "elevated" | "surface";
}) {
  return (
    <section
      className="rounded-lg p-5 shadow-[var(--shadow-xs)]"
      style={{
        backgroundColor: background === "surface"
          ? "var(--bg-surface, #1e1e23)"
          : "var(--bg-elevated, #232329)",
        borderColor: "var(--border-subtle, #2e2e36)",
        borderStyle: "solid",
        borderWidth: "1px",
      }}
      {...(testId ? { "data-testid": testId } : {})}
    >
      <div className="flex min-w-0 items-center justify-between gap-3">
        <h2 className="text-sm font-semibold" style={{ color: "var(--text-primary)" }}>
          {title}
        </h2>
        {headerRight}
      </div>
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
        className="whitespace-pre-wrap break-words rounded-md p-3 text-[0.8125rem] leading-6"
        style={{
          backgroundColor: "var(--bg-surface, #1e1e23)",
          borderColor: "var(--border-subtle, #2e2e36)",
          borderStyle: "solid",
          borderWidth: "1px",
          color: "var(--text-secondary, #c7c7cc)",
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
