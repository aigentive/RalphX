import type { AutomationRun } from "@/api/automations";

export const OPEN_AUTOMATION_RUN_STATUSES = [
  "pending",
  "provisioning",
  "running",
  "awaiting_plan_approval",
  "published",
] as const satisfies readonly AutomationRun["status"][];

export const SIGNAL_TERMINAL_AUTOMATION_RUN_STATUSES = [
  "merged",
  "pr_closed",
  "agent_failed",
  "completed",
] as const satisfies readonly AutomationRun["status"][];

// Keep in lockstep with the judge-pending arm of
// `crates/ralphx-domain/src/entities/automation.rs::is_open_automation_run`.
export const OPEN_JUDGE_PENDING_STATES = [
  "none",
  "in_progress",
  "failed",
] as const satisfies readonly AutomationRun["judgeState"][];

export const TERMINAL_AUTOMATION_RUN_STATUSES = [
  "merged",
  "pr_closed",
  "agent_failed",
  "completed",
  "cancelled",
] as const satisfies readonly AutomationRun["status"][];

export const OPEN_AUTOMATION_RUN_STATUS_SET = new Set<AutomationRun["status"]>(
  OPEN_AUTOMATION_RUN_STATUSES,
);

export const SIGNAL_TERMINAL_AUTOMATION_RUN_STATUS_SET = new Set<
  AutomationRun["status"]
>(SIGNAL_TERMINAL_AUTOMATION_RUN_STATUSES);

export const OPEN_JUDGE_PENDING_STATE_SET = new Set<
  AutomationRun["judgeState"]
>(OPEN_JUDGE_PENDING_STATES);

export const TERMINAL_AUTOMATION_RUN_STATUS_SET = new Set<
  AutomationRun["status"]
>(TERMINAL_AUTOMATION_RUN_STATUSES);
