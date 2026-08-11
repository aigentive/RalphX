// Tauri invoke wrappers for running processes API with type safety using Zod schemas

import { invoke } from "@tauri-apps/api/core";
import { z } from "zod";
import { RunningProcessesResponseSchema } from "./running-processes.schemas";
import { transformRunningProcessesResponse } from "./running-processes.transforms";
import type { RunningProcessesResponse } from "./running-processes.types";

// Re-export types for convenience
export type {
  StepProgressSummary,
  RunningProcess,
  RunningProcessesResponse,
  RunningIdeationSession,
  RunningWorkspaceSession,
  ExecutionLaneUsage,
  ExecutionCapacitySummary,
  ExecutionLaneName,
} from "./running-processes.types";

// Re-export schemas for consumers that need validation
export {
  StepProgressSummarySchema,
  RunningProcessSchema,
  RunningProcessesResponseSchema,
  RunningIdeationSessionSchema,
  RunningWorkspaceSessionSchema,
  ExecutionLaneUsageSchema,
  ExecutionCapacitySummarySchema,
  ExecutionLaneNameSchema,
} from "./running-processes.schemas";

// Re-export transforms for consumers that need manual transformation
export {
  transformStepProgressSummary,
  transformRunningProcess,
  transformRunningIdeationSession,
  transformRunningWorkspaceSession,
  transformExecutionLaneUsage,
  transformExecutionCapacitySummary,
  transformRunningProcessesResponse,
} from "./running-processes.transforms";

// ============================================================================
// Typed Invoke Helper
// ============================================================================

async function typedInvokeWithTransform<TRaw, TResult>(
  cmd: string,
  args: Record<string, unknown>,
  schema: z.ZodType<TRaw>,
  transform: (raw: TRaw) => TResult
): Promise<TResult> {
  const result = await invoke(cmd, args);
  const validated = schema.parse(result);
  return transform(validated);
}

// ============================================================================
// API Object
// ============================================================================

/**
 * Running processes API wrappers for Tauri commands
 */
export const runningProcessesApi = {
  /**
   * Get currently running processes (tasks in agent-active states).
   * If projectId is provided, results are scoped to that project.
   * @returns List of running processes with enriched data
   */
  getRunningProcesses: (projectId?: string): Promise<RunningProcessesResponse> =>
    typedInvokeWithTransform(
      "get_running_processes",
      { projectId: projectId ?? null },
      RunningProcessesResponseSchema,
      transformRunningProcessesResponse
    ),
};
