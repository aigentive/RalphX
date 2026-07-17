// Task types and Zod schema
// Must match the Rust backend Task struct

import { z } from "zod";
import { InternalStatusSchema, type InternalStatus } from "./status";

// Re-export InternalStatus for convenience
export type { InternalStatus };

/**
 * Task schema matching Rust backend serialization (snake_case)
 * Backend outputs snake_case (Rust default). Transform layer converts to camelCase for UI.
 *
 * Note: Backend TaskResponse does NOT include source_proposal_id or plan_artifact_id.
 * Those fields are in the Task entity but not exposed in API responses.
 * Use get_task_context to fetch them if needed.
 */
export const TaskSchema = z.object({
  id: z.string().min(1),
  project_id: z.string().min(1),
  category: z.string().min(1),
  title: z.string().min(1),
  description: z.string().nullable(),
  priority: z.number().int(),
  internal_status: InternalStatusSchema,
  /** Whether this task needs a review point (human-in-loop checkpoint) */
  needs_review_point: z.boolean().default(false),
  /** Ideation session ID (plan association). Backend Option<String> → null when the
   * task has no plan (e.g. Kanban-created), so accept null as well as absent. */
  ideation_session_id: z.string().nullable().optional(),
  /** Execution plan ID for implementation-flow tasks */
  execution_plan_id: z.string().nullable().optional(),
  // Accept RFC3339 timestamps with offset (e.g., +00:00)
  created_at: z.string().datetime({ offset: true }),
  updated_at: z.string().datetime({ offset: true }),
  started_at: z.string().datetime({ offset: true }).nullable(),
  completed_at: z.string().datetime({ offset: true }).nullable(),
  archived_at: z.string().datetime({ offset: true }).nullable(),
  blocked_reason: z.string().nullable(),
  // Git branch isolation fields (Phase 66)
  task_branch: z.string().nullable().optional(),
  worktree_path: z.string().nullable().optional(),
  merge_commit_sha: z.string().nullable().optional(),
  metadata: z.string().nullable().optional(),
});

/**
 * Frontend Task type (camelCase)
 * This is what components and stores use. Transformed from snake_case API responses.
 *
 * Note: sourceProposalId and planArtifactId are not included in TaskResponse.
 * They exist in the database but aren't serialized in API responses.
 * These fields are undefined unless explicitly populated from TaskContext.
 */
export interface Task {
  id: string;
  projectId: string;
  category: string;
  title: string;
  description: string | null;
  priority: number;
  internalStatus: InternalStatus;
  needsReviewPoint: boolean;
  /** Ideation session ID (plan association) */
  ideationSessionId?: string | undefined;
  /** Execution plan ID for implementation-flow tasks */
  executionPlanId?: string | undefined;
  createdAt: string;
  updatedAt: string;
  startedAt: string | null;
  completedAt: string | null;
  archivedAt: string | null;
  blockedReason: string | null;
  /** Not in TaskResponse - fetch via get_task_context */
  sourceProposalId?: string | null;
  /** Not in TaskResponse - fetch via get_task_context */
  planArtifactId?: string | null;
  // Git branch isolation fields (Phase 66)
  /** Branch name for this task (both Local and Worktree modes) */
  taskBranch?: string | null;
  /** Worktree path for this task (Worktree mode only) */
  worktreePath?: string | null;
  /** Merge commit SHA after successful merge */
  mergeCommitSha?: string | null;
  /** Task metadata as JSON string (e.g., conflict_files for merge states) */
  metadata?: string | null;
}

/**
 * Transform function to convert snake_case API response to camelCase frontend type
 */
export function transformTask(raw: z.infer<typeof TaskSchema>): Task {
  return {
    id: raw.id,
    projectId: raw.project_id,
    category: raw.category,
    title: raw.title,
    description: raw.description,
    priority: raw.priority,
    internalStatus: raw.internal_status,
    needsReviewPoint: raw.needs_review_point,
    ideationSessionId: raw.ideation_session_id ?? undefined,
    executionPlanId: raw.execution_plan_id ?? undefined,
    createdAt: raw.created_at,
    updatedAt: raw.updated_at,
    startedAt: raw.started_at,
    completedAt: raw.completed_at,
    archivedAt: raw.archived_at,
    blockedReason: raw.blocked_reason,
    taskBranch: raw.task_branch ?? null,
    worktreePath: raw.worktree_path ?? null,
    mergeCommitSha: raw.merge_commit_sha ?? null,
    metadata: raw.metadata ?? null,
  };
}

/**
 * Common task categories
 */
export const TASK_CATEGORIES = [
  "feature",
  "bug",
  "chore",
  "docs",
  "test",
  "refactor",
] as const;

export const TaskCategorySchema = z.enum(TASK_CATEGORIES);
export type TaskCategory = z.infer<typeof TaskCategorySchema>;

/**
 * Schema for creating a new task
 * Excludes auto-generated fields (id, timestamps, status)
 */
export const CreateTaskSchema = z.object({
  projectId: z.string().min(1, "Project ID is required"),
  category: z.string().min(1, "Category is required").default("feature"),
  title: z.string().min(1, "Title is required"),
  description: z.string().optional(),
  priority: z.number().int().default(0),
  /** Override for QA enablement. null means inherit from global settings. */
  needsQa: z.boolean().nullable().optional(),
  /** Optional list of step titles to create for this task */
  steps: z.array(z.string()).optional(),
  /** Optional ideation session association for plan-scoped task creation */
  ideationSessionId: z.string().optional(),
  /** Optional execution plan association for implementation-flow task creation */
  executionPlanId: z.string().optional(),
});

export type CreateTask = z.infer<typeof CreateTaskSchema>;

/**
 * Schema for updating a task
 * All fields are optional
 */
export const UpdateTaskSchema = z.object({
  category: z.string().min(1).optional(),
  title: z.string().min(1).optional(),
  description: z.string().nullable().optional(),
  priority: z.number().int().optional(),
});

export type UpdateTask = z.infer<typeof UpdateTaskSchema>;

/**
 * Schema for task list response (backend uses array)
 */
export const TaskListSchema = z.array(TaskSchema);
export type TaskList = Task[];

/**
 * Schema for paginated task list response (snake_case from backend)
 */
export const TaskListResponseSchema = z.object({
  tasks: z.array(TaskSchema),
  total: z.number(),
  has_more: z.boolean(),
  offset: z.number(),
});

/**
 * Frontend TaskListResponse type (camelCase)
 */
export interface TaskListResponse {
  tasks: Task[];
  total: number;
  hasMore: boolean;
  offset: number;
}

/**
 * Transform function for TaskListResponse
 */
export function transformTaskListResponse(raw: z.infer<typeof TaskListResponseSchema>): TaskListResponse {
  return {
    tasks: raw.tasks.map(transformTask),
    total: raw.total,
    hasMore: raw.has_more,
    offset: raw.offset,
  };
}

/**
 * Schema for status transition option
 */
export const StatusTransitionSchema = z.object({
  status: z.string(),
  label: z.string(),
});
export type StatusTransition = z.infer<typeof StatusTransitionSchema>;

/**
 * Merge recovery event types
 * These track the history of merge deferral and retry attempts
 */
export type MergeRecoveryEventKind =
  | "deferred"
  | "auto_retry_triggered"
  | "attempt_started"
  | "attempt_failed"
  | "attempt_succeeded"
  | "manual_retry";

export type MergeRecoveryEventSource = "system" | "auto" | "user";

export type MergeRecoveryReasonCode =
  | "target_branch_busy"
  | "git_error"
  | "validation_failed"
  | "unknown";

export interface MergeRecoveryEvent {
  /** ISO 8601 timestamp */
  at: string;
  /** Type of event */
  kind: MergeRecoveryEventKind;
  /** Source of the event (system/auto/user) */
  source: MergeRecoveryEventSource;
  /** Machine-readable reason code */
  reason_code: MergeRecoveryReasonCode;
  /** Human-readable summary */
  message: string;
  /** Target branch being merged into */
  target_branch?: string;
  /** Source branch being merged from */
  source_branch?: string;
  /** ID of task blocking this merge */
  blocking_task_id?: string;
  /** Attempt number for this retry */
  attempt?: number;
}

export type MergeRecoveryLastState = "deferred" | "retrying" | "failed" | "succeeded";

export interface MergeRecoveryState {
  /** Schema version for future compatibility */
  version: number;
  /** Chronological list of recovery events (newest last) */
  events: MergeRecoveryEvent[];
  /** Current state of recovery */
  last_state: MergeRecoveryLastState;
}

// ============================================================================
// StopMetadata - Captures context when a task is stopped mid-execution
// ============================================================================

/**
 * Metadata captured when a task is stopped mid-execution.
 *
 * Enables "smart resume" capability where the system can restore
 * the task to its previous state with context about why it was stopped.
 *
 * Schema from backend:
 * ```json
 * {
 *   "stopped_from_status": "merging",
 *   "stop_reason": "User stopped to protect main branch",
 *   "stopped_at": "2026-02-15T10:30:00Z"
 * }
 * ```
 */
export interface StopMetadata {
  /** The status the task was in when stopped (snake_case string) */
  stoppedFromStatus: string;
  /** Optional reason provided by user for stopping */
  stopReason?: string;
  /** Timestamp when the task was stopped (RFC3339 format) */
  stoppedAt: string;
}

/**
 * Parse stop metadata from a task's metadata JSON string.
 *
 * @param metadata - The task.metadata JSON string (may be null)
 * @returns StopMetadata if present and valid, null otherwise
 */
export function parseStopMetadata(metadata: string | null | undefined): StopMetadata | null {
  if (!metadata) return null;
  try {
    const obj = JSON.parse(metadata);
    const stopJson = obj?.stop_metadata;
    if (!stopJson || typeof stopJson !== "string") return null;
    const parsed = JSON.parse(stopJson);
    if (!parsed?.stopped_from_status || !parsed?.stopped_at) return null;
    return {
      stoppedFromStatus: parsed.stopped_from_status,
      stopReason: parsed.stop_reason ?? undefined,
      stoppedAt: parsed.stopped_at,
    };
  } catch {
    return null;
  }
}

/**
 * Failure source classification — matches Rust ExecutionFailureSource enum.
 * Transient sources are auto-retryable; terminal sources require manual intervention.
 */
export type FailureSource =
  | "transient_timeout"
  | "agent_crash"
  | "local_tool_failed"
  | "parse_stall"
  | "git_isolation"
  | "wall_clock_timeout"
  | "max_retries_exceeded"
  | "provider_error"
  | "validation_failed"
  | "unknown";

/** Sources that are transient (auto-retrying, shown as amber badge) */
export const TRANSIENT_FAILURE_SOURCES = new Set<FailureSource>([
  "transient_timeout",
  "agent_crash",
  "parse_stall",
  "git_isolation",
]);

/**
 * Extended metadata structure with merge recovery and stop info
 */
export interface TaskMetadata {
  /** Error message (legacy field) */
  error?: string;
  /** Source branch (legacy field) */
  source_branch?: string;
  /** Target branch (legacy field) */
  target_branch?: string;
  /** Diagnostic info (legacy field) */
  diagnostic_info?: string;
  /** Validation failures (legacy field) */
  validation_failures?: unknown[];
  /** Machine-readable error code (e.g. "plan_branch_repo_not_wired", "feature_branch_missing") */
  error_code?: string;
  /** Structured merge recovery timeline */
  merge_recovery?: MergeRecoveryState;
  /** Repository hook failure classification from merge recovery */
  merge_hook_failure_kind?: "policy_failure" | "environment_failure" | "unknown" | string;
  /** Why RalphX blocked the hook failure instead of rerouting it */
  merge_hook_blocked_reason?: "hook_environment_failure" | "repeated_hook_failure" | string;
  /** Full hook environment/bootstrap failure output */
  merge_hook_environment_error?: string;
  /** Full repeated hook failure output */
  merge_hook_repeated_error?: string;
  /** Repeated hook failure count for loop prevention */
  merge_hook_failure_repeat_count?: number;
  /** Stop metadata for smart resume */
  stop?: StopMetadata;
}

// ============================================================================
// Restart Result Types
// ============================================================================

/**
 * Category of resume behavior based on the stopped_from_status.
 *
 * - Direct: Resume directly to original state (spawn agent if needed)
 * - Validated: Validate git state before resuming
 * - Redirect: Resume to successor state
 */
export type ResumeCategory = "direct" | "validated" | "redirect";

/**
 * Validation warning for resume operations.
 */
export interface ResumeValidationWarning {
  /** Warning code (e.g., "dirty_worktree", "base_branch_moved") */
  code: string;
  /** Human-readable warning message */
  message: string;
}

export type RestartDisposition =
  | "recovered_to_review"
  | "restarted_to_ready";

/**
 * Result of a restart_task command (frontend camelCase type).
 *
 * Success variant: Task was successfully restarted
 * ValidationFailed variant: Validation failed (only for Validated category)
 */
export type RestartResult =
  | {
      type: "Success";
      task: Record<string, unknown>;
      category: ResumeCategory;
      resumedToStatus: string;
      disposition?: RestartDisposition;
    }
  | {
      type: "ValidationFailed";
      warnings: ResumeValidationWarning[];
      stoppedFromStatus: string;
    }
  | {
      type: "Blocked";
      warnings: ResumeValidationWarning[];
    };

// Zod schemas for restart result validation (snake_case from backend)

export const ResumeValidationWarningSchema = z.object({
  code: z.string(),
  message: z.string(),
});

export const ResumeCategorySchema = z.enum(["direct", "validated", "redirect"]);
export const RestartDispositionSchema = z.enum([
  "recovered_to_review",
  "restarted_to_ready",
]);

/** Raw schema for backend response (snake_case) */
export const RestartResultSchemaRaw = z.discriminatedUnion("type", [
  z.object({
    type: z.literal("Success"),
    task: z.record(z.string(), z.unknown()),
    category: ResumeCategorySchema,
    resumed_to_status: z.string(),
    disposition: RestartDispositionSchema.optional(),
  }),
  z.object({
    type: z.literal("ValidationFailed"),
    warnings: z.array(ResumeValidationWarningSchema),
    stopped_from_status: z.string(),
  }),
  z.object({
    type: z.literal("Blocked"),
    warnings: z.array(ResumeValidationWarningSchema),
  }),
]);

export type RestartResultRaw = z.infer<typeof RestartResultSchemaRaw>;

/**
 * Transform snake_case RestartResult to camelCase.
 */
export function transformRestartResult(raw: RestartResultRaw): RestartResult {
  if (raw.type === "Success") {
    return {
      type: "Success",
      task: raw.task,
      category: raw.category,
      resumedToStatus: raw.resumed_to_status,
      ...(raw.disposition ? { disposition: raw.disposition } : {}),
    };
  }
  if (raw.type === "Blocked") {
    return { type: "Blocked", warnings: raw.warnings };
  }
  return {
    type: "ValidationFailed",
    warnings: raw.warnings,
    stoppedFromStatus: raw.stopped_from_status,
  };
}
