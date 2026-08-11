// Zod schemas for running processes API responses (snake_case from Rust backend)

import { z } from "zod";
import { TaskStepResponseSchema } from "@/types/task-step";
import { ExecutionTaskAgentWorkspaceSchema } from "./execution-task-agent-workspace";

/**
 * Step progress summary schema from Rust (snake_case)
 */
export const StepProgressSummarySchema = z.object({
  task_id: z.string(),
  total: z.number().int().nonnegative(),
  completed: z.number().int().nonnegative(),
  in_progress: z.number().int().nonnegative(),
  pending: z.number().int().nonnegative(),
  skipped: z.number().int().nonnegative(),
  failed: z.number().int().nonnegative(),
  current_step: TaskStepResponseSchema.nullable(),
  next_step: TaskStepResponseSchema.nullable(),
  percent_complete: z.number(),
});

/**
 * Running process schema from Rust (snake_case)
 */
export const RunningProcessSchema = z.object({
  task_id: z.string(),
  title: z.string(),
  internal_status: z.string(),
  step_progress: StepProgressSummarySchema.nullable(),
  elapsed_seconds: z.number().int().nullable(),
  trigger_origin: z.string().nullable(),
  task_branch: z.string().nullable(),
  agent_workspace: ExecutionTaskAgentWorkspaceSchema.nullable().optional(),
});

/**
 * Running ideation session schema from Rust (snake_case)
 */
export const RunningIdeationSessionSchema = z.object({
  session_id: z.string(),
  title: z.string(),
  elapsed_seconds: z.number().int().nullable(),
  is_generating: z.boolean(),
});

/**
 * Running workspace conversation schema from Rust (snake_case)
 */
export const RunningWorkspaceSessionSchema = z.object({
  conversation_id: z.string(),
  project_id: z.string(),
  automation_id: z.string().nullable(),
  automation_run_id: z.string().nullable(),
  title: z.string(),
  elapsed_seconds: z.number().int().nullable(),
  model: z.string().nullable(),
});

export const ExecutionLaneNameSchema = z.enum(["workspaces", "tasks", "ideation"]);

/**
 * Execution lane usage schema from Rust (snake_case)
 */
export const ExecutionLaneUsageSchema = z.object({
  lane: ExecutionLaneNameSchema,
  active: z.number().int().nonnegative(),
  idle: z.number().int().nonnegative(),
  waiting: z.number().int().nonnegative(),
  max: z.number().int().nonnegative(),
  borrowed: z.number().int().nonnegative(),
  priority_rank: z.number().int().positive(),
});

/**
 * Execution capacity summary schema from Rust (snake_case)
 */
export const ExecutionCapacitySummarySchema = z.object({
  total_active: z.number().int().nonnegative(),
  global_max_concurrent: z.number().int().nonnegative(),
  borrowing_enabled: z.boolean(),
  priority: z.array(ExecutionLaneNameSchema),
});

/**
 * Running processes response schema from Rust (snake_case)
 */
export const RunningProcessesResponseSchema = z.object({
  processes: z.array(RunningProcessSchema),
  ideation_sessions: z.array(RunningIdeationSessionSchema),
  workspace_sessions: z.array(RunningWorkspaceSessionSchema).default([]),
  lanes: z.array(ExecutionLaneUsageSchema).default([]),
  capacity: ExecutionCapacitySummarySchema.default({
    total_active: 0,
    global_max_concurrent: 0,
    borrowing_enabled: false,
    priority: ["workspaces", "tasks", "ideation"],
  }),
});
