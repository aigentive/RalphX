import type { Automation, AutomationRun } from "@/api/automations";
import {
  OPEN_JUDGE_PENDING_STATE_SET,
  OPEN_AUTOMATION_RUN_STATUS_SET,
  SIGNAL_TERMINAL_AUTOMATION_RUN_STATUS_SET,
} from "./automationRunStatusSets";

/** Human labels for known automation run error codes. */
const ERROR_CODE_LABELS: Record<string, string> = {
  no_changes: "No changes to publish",
  publish_failed: "Publish failed",
  timeout: "Run timed out",
  agent_failed: "Agent run failed",
};

const REAL_SIGNAL_TERMINAL_STATUS_SET = new Set<AutomationRun["status"]>([
  "merged",
  "pr_closed",
  "agent_failed",
  "completed",
]);

const GOAL_AUTHORITY_JUDGE_STATES = new Set<AutomationRun["judgeState"]>([
  "none",
  "in_progress",
  "done",
]);

export const AUTOMATION_RUN_STATUS_LABELS: Record<AutomationRun["status"], string> = {
  pending: "Pending",
  provisioning: "Provisioning",
  running: "Running",
  awaiting_plan_approval: "Awaiting plan approval",
  published: "Published",
  completed: "Agent completed",
  merged: "Merged",
  pr_closed: "PR closed",
  agent_failed: "Agent failed",
  cancelled: "Cancelled",
};

export type AutomationRunStatusTone = "success" | "warning" | "error" | "neutral";

/**
 * Whether an automation can be deleted. Delete is allowed only from terminal or
 * pre-activation states: draft, completed, or stopped. Active/paused
 * automations must be stopped first (the backend rejects delete otherwise).
 */
export function isAutomationDeletable(status: Automation["status"]): boolean {
  return status === "draft" || status === "completed" || status === "stopped";
}

/** Count run conversations that will be archived when the automation is deleted. */
function runConversationCount(runs: AutomationRun[]): number {
  return runs.filter((run) => run.conversationId).length;
}

/** Count runs whose publication PR is still open (published, not merged/closed). */
function openPrCount(runs: AutomationRun[]): number {
  return runs.filter(
    (run) =>
      run.prNumber !== null &&
      run.prMergedAt === null &&
      run.status !== "pr_closed",
  ).length;
}

/**
 * Factual, short inventory of what deleting an automation destroys: archives the
 * setup + run conversations, closes any still-open publication PRs, archives the
 * linked spec (only when one is set), and hard-removes the automation and its run
 * history. Shared by the detail view and the Agents artifact panel confirm dialog.
 */
export function describeAutomationDeleteConsequences(
  automation: Automation,
  runs: AutomationRun[],
): string {
  const runConversations = runConversationCount(runs);
  const runNoun = runConversations === 1 ? "conversation" : "conversations";
  const parts: string[] = [
    automation.setupConversationId
      ? `Archives the setup conversation and ${runConversations} run ${runNoun}.`
      : `Archives ${runConversations} run ${runNoun}.`,
  ];

  const openPrs = openPrCount(runs);
  if (openPrs > 0) {
    parts.push(`Closes ${openPrs} open ${openPrs === 1 ? "PR" : "PRs"}.`);
  }
  if (automation.specArtifactId) {
    parts.push("Archives the linked spec.");
  }
  parts.push("Permanently removes the automation and its run history.");

  return parts.join(" ");
}

/** Pick the newest run (highest run index), or null when there are no runs. */
export function latestRun(runs: AutomationRun[]): AutomationRun | null {
  return runs.reduce<AutomationRun | null>(
    (latest, run) => (!latest || run.runIndex > latest.runIndex ? run : latest),
    null,
  );
}

/**
 * Whether a run is still open. Mirrors `ralphx-domain::is_open_automation_run`:
 * pending/provisioning/running/awaiting_plan_approval/published are always open,
 * and a signal-terminal run is open until its judge settles.
 */
export function isOpenAutomationRun(run: AutomationRun | null): run is AutomationRun {
  if (!run) {
    return false;
  }
  if (OPEN_AUTOMATION_RUN_STATUS_SET.has(run.status)) {
    return true;
  }
  return (
    SIGNAL_TERMINAL_AUTOMATION_RUN_STATUS_SET.has(run.status) &&
    OPEN_JUDGE_PENDING_STATE_SET.has(run.judgeState)
  );
}

/** Status-only cancellability; deliberately narrower than judge-aware openness. */
export function isAutomationRunCancellable(run: AutomationRun | null): run is AutomationRun {
  return Boolean(run && OPEN_AUTOMATION_RUN_STATUS_SET.has(run.status));
}

/**
 * Frontend mirror of backend `latest_run_holds_goal_authority`, intentionally
 * independent from `isOpenAutomationRun`.
 */
export function latestRunHoldsGoalAuthority(run: AutomationRun | null): boolean {
  if (!run) {
    return false;
  }
  if (OPEN_AUTOMATION_RUN_STATUS_SET.has(run.status)) {
    return true;
  }
  return (
    REAL_SIGNAL_TERMINAL_STATUS_SET.has(run.status) &&
    GOAL_AUTHORITY_JUDGE_STATES.has(run.judgeState)
  );
}

export function isAutomationRunComposerReadOnly(run: AutomationRun | null): boolean {
  if (!run || run.status === "awaiting_plan_approval") {
    return false;
  }
  return latestRunHoldsGoalAuthority(run);
}

export function getAutomationRunStatusLabel(run: AutomationRun | null): string {
  if (!run) {
    return "No run";
  }
  if (run.status === "awaiting_plan_approval") {
    if (run.planJudgeState === "in_progress") {
      return "Judging plan";
    }
    if (run.planRevisionPending) {
      return "Revision pending";
    }
    return "Awaiting plan approval";
  }
  return AUTOMATION_RUN_STATUS_LABELS[run.status];
}

export function getAutomationRunStatusTone(
  run: AutomationRun | null,
): AutomationRunStatusTone {
  if (!run) {
    return "neutral";
  }
  if (["running", "published", "merged", "completed"].includes(run.status)) {
    return "success";
  }
  if (["awaiting_plan_approval", "agent_failed", "pr_closed"].includes(run.status)) {
    return "warning";
  }
  if (run.status === "cancelled") {
    return "error";
  }
  return "neutral";
}

export function getAutomationRunJudgeLabel(run: AutomationRun | null): string | null {
  if (!run) {
    return null;
  }
  if (run.status === "cancelled") {
    return null;
  }
  switch (run.judgeState) {
    case "none":
      return "Judge pending";
    case "in_progress":
      return "Judging";
    case "done":
      return "Judge done";
    case "failed":
      return "Judge failed";
    case "skipped":
      return "Judge skipped";
  }
}

export interface AutomationRunPrView {
  rowLabel: "Current PR" | "Last PR";
  value: string;
}

function getAutomationRunPrView(run: AutomationRun | null): AutomationRunPrView {
  return {
    rowLabel: run && isOpenAutomationRun(run) ? "Current PR" : "Last PR",
    value: describeAutomationRunPrState(run),
  };
}

export interface AutomationRunView {
  isOpen: boolean;
  isCancellable: boolean;
  holdsGoalAuthority: boolean;
  composerReadOnly: boolean;
  statusLabel: string;
  statusTone: AutomationRunStatusTone;
  judgeLabel: string | null;
  stageLabel: string;
  pr: AutomationRunPrView;
}

export function getAutomationRunView(
  automation: Automation,
  run: AutomationRun | null,
): AutomationRunView {
  return {
    isOpen: isOpenAutomationRun(run),
    isCancellable: isAutomationRunCancellable(run),
    holdsGoalAuthority: latestRunHoldsGoalAuthority(run),
    composerReadOnly: isAutomationRunComposerReadOnly(run),
    statusLabel: getAutomationRunStatusLabel(run),
    statusTone: getAutomationRunStatusTone(run),
    judgeLabel: getAutomationRunJudgeLabel(run),
    stageLabel: describeAutomationStage(automation, run),
    pr: getAutomationRunPrView(run),
  };
}

/**
 * Describe the automation's live stage: the "what's happening now" line shared
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
  if (run.status === "awaiting_plan_approval") {
    if (run.planApprovedAt || run.planApprovedArtifactVersion !== null) {
      return "Approved — resuming";
    }
    return run.planJudgeState === "in_progress"
      ? "Judging plan"
      : "Awaiting plan approval";
  }
  if (run.status === "cancelled") {
    return "Cancelled";
  }
  if (run.judgeState === "in_progress") {
    return "Judging";
  }
  if (run.judgeState === "failed") {
    return "Paused: judge failed";
  }
  if (["pending", "provisioning", "running"].includes(run.status)) {
    return run.planPhase
      ? `Run ${run.runIndex} planning`
      : `Run ${run.runIndex} in progress`;
  }
  if (run.status === "completed" && run.judgeState === "none") {
    return "Waiting for judge";
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
    run.status === "agent_failed" ||
    run.status === "pr_closed" ||
    run.judgeState === "failed"
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

export function describeAutomationRunPrState(run: AutomationRun | null): string {
  if (!run) {
    return "No PR yet";
  }
  if (run.status === "awaiting_plan_approval") {
    return run.prNumber
      ? `PR #${run.prNumber} · Awaiting plan approval`
      : "Awaiting plan approval";
  }
  if (run.status === "cancelled") {
    return run.prNumber ? `PR #${run.prNumber} on cancelled run` : "Cancelled";
  }
  const prNumber = run.prNumber;
  const status = run.status;
  if (!prNumber) {
    return AUTOMATION_RUN_STATUS_LABELS[status];
  }
  if (isOpenAutomationRun(run)) {
    return `Current PR #${prNumber}`;
  }
  return `PR #${prNumber} · ${AUTOMATION_RUN_STATUS_LABELS[status]}`;
}
