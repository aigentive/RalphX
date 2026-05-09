import type { ExecutionHaltMode } from "@/api/execution.types";

export type AgentQueueHaltState = "paused" | "stopped" | null;

interface ExecutionPauseSnapshot {
  isPaused: boolean;
  haltMode?: ExecutionHaltMode | null;
}

export function getAgentQueueHaltState(
  status: ExecutionPauseSnapshot
): AgentQueueHaltState {
  if (status.haltMode === "stopped") {
    return "stopped";
  }
  if (status.isPaused || status.haltMode === "paused") {
    return "paused";
  }
  return null;
}
