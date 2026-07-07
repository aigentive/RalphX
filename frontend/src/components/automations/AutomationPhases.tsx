import { useMemo } from "react";
import { CheckCircle2, Circle, Loader2, MinusCircle } from "lucide-react";

import { cn } from "@/lib/utils";
import {
  AUTOMATION_PHASE_STATUS_LABELS,
  normalizeAutomationPhaseStatus,
  parseAutomationGoalItems,
  summarizeAutomationPhases,
  type AutomationPhaseStatus,
} from "./automationGoalItems";

/**
 * Shared phase-progress surface used by both the Agents automation panel and
 * the Automations detail view. Renders an at-a-glance progress row (count +
 * slim bar) plus a per-phase list with distinct status styling and the current
 * in-progress phase highlighted. Returns `null` when there are no phases so the
 * host can render its own empty-state copy.
 */

type PhaseStatusStyle = {
  label: string;
  color: string;
  backgroundColor: string;
  borderColor: string;
  Icon: typeof Circle;
};

// Literal-value fallbacks alongside the semantic tokens keep the badges painted
// in WKWebView even if a var() chain fails to cascade.
const PHASE_STATUS_STYLES: Record<AutomationPhaseStatus, PhaseStatusStyle> = {
  done: {
    label: AUTOMATION_PHASE_STATUS_LABELS.done,
    color: "var(--status-success, #2eb867)",
    backgroundColor: "var(--bg-hover, #2a2a31)",
    borderColor: "var(--border-subtle, #2e2e36)",
    Icon: CheckCircle2,
  },
  in_progress: {
    label: AUTOMATION_PHASE_STATUS_LABELS.in_progress,
    color: "var(--accent-primary, #ff6a35)",
    backgroundColor: "var(--accent-muted)",
    borderColor: "var(--accent-border)",
    Icon: Loader2,
  },
  pending: {
    label: AUTOMATION_PHASE_STATUS_LABELS.pending,
    color: "var(--text-muted, #8e8e96)",
    backgroundColor: "var(--bg-hover, #2a2a31)",
    borderColor: "var(--border-subtle, #2e2e36)",
    Icon: Circle,
  },
  skipped: {
    label: AUTOMATION_PHASE_STATUS_LABELS.skipped,
    color: "var(--text-subtle, #6a6a72)",
    backgroundColor: "var(--bg-hover, #2a2a31)",
    borderColor: "var(--border-subtle, #2e2e36)",
    Icon: MinusCircle,
  },
};

function PhaseStatusBadge({ status }: { status: AutomationPhaseStatus }) {
  const style = PHASE_STATUS_STYLES[status];
  const Icon = style.Icon;
  return (
    <span
      className="inline-flex shrink-0 items-center gap-1 rounded-full px-2 py-0.5 text-[0.6875rem] font-semibold"
      style={{
        color: style.color,
        backgroundColor: style.backgroundColor,
        borderColor: style.borderColor,
        borderStyle: "solid",
        borderWidth: "1px",
      }}
      data-phase-status={status}
    >
      <Icon
        className={cn("h-3 w-3", status === "in_progress" && "animate-spin")}
        aria-hidden="true"
      />
      {style.label}
    </span>
  );
}

export function AutomationPhaseProgress({
  value,
  limit,
  testId,
}: {
  value: string | null;
  limit?: number;
  testId?: string;
}) {
  const items = useMemo(
    () => parseAutomationGoalItems(value, limit !== undefined ? { limit } : {}),
    [value, limit],
  );
  // Summarize the full (unsliced) list so the "done" count is accurate even when
  // the rendered list is truncated by `limit`.
  const summary = useMemo(
    () => summarizeAutomationPhases(parseAutomationGoalItems(value)),
    [value],
  );

  if (items.length === 0) {
    return null;
  }

  const percent = Math.round(summary.progressRatio * 100);

  return (
    <div
      className="space-y-2"
      {...(testId ? { "data-testid": testId } : {})}
    >
      <div className="flex items-center gap-3">
        <span
          className="text-xs font-semibold tabular-nums"
          style={{ color: "var(--text-primary, #f2f2f4)" }}
          data-testid={testId ? `${testId}-count` : undefined}
        >
          {summary.done}/{summary.total} done
        </span>
        {summary.skipped > 0 ? (
          <span
            className="text-[0.6875rem]"
            style={{ color: "var(--text-subtle, #6a6a72)" }}
          >
            {summary.skipped} skipped
          </span>
        ) : null}
        <div
          className="h-1.5 min-w-0 flex-1 overflow-hidden rounded-full"
          style={{ backgroundColor: "var(--bg-hover, #2a2a31)" }}
          role="progressbar"
          aria-label="Phase progress"
          aria-valuenow={summary.done}
          aria-valuemin={0}
          aria-valuemax={summary.total}
        >
          <div
            className="h-full rounded-full transition-[width] duration-300"
            style={{
              width: `${percent}%`,
              backgroundColor: "var(--accent-primary, #ff6a35)",
            }}
          />
        </div>
      </div>
      <ul className="space-y-1">
        {items.map((item, index) => {
          const status = normalizeAutomationPhaseStatus(item.status);
          const isCurrent = status === "in_progress";
          return (
            <li
              key={`${item.id}-${index}`}
              className="flex items-center justify-between gap-3 rounded-md px-2 py-1"
              style={
                isCurrent
                  ? {
                      backgroundColor: "var(--accent-muted)",
                      borderColor: "var(--accent-border)",
                      borderStyle: "solid",
                      borderWidth: "1px",
                    }
                  : {
                      borderColor: "transparent",
                      borderStyle: "solid",
                      borderWidth: "1px",
                    }
              }
              data-testid={testId ? `${testId}-item` : undefined}
            >
              <span
                className={cn(
                  "min-w-0 truncate text-xs",
                  isCurrent ? "font-semibold" : "font-medium",
                  status === "skipped" && "line-through",
                )}
                style={{
                  color: isCurrent
                    ? "var(--text-primary, #f2f2f4)"
                    : status === "skipped"
                      ? "var(--text-subtle, #6a6a72)"
                      : "var(--text-secondary, #c7c7cc)",
                }}
              >
                {item.title}
              </span>
              <PhaseStatusBadge status={status} />
            </li>
          );
        })}
      </ul>
    </div>
  );
}
