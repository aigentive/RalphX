import type { AutomationRun } from "@/api/automations";

import { formatElapsed } from "./automationDetailFormat";
import {
  AUTOMATION_RUN_STATUS_LABELS,
  describeRunFailure,
  isOpenAutomationRun,
} from "./automationRunView";
import type { AutomationRunStatusTone } from "./automationRunView";

/**
 * Milestone list for the expanded Runs-timeline card.
 *
 * Every entry is derived from a field the backend actually persists. Only
 * `startedAt`, `prMergedAt`, and `finishedAt` carry timestamps, so plan and PR
 * milestones render without an elapsed offset rather than with a guessed one,
 * and events with no backing data at all (commit counts, CI check results) are
 * deliberately absent.
 */

export type AutomationRunMilestoneKey =
  | "started"
  | "plan"
  | "pr"
  | "merged"
  | "failed"
  | "cancelled"
  | "finished"
  | "running";

export interface AutomationRunMilestone {
  key: AutomationRunMilestoneKey;
  label: string;
  tone: AutomationRunStatusTone;
  /** `mm:ss` offset from the run start, or null when no timestamp exists. */
  elapsed: string | null;
  /** Optional trailing reference chip (e.g. the PR number). */
  chip?: string;
}

function terminalMilestone(
  run: AutomationRun,
  elapsedAt: (value: string | null) => string | null,
): AutomationRunMilestone {
  const failure = describeRunFailure(run);
  if (failure) {
    return {
      key: "failed",
      label: failure,
      tone: "error",
      elapsed: elapsedAt(run.finishedAt),
    };
  }
  if (run.status === "cancelled") {
    return {
      key: "cancelled",
      label: "Cancelled",
      tone: "neutral",
      elapsed: elapsedAt(run.finishedAt),
    };
  }
  if (run.status === "merged") {
    return {
      key: "merged",
      label: "Merged into base",
      tone: "success",
      elapsed: elapsedAt(run.prMergedAt ?? run.finishedAt),
    };
  }
  return {
    key: "finished",
    label: AUTOMATION_RUN_STATUS_LABELS[run.status],
    tone: "success",
    elapsed: elapsedAt(run.finishedAt),
  };
}

export function deriveAutomationRunMilestones(
  run: AutomationRun,
): AutomationRunMilestone[] {
  const elapsedAt = (value: string | null) => formatElapsed(run.startedAt, value);
  const milestones: AutomationRunMilestone[] = [];

  if (run.startedAt) {
    milestones.push({
      key: "started",
      label: "Run started",
      tone: "neutral",
      elapsed: elapsedAt(run.startedAt),
    });
  }
  if (run.planArtifactId) {
    milestones.push({
      key: "plan",
      label: "Plan artifact published",
      tone: "accent",
      elapsed: null,
    });
  }
  if (run.prNumber || run.prUrl) {
    milestones.push({
      key: "pr",
      label: "PR opened against base",
      tone: "accent",
      elapsed: null,
      ...(run.prNumber ? { chip: `PR #${run.prNumber}` } : {}),
    });
  }

  // A still-open run has no settled outcome to report yet.
  if (isOpenAutomationRun(run) && !describeRunFailure(run)) {
    milestones.push({
      key: "running",
      label: AUTOMATION_RUN_STATUS_LABELS[run.status],
      tone: "accent",
      elapsed: null,
    });
    return milestones;
  }

  milestones.push(terminalMilestone(run, elapsedAt));
  return milestones;
}
