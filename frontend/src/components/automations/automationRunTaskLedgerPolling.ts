import type { AutomationRunStatus } from "@/api/automations";

const ACTIVE_AGENT_TASK_REFRESH_MS = 2_500;
const PARKED_AGENT_TASK_REFRESH_MS = 15_000;
const UNCHANGED_RESPONSE_SLOWDOWN_THRESHOLD = 40;

export function automationRunTaskLedgerRefetchInterval(
  runStatus: AutomationRunStatus,
  unchangedResponses: number,
): number | false {
  if (runStatus === "running") {
    return unchangedResponses >= UNCHANGED_RESPONSE_SLOWDOWN_THRESHOLD
      ? PARKED_AGENT_TASK_REFRESH_MS
      : ACTIVE_AGENT_TASK_REFRESH_MS;
  }
  if (runStatus === "awaiting_plan_approval" || runStatus === "published") {
    return PARKED_AGENT_TASK_REFRESH_MS;
  }
  return false;
}
