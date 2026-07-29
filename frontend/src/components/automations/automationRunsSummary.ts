import type { AutomationChainMode, AutomationRun } from "@/api/automations";

import { describeRunFailure, isOpenAutomationRun } from "./automationRunView";
import type { AutomationRunStatusTone } from "./automationRunView";

/**
 * Outcome roll-up for the Runs-timeline header strip. Buckets are derived from
 * real run status/judge state — never from hard-coded totals — and empty buckets
 * are dropped so the strip only advertises outcomes this automation actually has.
 */

export type AutomationRunsSummaryKey =
  | "merged"
  | "failed"
  | "cancelled"
  | "running"
  | "completed";

export interface AutomationRunsSummaryCount {
  key: AutomationRunsSummaryKey;
  label: string;
  tone: AutomationRunStatusTone;
}

export interface AutomationRunsSummary {
  counts: AutomationRunsSummaryCount[];
  firstRunAt: string | null;
  total: number;
}

interface BucketDefinition {
  key: AutomationRunsSummaryKey;
  noun: string;
  tone: AutomationRunStatusTone;
}

/** Display order mirrors the run lifecycle: settled success → problems → live. */
const BUCKETS: BucketDefinition[] = [
  { key: "merged", noun: "merged", tone: "success" },
  { key: "failed", noun: "failed", tone: "error" },
  { key: "cancelled", noun: "cancelled", tone: "neutral" },
  { key: "running", noun: "running", tone: "accent" },
  { key: "completed", noun: "completed", tone: "neutral" },
];

/**
 * Classify one run into exactly one bucket. Failure and cancellation win over
 * openness so a failed run with a still-pending judge never reads as "running".
 */
function bucketFor(run: AutomationRun): AutomationRunsSummaryKey {
  if (run.status === "merged") {
    return "merged";
  }
  if (run.status === "cancelled") {
    return "cancelled";
  }
  if (describeRunFailure(run)) {
    return "failed";
  }
  if (isOpenAutomationRun(run)) {
    return "running";
  }
  return "completed";
}

function earliestCreatedAt(runs: AutomationRun[]): string | null {
  return runs.reduce<string | null>((earliest, run) => {
    if (!earliest) {
      return run.createdAt;
    }
    return run.createdAt < earliest ? run.createdAt : earliest;
  }, null);
}

export function summarizeAutomationRuns(runs: AutomationRun[]): AutomationRunsSummary {
  const totals = new Map<AutomationRunsSummaryKey, number>();
  for (const run of runs) {
    const key = bucketFor(run);
    totals.set(key, (totals.get(key) ?? 0) + 1);
  }
  const counts = BUCKETS.flatMap<AutomationRunsSummaryCount>((bucket) => {
    const count = totals.get(bucket.key) ?? 0;
    return count === 0
      ? []
      : [{ key: bucket.key, label: `${count} ${bucket.noun}`, tone: bucket.tone }];
  });
  return { counts, firstRunAt: earliestCreatedAt(runs), total: runs.length };
}

/** How this automation's runs chain onto each other, in plain language. */
export function describeAutomationChainMode(mode: AutomationChainMode): string {
  return mode === "merged_base"
    ? "every merge advances the base"
    : "each run stacks on the previous PR head";
}
