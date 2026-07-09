import { ShieldCheck } from "lucide-react";

import type { Automation, AutomationRun } from "@/api/automations";
import { cn } from "@/lib/utils";
import type { AutomationGoalItem } from "./automationGoalItems";
import { AutomationRunPhaseChip } from "./AutomationRunPhaseChip";
import { AutomationRunPrLink } from "./AutomationRunPrLink";
import {
  AUTOMATION_RUN_STATUS_LABELS,
  getAutomationRunView,
} from "./automationRunView";

export type AutomationRunStatusHeaderDensity = "card" | "row" | "banner";

interface AutomationRunStatusHeaderProps {
  run: AutomationRun | null;
  automation?: Automation | null;
  density: AutomationRunStatusHeaderDensity;
  activeGoalItem?: AutomationGoalItem | null;
  message?: string | null;
  showPr?: boolean;
  phaseTestId?: string;
  className?: string;
  testId?: string;
}

function StatusPill({ label }: { label: string }) {
  return (
    <span
      className="inline-flex w-fit items-center rounded-full px-2 py-0.5 text-[0.6875rem] font-semibold"
      style={{
        backgroundColor: "var(--bg-surface)",
        borderColor: "var(--border-default)",
        borderStyle: "solid",
        borderWidth: "1px",
        color: "var(--text-secondary)",
      }}
    >
      {label}
    </span>
  );
}

export function AutomationRunStatusHeader({
  run,
  automation = null,
  density,
  activeGoalItem = null,
  message = null,
  showPr = true,
  phaseTestId,
  className,
  testId,
}: AutomationRunStatusHeaderProps) {
  const view = automation && run ? getAutomationRunView(automation, run) : null;
  if (density === "banner") {
    return (
      <div
        className={cn("flex items-start gap-2 rounded-md px-3 py-2 text-xs", className)}
        style={{
          backgroundColor: "var(--bg-surface)",
          borderColor: "var(--border-default)",
          borderStyle: "solid",
          borderWidth: "1px",
          color: "var(--text-muted)",
        }}
        {...(testId ? { "data-testid": testId } : {})}
      >
        <ShieldCheck
          className="mt-0.5 h-4 w-4 shrink-0"
          style={{ color: "var(--accent-primary)" }}
          aria-hidden="true"
        />
        <span>{message ?? "Automation run conversation is read-only."}</span>
      </div>
    );
  }

  if (!run) {
    return null;
  }

  const phaseItem = view?.isOpen ? activeGoalItem : null;
  const stageLabel = view?.isOpen ? view.stageLabel : null;
  return (
    <span
      className={cn(
        "flex min-w-0 flex-wrap items-center gap-2",
        density === "card" ? "text-sm" : "text-xs",
        className,
      )}
      {...(testId ? { "data-testid": testId } : {})}
    >
      <span className="font-semibold" style={{ color: "var(--text-primary)" }}>
        Run {run.runIndex}
      </span>
      <StatusPill label={AUTOMATION_RUN_STATUS_LABELS[run.status]} />
      <StatusPill label={`Judge ${run.judgeState}`} />
      {stageLabel ? <StatusPill label={stageLabel} /> : null}
      {phaseItem ? (
        <AutomationRunPhaseChip
          item={phaseItem}
          {...(phaseTestId || testId
            ? { testId: phaseTestId ?? `${testId}-phase` }
            : {})}
        />
      ) : null}
      {showPr && run.prUrl ? (
        <AutomationRunPrLink
          run={run}
          {...(testId ? { testId: `${testId}-pr-link` } : {})}
        />
      ) : null}
    </span>
  );
}
