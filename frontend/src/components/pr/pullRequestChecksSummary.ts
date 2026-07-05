import type { PullRequestCheck } from "@/api/github";

/**
 * Conclusions that count as a failed check. Anything not listed here that is
 * `completed` counts as passed; anything not yet `completed` counts as pending.
 */
const FAILED_CONCLUSIONS = new Set([
  "failure",
  "timed_out",
  "cancelled",
  "action_required",
  "startup_failure",
  "stale",
]);

const COMPLETED_STATUS = "completed";

export interface ChecksSummary {
  total: number;
  passed: number;
  failed: number;
  pending: number;
  /** Checks whose conclusion is in {@link FAILED_CONCLUSIONS}. */
  failing: PullRequestCheck[];
}

type CheckBucket = "passed" | "failed" | "pending";

/** Classify a single check into passed / failed / pending. */
export function bucketCheck(check: PullRequestCheck): CheckBucket {
  const conclusion = (check.conclusion ?? "").toLowerCase();
  if (FAILED_CONCLUSIONS.has(conclusion)) {
    return "failed";
  }
  const status = (check.status ?? "").toLowerCase();
  // Not finished yet, or finished without a recorded conclusion → pending.
  if (status !== COMPLETED_STATUS || conclusion === "") {
    return "pending";
  }
  return "passed";
}

/** Summarize a list of PR checks into counts + the failing subset. */
export function summarizeChecks(checks: PullRequestCheck[]): ChecksSummary {
  const summary: ChecksSummary = {
    total: checks.length,
    passed: 0,
    failed: 0,
    pending: 0,
    failing: [],
  };
  for (const check of checks) {
    const bucket = bucketCheck(check);
    if (bucket === "failed") {
      summary.failed += 1;
      summary.failing.push(check);
    } else if (bucket === "pending") {
      summary.pending += 1;
    } else {
      summary.passed += 1;
    }
  }
  return summary;
}
