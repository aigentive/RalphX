import type { Automation, AutomationRun } from "@/api/automations";

/** Human labels for known automation run error codes. */
const ERROR_CODE_LABELS: Record<string, string> = {
  no_changes: "No changes to publish",
  publish_failed: "Publish failed",
  timeout: "Run timed out",
  agent_failed: "Agent run failed",
};

/** Pick the newest run (highest run index), or null when there are no runs. */
export function latestRun(runs: AutomationRun[]): AutomationRun | null {
  return runs.reduce<AutomationRun | null>(
    (latest, run) => (!latest || run.runIndex > latest.runIndex ? run : latest),
    null,
  );
}

/**
 * Describe the automation's live stage — the "what's happening now" line shared
 * by the Automations list, the Agents artifact panel, and the detail view.
 */
export function describeAutomationStage(
  automation: Automation,
  run: AutomationRun | null,
): string {
  if (automation.status === "draft") {
    return "Draft setup";
  }
  if (automation.status === "paused") {
    return automation.pausedReasonCode
      ? `Paused: ${automation.pausedReasonCode}`
      : "Paused";
  }
  if (automation.status === "completed") {
    return "Goal completed";
  }
  if (automation.status === "stopped") {
    return "Stopped";
  }
  if (!run) {
    return "Waiting for first run";
  }
  if (run.judgeState === "in_progress") {
    return "Judging";
  }
  if (run.judgeState === "failed") {
    return "Paused: judge failed";
  }
  if (["pending", "provisioning", "running"].includes(run.status)) {
    return `Run ${run.runIndex} in progress`;
  }
  if (run.status === "published") {
    return run.prNumber
      ? `Waiting for PR #${run.prNumber} to merge`
      : "Waiting for PR merge";
  }
  if (run.judgeState === "none") {
    return "Waiting for judge";
  }
  return "Scheduling next run";
}

function isFailedRun(run: AutomationRun): boolean {
  return (
    run.status === "agent_failed"
    || run.status === "pr_closed"
    || run.judgeState === "failed"
  );
}

/**
 * Formatted failure reason for a run, or null when the run has not failed.
 * Prefers `errorDetail`, then a human label derived from `errorCode`, then a
 * status/judge-specific fallback.
 */
export function describeRunFailure(run: AutomationRun | null): string | null {
  if (!run || !isFailedRun(run)) {
    return null;
  }

  const detail = run.errorDetail?.trim();
  if (detail) {
    return detail;
  }

  const code = run.errorCode?.trim();
  if (code) {
    return ERROR_CODE_LABELS[code] ?? code;
  }

  if (run.status === "pr_closed") {
    return "PR closed without merging";
  }
  if (run.status === "agent_failed") {
    return "Agent run failed";
  }
  return "Judge failed";
}
