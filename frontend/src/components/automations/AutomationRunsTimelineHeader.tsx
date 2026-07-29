import type { AutomationChainMode, AutomationRun } from "@/api/automations";
import { StatusPill } from "@/components/ui/status-pill";

import { formatShortDate } from "./automationDetailFormat";
import {
  describeAutomationChainMode,
  summarizeAutomationRuns,
} from "./automationRunsSummary";
import { FieldLabel } from "./automationDetailShared";

/**
 * Runs-timeline header: section label, a tinted outcome strip counted from the
 * real run list, and a right-aligned provenance hint (first run + chain mode).
 */
export function AutomationRunsTimelineHeader({
  runs,
  chainMode,
}: {
  runs: AutomationRun[];
  chainMode: AutomationChainMode;
}) {
  const summary = summarizeAutomationRuns(runs);
  const firstRunLabel = formatShortDate(summary.firstRunAt);
  const hint = [
    firstRunLabel ? `first run ${firstRunLabel}` : null,
    describeAutomationChainMode(chainMode),
  ]
    .filter((part): part is string => Boolean(part))
    .join(" · ");

  return (
    <div
      className="mb-4 flex min-w-0 flex-wrap items-center gap-x-3 gap-y-2"
      data-testid="automation-runs-timeline-header"
    >
      <FieldLabel>Runs timeline</FieldLabel>
      {summary.counts.map((count) => (
        <StatusPill
          key={count.key}
          label={count.label}
          tone={count.tone}
          variant="tinted"
          testId={`automation-runs-summary-${count.key}`}
        />
      ))}
      <span
        className="ml-auto min-w-0 truncate text-[0.6875rem]"
        style={{ color: "var(--text-subtle, #6a6a72)" }}
        data-testid="automation-runs-timeline-hint"
      >
        {hint}
      </span>
    </div>
  );
}
