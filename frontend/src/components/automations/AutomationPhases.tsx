import { useMemo, useState } from "react";
import { CheckCircle2, Circle, FileText, Loader2, MinusCircle } from "lucide-react";

import { Button } from "@/components/ui/button";
import { StatusPill } from "@/components/ui/status-pill";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { cn } from "@/lib/utils";
import { AutomationPlanDialog } from "./AutomationPlanDialog";
import {
  AUTOMATION_PHASES_LABEL,
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
 *
 * When `planByGoalItemId` maps an item to a plan artifact, the row grows a
 * trailing document icon that opens the plan in a markdown dialog.
 */

type PhaseStatusStyle = {
  Icon: typeof Circle;
  color: string;
};

const PHASE_STATUS_STYLES: Record<AutomationPhaseStatus, PhaseStatusStyle> = {
  done: { Icon: CheckCircle2, color: "var(--status-success, #3fbf7f)" },
  in_progress: { Icon: Loader2, color: "var(--accent-primary, #ff6a35)" },
  pending: { Icon: Circle, color: "var(--text-subtle, #6a6a72)" },
  skipped: { Icon: MinusCircle, color: "var(--text-subtle, #6a6a72)" },
};

function PhaseStatusIndicator({ status }: { status: AutomationPhaseStatus }) {
  const style = PHASE_STATUS_STYLES[status];
  const Icon = style.Icon;
  const label = AUTOMATION_PHASE_STATUS_LABELS[status];

  if (status === "in_progress") {
    return (
      <span data-phase-status={status} className="inline-flex shrink-0">
        <StatusPill
          label={label}
          tone="accent"
          live
          icon={<Icon className="h-3 w-3 shrink-0 animate-spin" aria-hidden="true" />}
          testId="automation-phase-in-progress"
        />
      </span>
    );
  }

  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <span
          className="inline-flex shrink-0"
          aria-label={label}
          data-phase-status={status}
        >
          <Icon
            className="h-3.5 w-3.5 shrink-0"
            style={{ color: style.color }}
            aria-hidden="true"
          />
        </span>
      </TooltipTrigger>
      <TooltipContent>{label}</TooltipContent>
    </Tooltip>
  );
}

interface OpenPlanState {
  planArtifactId: string;
  title: string;
}

export function AutomationPhaseProgress({
  value,
  limit,
  testId,
  planByGoalItemId = null,
}: {
  value: string | null;
  limit?: number;
  testId?: string;
  /** Goal-item id → newest plan artifact id; rows without an entry get no icon. */
  planByGoalItemId?: ReadonlyMap<string, string> | null;
}) {
  const [openPlan, setOpenPlan] = useState<OpenPlanState | null>(null);
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
      <ul
        className="max-h-64 space-y-1 overflow-y-auto overscroll-contain pr-1"
        aria-label={`Automation ${AUTOMATION_PHASES_LABEL.toLowerCase()}`}
        tabIndex={items.length > 6 ? 0 : undefined}
      >
        {items.map((item, index) => {
          const status = normalizeAutomationPhaseStatus(item.status);
          const isCurrent = status === "in_progress";
          const planArtifactId = planByGoalItemId?.get(item.id) ?? null;
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
                      : status === "done"
                        ? "var(--text-muted, #8e8e96)"
                      : "var(--text-secondary, #c7c7cc)",
                }}
              >
                {item.title}
              </span>
              <span className="flex shrink-0 items-center gap-1">
                {planArtifactId ? (
                  <Tooltip>
                    <TooltipTrigger asChild>
                      <Button
                        type="button"
                        variant="ghost"
                        size="icon-sm"
                        aria-label={`View plan for ${item.title}`}
                        className="h-6 w-6 shrink-0"
                        onClick={() =>
                          setOpenPlan({ planArtifactId, title: item.title })
                        }
                        data-testid={testId ? `${testId}-plan-icon` : undefined}
                      >
                        <FileText className="h-3.5 w-3.5" aria-hidden="true" />
                      </Button>
                    </TooltipTrigger>
                    <TooltipContent>View plan for this phase</TooltipContent>
                  </Tooltip>
                ) : null}
                <PhaseStatusIndicator status={status} />
              </span>
            </li>
          );
        })}
      </ul>
      <AutomationPlanDialog
        planArtifactId={openPlan?.planArtifactId ?? null}
        title={openPlan?.title ?? null}
        open={openPlan !== null}
        onOpenChange={(open) => {
          if (!open) {
            setOpenPlan(null);
          }
        }}
      />
    </div>
  );
}
