import type { AutomationRun } from "@/api/automations";

import { deriveAutomationRunMilestones } from "./automationRunMilestones";
import type { AutomationRunStatusTone } from "./automationRunView";

const TONE_COLORS: Record<AutomationRunStatusTone, string> = {
  success: "var(--status-success, #3fbf7f)",
  warning: "var(--status-warning, #e0b341)",
  error: "var(--status-error, #d55e00)",
  accent: "var(--accent-primary, #ff6a35)",
  neutral: "var(--text-subtle, #6a6a72)",
};

/**
 * Derived milestone rail for one run: elapsed gutter, tone dot, label, and an
 * optional reference chip. Renders nothing when the run has no recorded
 * milestones at all (e.g. a queued run that never started).
 */
export function AutomationRunMilestoneList({ run }: { run: AutomationRun }) {
  const milestones = deriveAutomationRunMilestones(run);
  if (milestones.length === 0) {
    return null;
  }
  return (
    <ol
      className="mt-3 flex flex-col gap-1"
      data-testid={`automation-run-${run.id}-milestones`}
    >
      {milestones.map((milestone) => (
        <li key={milestone.key} className="flex min-w-0 items-start gap-3">
          <span
            className="w-10 shrink-0 pt-px text-right font-mono text-[0.6875rem] tabular-nums"
            style={{ color: "var(--text-subtle, #6a6a72)" }}
          >
            {milestone.elapsed ?? "—"}
          </span>
          <span
            className="mt-1.5 h-1.5 w-1.5 shrink-0 rounded-full"
            style={{ backgroundColor: TONE_COLORS[milestone.tone] }}
            aria-hidden="true"
          />
          <span
            className="min-w-0 flex-1 text-[0.8125rem] leading-5"
            style={{ color: "var(--text-secondary, #c7c7cc)" }}
            data-testid={`automation-run-${run.id}-milestone-${milestone.key}`}
          >
            {milestone.label}
          </span>
          {milestone.chip ? (
            <span
              className="shrink-0 rounded px-1.5 py-px font-mono text-[0.625rem] font-semibold"
              style={{
                backgroundColor: "var(--status-success-muted, #173c29)",
                color: "var(--status-success, #3fbf7f)",
              }}
            >
              {milestone.chip}
            </span>
          ) : null}
        </li>
      ))}
    </ol>
  );
}
