import { ShieldCheck } from "lucide-react";

import type { Automation, AutomationRun } from "@/api/automations";
import { StatusPill } from "@/components/ui/status-pill";
import { cn } from "@/lib/utils";
import type { AutomationGoalItem } from "./automationGoalItems";
import { AutomationRunPrLink } from "./AutomationRunPrLink";
import { getRunCardBadges, type AutomationRunCardBadge } from "./automationRunBadges";
import {
  getAutomationRunJudgeLabel,
  getAutomationRunStatusLabel,
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

function badgeTestId(
  badge: AutomationRunCardBadge,
  testId: string | undefined,
): string | undefined {
  return testId ? `${testId}-${badge.key}` : undefined;
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
    const statusLabel = view?.statusLabel ?? (run ? getAutomationRunStatusLabel(run) : null);
    const judgeLabel = view?.judgeLabel ?? (run ? getAutomationRunJudgeLabel(run) : null);
    const stageLabel = view?.stageLabel ?? null;
    const prLabel = showPr && view ? view.pr.value : null;
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
        <span className="min-w-0 space-y-2">
          <span>{message ?? "Automation run conversation is read-only."}</span>
          {statusLabel ? (
            <span className="flex min-w-0 flex-wrap items-center gap-2">
              <StatusPill
                label={statusLabel}
                {...(testId ? { testId: `${testId}-status` } : {})}
              />
              {judgeLabel ? (
                <StatusPill
                  label={judgeLabel}
                  {...(testId ? { testId: `${testId}-judge` } : {})}
                />
              ) : null}
              {stageLabel ? (
                <StatusPill
                  label={stageLabel}
                  {...(testId ? { testId: `${testId}-stage` } : {})}
                />
              ) : null}
              {prLabel ? (
                <StatusPill
                  label={prLabel}
                  {...(testId ? { testId: `${testId}-pr` } : {})}
                />
              ) : null}
            </span>
          ) : null}
        </span>
      </div>
    );
  }

  if (!run) {
    return null;
  }

  // Card/row densities render the de-duplicated badge contract: one status
  // badge, judge/stage only when they add information (see getRunCardBadges).
  const badges = automation
    ? getRunCardBadges(automation, run)
    : [
        {
          key: "status" as const,
          label: getAutomationRunStatusLabel(run),
          tone: "neutral" as const,
          live: false,
        },
      ];
  const phaseItem = view?.isOpen ? activeGoalItem : null;
  const prLabel = showPr && view ? view.pr.value : null;
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
      {badges.map((badge) => {
        const badgeId = badgeTestId(badge, testId);
        return (
          <StatusPill
            key={badge.key}
            label={badge.label}
            tone={badge.tone}
            live={badge.live}
            {...(badgeId ? { testId: badgeId } : {})}
          />
        );
      })}
      {prLabel ? (
        <StatusPill
          label={prLabel}
          {...(testId ? { testId: `${testId}-pr` } : {})}
        />
      ) : null}
      {phaseItem ? (
        <span
          className="min-w-0 max-w-full truncate text-xs font-medium"
          style={{ color: "var(--text-secondary)" }}
          title={phaseItem.title}
          {...(phaseTestId || testId
            ? { "data-testid": phaseTestId ?? `${testId}-phase` }
            : {})}
        >
          {phaseItem.title}
        </span>
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
