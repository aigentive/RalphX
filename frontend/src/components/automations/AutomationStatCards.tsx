import type { Automation, AutomationRun } from "@/api/automations";
import {
  AUTOMATION_STATUS_LABELS,
  parseAutomationGoalItems,
  summarizeAutomationPhases,
} from "@/components/automations/automationGoalItems";
import { AutomationDetailPrChip } from "@/components/automations/AutomationDetailPrChip";
import {
  getLatestMergedRun,
  getTrailingFailureStreak,
} from "@/components/automations/automationDetailPresentation";
import { StatusPill } from "@/components/ui/status-pill";
import { formatDate } from "./automationDetailFormat";
import { statusDotColor } from "./automationDetailShared";

function clampPercent(value: number, maximum: number): number {
  return maximum <= 0 ? 0 : Math.min(100, Math.round((value / maximum) * 100));
}

function SummaryCard({
  label,
  children,
  testId,
}: {
  label: string;
  children: React.ReactNode;
  testId: string;
}) {
  return (
    <section
      className="min-w-0 rounded-lg px-4 py-3"
      style={{
        backgroundColor: "var(--bg-surface, #1e1e23)",
        borderColor: "var(--border-subtle, #2e2e36)",
        borderStyle: "solid",
        borderWidth: "1px",
      }}
      data-testid={testId}
    >
      <div
        className="text-[0.6875rem] font-semibold uppercase tracking-[0.08em]"
        style={{ color: "var(--text-muted, #8e8e96)" }}
      >
        {label}
      </div>
      <div className="mt-2">{children}</div>
    </section>
  );
}

export function AutomationStatCards({
  automation,
  runs,
}: {
  automation: Automation;
  runs: AutomationRun[];
}) {
  const phases = summarizeAutomationPhases(
    parseAutomationGoalItems(automation.goalItemsJson),
  );
  const failureStreak = getTrailingFailureStreak(runs);
  const latestMerge = getLatestMergedRun(runs);

  return (
    <div
      className="grid gap-3 sm:grid-cols-2 xl:grid-cols-4"
      data-testid="automation-stat-cards"
    >
      <SummaryCard label="Status" testId="automation-stat-status">
        <StatusPill
          label={AUTOMATION_STATUS_LABELS[automation.status]}
          tone={automation.status === "active"
            ? "success"
            : automation.status === "paused"
              ? "warning"
              : automation.status === "stopped"
                ? "error"
                : "neutral"}
          variant="tinted"
          size="md"
        />
      </SummaryCard>

      <SummaryCard label="Phases merged" testId="automation-stat-phases">
        <div className="flex items-end justify-between gap-3">
          <span
            className="text-lg font-semibold tabular-nums"
            style={{ color: "var(--text-primary, #f2f2f4)" }}
          >
            {phases.done} / {phases.total}
          </span>
          <span className="text-xs" style={{ color: "var(--text-muted, #8e8e96)" }}>
            {clampPercent(phases.done, phases.total)}%
          </span>
        </div>
        <div
          className="mt-2 h-1.5 overflow-hidden rounded-full"
          style={{ backgroundColor: "var(--bg-hover, #2a2a31)" }}
          role="progressbar"
          aria-label="Merged phase progress"
          aria-valuenow={phases.done}
          aria-valuemin={0}
          aria-valuemax={phases.total}
        >
          <div
            className="h-full rounded-full"
            style={{
              backgroundColor: "var(--status-success, #2eb867)",
              width: `${clampPercent(phases.done, phases.total)}%`,
            }}
          />
        </div>
      </SummaryCard>

      <SummaryCard label="Run budget" testId="automation-stat-budget">
        <div className="flex items-end justify-between gap-3">
          <span
            className="text-lg font-semibold tabular-nums"
            style={{ color: "var(--text-primary, #f2f2f4)" }}
          >
            {runs.length} / {automation.maxRuns}
          </span>
          <span
            className="text-xs tabular-nums"
            style={{
              color: failureStreak > 0
                ? "var(--status-warning, #f4c025)"
                : "var(--text-muted, #8e8e96)",
            }}
          >
            failure streak {failureStreak}/{automation.maxConsecutiveFailures}
          </span>
        </div>
        <div
          className="mt-2 h-1.5 overflow-hidden rounded-full"
          style={{ backgroundColor: "var(--bg-hover, #2a2a31)" }}
          role="progressbar"
          aria-label="Run budget used"
          aria-valuenow={runs.length}
          aria-valuemin={0}
          aria-valuemax={automation.maxRuns}
        >
          <div
            className="h-full rounded-full"
            style={{
              backgroundColor: statusDotColor(
                runs.length >= automation.maxRuns ? "failed" : "active",
              ),
              width: `${clampPercent(runs.length, automation.maxRuns)}%`,
            }}
          />
        </div>
      </SummaryCard>

      <SummaryCard label="Last merge" testId="automation-stat-last-merge">
        {latestMerge ? (
          <div className="flex min-w-0 items-center gap-2">
            <AutomationDetailPrChip
              run={latestMerge}
              testId="automation-last-merge-pr"
            />
            <span className="shrink-0 text-sm" style={{ color: "var(--text-primary)" }}>
              Run {latestMerge.runIndex}
            </span>
            <span className="min-w-0 truncate text-xs" style={{ color: "var(--text-muted)" }}>
              {formatDate(latestMerge.prMergedAt ?? latestMerge.finishedAt)}
            </span>
          </div>
        ) : (
          <span className="text-sm" style={{ color: "var(--text-muted)" }}>
            No merged runs
          </span>
        )}
      </SummaryCard>
    </div>
  );
}
