import { useMemo, useState } from "react";
import { Check, Circle, FileText, LoaderCircle, Minus } from "lucide-react";

import type { AutomationRun } from "@/api/automations";
import { AutomationDetailPrChip } from "@/components/automations/AutomationDetailPrChip";
import {
  getAutomationPhaseGroups,
  getMergedPrByGoalItem,
  getPlanArtifactByGoalItem,
} from "@/components/automations/automationDetailPresentation";
import {
  normalizeAutomationPhaseStatus,
  parseAutomationGoalItems,
  summarizeAutomationPhases,
} from "@/components/automations/automationGoalItems";
import { AutomationPlanDialog } from "@/components/automations/AutomationPlanDialog";
import { Button } from "@/components/ui/button";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { cn } from "@/lib/utils";

const STATUS_ICONS = {
  done: Check,
  in_progress: LoaderCircle,
  pending: Circle,
  skipped: Minus,
};

export function AutomationPhaseList({
  value,
  runs,
}: {
  value: string | null;
  runs: AutomationRun[];
}) {
  const [openPlan, setOpenPlan] = useState<{ id: string; title: string } | null>(null);
  const items = useMemo(() => parseAutomationGoalItems(value), [value]);
  const summary = useMemo(() => summarizeAutomationPhases(items), [items]);
  const groups = useMemo(() => getAutomationPhaseGroups(items), [items]);
  const mergedPrByGoalItem = useMemo(() => getMergedPrByGoalItem(runs), [runs]);
  const planByGoalItem = useMemo(() => getPlanArtifactByGoalItem(runs), [runs]);

  if (items.length === 0) {
    return (
      <p className="text-sm" style={{ color: "var(--text-muted)" }}>
        No phases have been defined.
      </p>
    );
  }

  return (
    <>
      <div
        className="mb-4 flex h-2 gap-1"
        role="progressbar"
        aria-label="Phase completion"
        aria-valuenow={summary.done}
        aria-valuemin={0}
        aria-valuemax={summary.total}
      >
        {items.map((item) => {
          const status = normalizeAutomationPhaseStatus(item.status);
          return (
            <span
              key={item.id}
              className="min-w-0 flex-1 rounded-full"
              style={{
                backgroundColor: status === "done"
                  ? "var(--status-success, #2eb867)"
                  : status === "in_progress"
                    ? "var(--accent-primary, #ff6a35)"
                    : "var(--bg-hover, #2a2a31)",
              }}
            />
          );
        })}
      </div>
      <div className="space-y-4" data-testid="automation-phase-list">
        {groups.map((group) => (
          <div key={group.key}>
            {group.label ? (
              <div
                className="mb-1.5 text-[0.6875rem] font-semibold uppercase tracking-[0.08em]"
                style={{ color: "var(--text-muted)" }}
              >
                Group {group.label}
              </div>
            ) : null}
            <ul className="space-y-1">
              {group.items.map((item) => {
                const status = normalizeAutomationPhaseStatus(item.status);
                const StatusIcon = STATUS_ICONS[status];
                const mergedRun = mergedPrByGoalItem.get(item.id);
                const planId = planByGoalItem.get(item.id);
                return (
                  <li
                    key={item.id}
                    className="grid grid-cols-[auto_auto_minmax(0,1fr)_auto] items-center gap-2 rounded-md px-2 py-2"
                    style={{
                      backgroundColor: status === "in_progress"
                        ? "var(--accent-muted, #3a2a22)"
                        : "transparent",
                      borderColor: status === "in_progress"
                        ? "var(--accent-border, #59392a)"
                        : "transparent",
                      borderStyle: "solid",
                      borderWidth: "1px",
                    }}
                    data-testid="automation-phase-item"
                  >
                    <StatusIcon
                      className={cn("h-4 w-4", status === "in_progress" && "animate-spin")}
                      style={{
                        color: status === "done"
                          ? "var(--status-success)"
                          : status === "in_progress"
                            ? "var(--accent-primary)"
                            : "var(--text-subtle)",
                      }}
                      aria-label={status.replace("_", " ")}
                    />
                    <code className="text-xs" style={{ color: "var(--text-muted)" }}>
                      {item.id}
                    </code>
                    <span
                      className={cn(
                        "min-w-0 truncate text-sm",
                        status === "skipped" && "line-through",
                      )}
                      style={{ color: "var(--text-primary)" }}
                    >
                      {item.title}
                    </span>
                    <span className="flex items-center gap-1">
                      {mergedRun ? (
                        <AutomationDetailPrChip
                          run={mergedRun}
                          testId={`automation-phase-${item.id}-pr`}
                        />
                      ) : null}
                      {planId ? (
                        <Tooltip>
                          <TooltipTrigger asChild>
                            <Button
                              type="button"
                              variant="ghost"
                              size="icon-sm"
                              className="h-7 w-7"
                              aria-label={`View plan for ${item.title}`}
                              onClick={() => setOpenPlan({ id: planId, title: item.title })}
                              data-testid="automation-phase-plan-icon"
                            >
                              <FileText className="h-3.5 w-3.5" aria-hidden="true" />
                            </Button>
                          </TooltipTrigger>
                          <TooltipContent>View plan for this phase</TooltipContent>
                        </Tooltip>
                      ) : null}
                    </span>
                  </li>
                );
              })}
            </ul>
          </div>
        ))}
      </div>
      <AutomationPlanDialog
        planArtifactId={openPlan?.id ?? null}
        title={openPlan?.title ?? null}
        open={openPlan !== null}
        onOpenChange={(open) => {
          if (!open) setOpenPlan(null);
        }}
      />
    </>
  );
}
