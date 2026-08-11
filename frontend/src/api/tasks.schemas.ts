// Zod schemas for tasks API responses (snake_case from Rust backend)

import { z } from "zod";
import { TaskSchema } from "@/types/task";

export const TaskHistoryAvailabilityResponseSchemaRaw = z.object({
  has_history: z.boolean(),
  task_count: z.number().int().nonnegative(),
});

/**
 * Inject task response schema from Rust (snake_case)
 * Backend outputs snake_case (Rust default). Transform layer converts to camelCase for UI.
 */
export const InjectTaskResponseSchemaRaw = z.object({
  task: TaskSchema,
  target: z.enum(["backlog", "planned"]),
  priority: z.number().int(),
  make_next_applied: z.boolean(),
});

/**
 * Cleanup report response schema from Rust (snake_case)
 * Returned by cleanup_tasks_in_group command.
 */
export const CleanupReportResponseSchemaRaw = z.object({
  deleted_count: z.number().int(),
  failed_count: z.number().int(),
  stopped_agents: z.number().int(),
});

/**
 * Bulk cancel response schema from Rust (snake_case)
 * Returned by cancel_tasks_in_group command.
 */
export const BulkCancelResponseSchemaRaw = z.object({
  cancelled_count: z.number().int(),
});

/**
 * Bulk pause response schema from Rust (snake_case)
 * Returned by pause_tasks_in_group command.
 */
export const BulkPauseResponseSchemaRaw = z.object({
  paused_count: z.number().int(),
});

/**
 * Bulk resume response schema from Rust (snake_case)
 * Returned by resume_tasks_in_group command.
 */
export const BulkResumeResponseSchemaRaw = z.object({
  resumed_count: z.number().int(),
});

/**
 * Execution plan control response schema from Rust (snake_case)
 * Returned by pause_execution_plan/resume_execution_plan/stop_execution_plan.
 */
export const ExecutionPlanControlResponseSchemaRaw = z.object({
  execution_plan_id: z.string(),
  affected_count: z.number().int(),
});

/**
 * Bulk archive response schema from Rust (snake_case)
 * Returned by archive_tasks_in_group command.
 */
export const BulkArchiveResponseSchemaRaw = z.object({
  archived_count: z.number().int(),
});

/**
 * Unblock task response schema from Rust (snake_case)
 * Returned by unblock_task command. Includes the updated task and an optional
 * warning when one or more of its dependencies are in Failed status.
 */
export const UnblockTaskResponseSchemaRaw = z.object({
  task: TaskSchema,
  warning: z.string().nullable(),
});

/**
 * State transition response schema from Rust (snake_case)
 * Used by StateTimelineNav for displaying task state history.
 */
export const StateTransitionResponseSchemaRaw = z.object({
  /** Status transitioned from (null for initial state) */
  from_status: z.string().nullable(),
  /** Status transitioned to */
  to_status: z.string(),
  /** What triggered this transition (e.g., "user", "agent", "system") */
  trigger: z.string(),
  /** When the transition occurred (RFC3339 format) */
  timestamp: z.string(),
  /** Conversation ID for states that spawn conversations (executing, re_executing, reviewing) */
  conversation_id: z.string().nullish(),
  /** Agent run ID for the specific execution within the conversation */
  agent_run_id: z.string().nullish(),
  /** Optional runtime context type for the associated transcript */
  context_type: z.string().nullish(),
  /** Optional stable transition identity */
  transition_id: z.string().nullish(),
});
