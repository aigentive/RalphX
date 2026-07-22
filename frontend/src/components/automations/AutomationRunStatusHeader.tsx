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
  prTestId?: string;
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
  prTestId,
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
  const primaryBadge = badges[0];
  const primaryBadgeId = primaryBadge ? badgeTestId(primaryBadge, testId) : undefined;
  const secondaryBadges = badges.slice(1);
  const phaseItem = view?.isOpen ? activeGoalItem : null;
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
      {primaryBadge ? (
        <StatusPill
          label={primaryBadge.label}
          tone={primaryBadge.tone}
          live={primaryBadge.live}
          {...(primaryBadgeId ? { testId: primaryBadgeId } : {})}
        />
      ) : null}
      {secondaryBadges.length > 0 ? (
        <span
          className="min-w-0 truncate text-xs"
          style={{ color: "var(--text-muted, #8e8e96)" }}
          {...(testId ? { "data-testid": `${testId}-secondary-states` } : {})}
        >
          {secondaryBadges.map((badge, index) => {
            const secondaryBadgeId = badgeTestId(badge, testId);
            return (
              <span
                key={badge.key}
                {...(secondaryBadgeId
                  ? { "data-testid": secondaryBadgeId }
                  : {})}
              >
                {index > 0 ? " · " : null}
                {badge.label}
              </span>
            );
          })}
        </span>
      ) : null}
      {showPr && run.prUrl ? (
        <AutomationRunPrLink
          run={run}
          className="pointer-events-auto shrink-0"
          {...(prTestId || testId
            ? { testId: prTestId ?? `${testId}-pr-link` }
            : {})}
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
    </span>
  );
}
