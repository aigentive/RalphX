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
] as const satisfies readonly AutomationRun["status"][];

export const TERMINAL_AUTOMATION_RUN_STATUSES = [
  "merged",
  "pr_closed",
  "agent_failed",
  "cancelled",
] as const satisfies readonly AutomationRun["status"][];

export const OPEN_AUTOMATION_RUN_STATUS_SET = new Set<AutomationRun["status"]>(
  OPEN_AUTOMATION_RUN_STATUSES,
);

export const SIGNAL_TERMINAL_AUTOMATION_RUN_STATUS_SET = new Set<
  AutomationRun["status"]
>(SIGNAL_TERMINAL_AUTOMATION_RUN_STATUSES);

export const TERMINAL_AUTOMATION_RUN_STATUS_SET = new Set<
  AutomationRun["status"]
>(TERMINAL_AUTOMATION_RUN_STATUSES);
