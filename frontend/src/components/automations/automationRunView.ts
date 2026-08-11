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

export const PAUSED_REASON_LABELS: Record<string, string> = {
  judge_stopped_unmet: "Judge stopped — goal unmet",
  workspace_review_blocked: "Workspace review blocked",
  max_consecutive_failures: "Too many consecutive failures",
  max_runs_exhausted: "Maximum runs reached",
  plan_revision_exhausted: "Plan revision limit reached",
  judge_failed: "Judge failed",
  plan_judge_failed: "Plan judge failed",
};

export function describePausedReason(code: string): string {
  const knownLabel = PAUSED_REASON_LABELS[code];
  if (knownLabel) {
    return knownLabel;
  }
  const normalized = code.replace(/_/g, " ").trim();
  return normalized
    ? `${normalized.charAt(0).toUpperCase()}${normalized.slice(1)}`
    : "Paused";
}

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

export const AUTOMATION_CANCEL_CONFIRMATION_DESCRIPTION =
  "Cancels any open run and stops automatic scheduling. Work, artifacts, conversations, branches, and PRs created so far are retained. The cancelled run cannot be resumed. Restart creates a new run.";

export const CANCELLED_RUN_RESTART_DESCRIPTION =
  "The last run was cancelled. Run now starts a new run from that run's prompt; it does not resume the cancelled run.";

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

export type AutomationRunStatusTone =
  | "success"
  | "warning"
  | "error"
  | "neutral"
  | "accent";

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

/** A run may be deleted from the timeline only when it is the latest run AND its
 *  status is failed, running, or stopped. A `running` run is stopped-then-deleted
 *  by the backend. Completed/published/merged/pending/provisioning/awaiting_plan_approval
 *  are never deletable. Latest-ness is enforced by the caller and re-checked by the backend. */
export function isAutomationRunDeletable(run: AutomationRun | null): run is AutomationRun {
  return !!run && (
    run.status === "agent_failed"
    || run.status === "running"
    || run.status === "cancelled"
  );
}

/** A run may be resumed (reopened in place) only when it is the latest run AND
 *  it failed. Latest-ness is enforced by the caller and re-checked by the backend. */
export function isAutomationRunResumable(run: AutomationRun | null): run is AutomationRun {
  return !!run && run.status === "agent_failed";
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

/**
 * Lockstep mirror of the missing `Cancelled` arm in the Rust scheduler's
 * `observe_latest_run`: an active automation with a cancelled latest run is
 * idle until the user explicitly starts another run.
 */
export function isIdleAfterCancelledRun(
  automation: Automation,
  latestRun: AutomationRun | null,
): boolean {
  return automation.status === "active" && latestRun?.status === "cancelled";
}

export function getAutomationRunStatusLabel(run: AutomationRun | null): string {
  if (!run) {
    return "No run";
  }
  if (run.status === "awaiting_plan_approval") {
    if (run.planJudgeState === "in_progress") {
      return "Plan judge running";
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
  // Accent marks live activity; green is reserved for settled success states.
  if (run.status === "running") {
    return "accent";
  }
  if (["published", "merged", "completed"].includes(run.status)) {
    return "success";
  }
  if (run.status === "awaiting_plan_approval") {
    return "warning";
  }
  if (["agent_failed", "pr_closed"].includes(run.status)) {
    return "error";
  }
  return "neutral";
}

export function getAutomationRunJudgeLabel(
  run: AutomationRun | null,
  automation: Automation | null = null,
): string | null {
  if (!run) {
    return null;
  }
  if (run.status === "cancelled") {
    return null;
  }
  if (run.status === "awaiting_plan_approval") {
    if (automation?.planApprovalMode !== "automatic" && run.planJudgeState === "none") {
      return null;
    }
    switch (run.planJudgeState) {
      case "none":
        return "Plan judge pending";
      case "in_progress":
        return "Plan judge running";
      case "done":
        return "Plan judge complete";
      case "failed":
        return "Plan judge failed";
    }
  }
  if (!REAL_SIGNAL_TERMINAL_STATUS_SET.has(run.status)) {
    return null;
  }
  switch (run.judgeState) {
    case "none":
      return "Terminal judge pending";
    case "in_progress":
      return "Terminal judge running";
    case "done":
      return "Terminal judge complete";
    case "failed":
      return "Terminal judge failed";
    case "skipped":
      return "Terminal judge skipped";
  }
}

export type AutomationJudgeRecoveryKind = "plan" | "terminal";

export interface AutomationJudgeRecovery {
  kind: AutomationJudgeRecoveryKind;
  statusLabel: string;
  description: string;
  actionLabel: "Retry plan judge" | "Retry terminal judge";
}

export function getAutomationJudgeRecovery(
  automation: Automation,
  run: AutomationRun | null,
): AutomationJudgeRecovery | null {
  if (
    run?.status === "awaiting_plan_approval" &&
    automation.planApprovalMode === "automatic" &&
    run.planJudgeState === "failed"
  ) {
    return {
      kind: "plan",
      statusLabel: "Plan judge failed",
      description:
        "The plan judge did not complete. Retry evaluates the current run plan.",
      actionLabel: "Retry plan judge",
    };
  }
  if (
    run &&
    REAL_SIGNAL_TERMINAL_STATUS_SET.has(run.status) &&
    run.judgeState === "failed"
  ) {
    return {
      kind: "terminal",
      statusLabel: "Terminal judge failed",
      description:
        "The terminal judge did not complete. Retry evaluates this finished run.",
      actionLabel: "Retry terminal judge",
    };
  }
  return null;
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
    judgeLabel: getAutomationRunJudgeLabel(run, automation),
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
      ? describePausedReason(automation.pausedReasonCode)
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
    if (run.planJudgeState === "in_progress") {
      return "Plan judge running";
    }
    if (run.planJudgeState === "failed") {
      return "Plan judge failed";
    }
    return automation.planApprovalMode === "automatic"
      ? "Plan judge pending"
      : "Awaiting plan approval";
  }
  if (run.status === "cancelled") {
    return "Cancelled";
  }
  if (run.judgeState === "in_progress") {
    return "Terminal judge running";
  }
  if (run.judgeState === "failed") {
    return "Terminal judge failed";
  }
  if (["pending", "provisioning", "running"].includes(run.status)) {
    return run.planPhase
      ? `Run ${run.runIndex} planning`
      : `Run ${run.runIndex} in progress`;
  }
  if (run.status === "completed" && run.judgeState === "none") {
    return "Terminal judge pending";
  }
  if (run.status === "published") {
    return run.prNumber
      ? `Waiting for PR #${run.prNumber} to merge`
      : "Waiting for PR merge";
  }
  if (run.judgeState === "none") {
    return "Terminal judge pending";
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

export interface RunTimelineHighlight {
  backgroundColor: string;
  borderColor: string;
  markerColor: string;
  /** Left edge rule on the Runs-timeline card — carries status at a glance. */
  accentColor: string;
  /** Whether this run is live and should animate its timeline marker. */
  live: boolean;
}

/**
 * Per-status soft card treatment, shared by the Runs-timeline cards and the Agents
 * automation panel's runs list so both surfaces read identically at a glance:
 * merged = soft green, running/active = soft accent (orange), failed = soft darker
 * surface with an error marker. Everything else stays neutral.
 */
export function runTimelineHighlight(run: AutomationRun): RunTimelineHighlight {
  if (run.status === "merged") {
    return {
      backgroundColor: "var(--status-success-muted)",
      borderColor: "var(--status-success-border)",
      markerColor: "var(--status-success, #3fbf7f)",
      accentColor: "var(--status-success-strong, #3fbf7f)",
      live: false,
    };
  }
  const isActive =
    isOpenAutomationRun(run) && run.status !== "cancelled" && !describeRunFailure(run);
  if (isActive) {
    return {
      backgroundColor: "var(--accent-muted)",
      borderColor: "var(--accent-border)",
      markerColor: "var(--accent-primary, #ff6a35)",
      accentColor: "var(--accent-primary, #ff6a35)",
      live: true,
    };
  }
  if (describeRunFailure(run)) {
    return {
      backgroundColor: "var(--bg-surface, #1c1c21)",
      borderColor: "var(--border-default, #393940)",
      markerColor: "var(--status-error, #d55e00)",
      accentColor: "var(--status-error-strong, #d55e00)",
      live: false,
    };
  }
  return {
    backgroundColor: "var(--bg-elevated, #232329)",
    borderColor: "var(--border-subtle, #2e2e36)",
    markerColor: "var(--text-subtle, #6b6b73)",
    accentColor: "var(--border-strong, #44444d)",
    live: false,
  };
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
