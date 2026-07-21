// Tauri invoke wrappers for tasks and steps with type safety using Zod schemas

import { invoke } from "@tauri-apps/api/core";
import { z } from "zod";
import { TauriVoidSchema } from "@/lib/tauri";
import {
  TaskSchema,
  TaskListSchema,
  TaskListResponseSchema,
  StatusTransitionSchema,
  RestartResultSchemaRaw,
  transformTask,
  transformTaskListResponse,
  transformRestartResult,
  type CreateTask,
  type UpdateTask,
  type Task,
  type TaskListResponse,
  type RestartResult,
} from "@/types/task";
import {
  TaskStepResponseSchema,
  StepProgressSummaryResponseSchema,
  transformTaskStep,
  transformStepProgressSummary,
  type TaskStep,
  type StepProgressSummary,
} from "@/types/task-step";
import {
  BulkArchiveResponseSchemaRaw,
  BulkCancelResponseSchemaRaw,
  BulkPauseResponseSchemaRaw,
  BulkResumeResponseSchemaRaw,
  CleanupReportResponseSchemaRaw,
  ExecutionPlanControlResponseSchemaRaw,
  InjectTaskResponseSchemaRaw,
  StateTransitionResponseSchemaRaw,
  TaskHistoryAvailabilityResponseSchemaRaw,
  UnblockTaskResponseSchemaRaw,
} from "./tasks.schemas";
import {
  transformBulkCancelResponse,
  transformBulkPauseResponse,
  transformBulkResumeResponse,
  transformExecutionPlanControlResponse,
  transformBulkArchiveResponse,
  transformCleanupReport,
  transformInjectTaskResponse,
  transformStateTransition,
  transformTaskHistoryAvailability,
  transformUnblockTaskResponse,
  type BulkCancelResponse,
  type BulkPauseResponse,
  type BulkResumeResponse,
  type ExecutionPlanControlResponse,
  type BulkArchiveResponse,
  type CleanupReport,
  type InjectTaskResponse,
  type StateTransition,
  type TaskHistoryAvailability,
  type UnblockTaskResponse,
} from "./tasks.transforms";
import {
  ExecutionTaskAgentWorkspaceSchema,
  transformExecutionTaskAgentWorkspace,
  type ExecutionTaskAgentWorkspace,
} from "./execution-task-agent-workspace";

// Re-export types for convenience
export type {
  BulkArchiveResponse,
  BulkCancelResponse,
  BulkPauseResponse,
  BulkResumeResponse,
  CleanupReport,
  ExecutionPlanControlResponse,
  InjectTaskResponse,
  StateTransition,
  UnblockTaskResponse,
  TaskHistoryAvailability,
} from "./tasks.transforms";

// Re-export schemas for consumers that need validation
export {
  BulkArchiveResponseSchemaRaw,
  BulkCancelResponseSchemaRaw,
  BulkPauseResponseSchemaRaw,
  BulkResumeResponseSchemaRaw,
  CleanupReportResponseSchemaRaw,
  ExecutionPlanControlResponseSchemaRaw,
  InjectTaskResponseSchemaRaw,
  StateTransitionResponseSchemaRaw,
  TaskHistoryAvailabilityResponseSchemaRaw,
  UnblockTaskResponseSchemaRaw,
} from "./tasks.schemas";

// Re-export transforms for consumers that need manual transformation
export {
  transformBulkArchiveResponse,
  transformBulkCancelResponse,
  transformBulkPauseResponse,
  transformBulkResumeResponse,
  transformCleanupReport,
  transformExecutionPlanControlResponse,
  transformInjectTaskResponse,
  transformStateTransition,
  transformTaskHistoryAvailability,
  transformUnblockTaskResponse,
} from "./tasks.transforms";

// ============================================================================
// Input Types
// ============================================================================

/**
 * Input type for injecting a task mid-loop
 */
export interface InjectTaskInput {
  /** The project ID to inject the task into */
  projectId: string;
  /** Title of the task */
  title: string;
  /** Optional description */
  description?: string;
  /** Category (defaults to "feature") */
  category?: string;
  /** Where to inject: "backlog" (deferred) or "planned" (immediate queue) */
  target?: "backlog" | "planned";
  /** If true and target is "planned", make this task the highest priority */
  makeNext?: boolean;
}

// ============================================================================
// Typed Invoke Helpers
// ============================================================================

async function typedInvoke<T>(
  cmd: string,
  args: Record<string, unknown>,
  schema: z.ZodType<T>
): Promise<T> {
  const result = await invoke(cmd, args);
  return schema.parse(result);
}

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
// List Schemas
// ============================================================================

const TaskStepListResponseSchema = z.array(TaskStepResponseSchema);

// ============================================================================
// Tasks API Object
// ============================================================================

/**
 * Tasks API wrappers for Tauri commands
 */
export const tasksApi = {
  /**
   * List all tasks for a project with optional pagination and filtering
   * @param params Parameters for listing tasks
   * @param params.projectId The project ID
   * @param params.statuses Optional status filter (array of statuses, matches any)
   * @param params.offset Optional pagination offset (default 0)
   * @param params.limit Optional pagination limit (default 20)
   * @param params.includeArchived Optional flag to include archived tasks (default false)
   * @param params.ideationSessionId Optional ideation session ID to filter tasks by plan
   * @param params.executionPlanId Optional execution plan ID to filter tasks (mutually exclusive with ideationSessionId)
   * @returns Paginated task list response
   */
  list: (params: {
    projectId: string;
    statuses?: string[];
    categories?: string[];
    offset?: number;
    limit?: number;
    includeArchived?: boolean;
    ideationSessionId?: string;
    executionPlanId?: string;
  }): Promise<TaskListResponse> =>
    typedInvokeWithTransform("list_tasks", params, TaskListResponseSchema, transformTaskListResponse),

  getSessionHistoryAvailability: (
    projectId: string,
    ideationSessionId: string
  ): Promise<TaskHistoryAvailability> =>
    typedInvokeWithTransform(
      "get_session_task_history_availability",
      { projectId, ideationSessionId },
      TaskHistoryAvailabilityResponseSchemaRaw,
      transformTaskHistoryAvailability
    ),

  /**
   * Search tasks by query string
   * @param projectId The project ID
   * @param query The search query (searches title and description)
   * @param includeArchived Optional flag to include archived tasks (default false)
   * @param ideationSessionId Optional ideation session ID to filter tasks by plan
   * @returns Array of matching tasks
   */
  search: (projectId: string, query: string, includeArchived?: boolean, ideationSessionId?: string | null): Promise<Task[]> =>
    typedInvokeWithTransform(
      "search_tasks",
      { projectId, query, includeArchived, ideationSessionId },
      TaskListSchema,
      (tasks) => tasks.map(transformTask)
    ),

  /**
   * Get a single task by ID
   * @param taskId The task ID
   * @returns The task
   */
  get: (taskId: string): Promise<Task> =>
    typedInvokeWithTransform("get_task", { id: taskId }, TaskSchema, transformTask),

  /**
   * Resolve the Agent conversation workspace that owns a task, when available.
   */
  resolveAgentWorkspace: (taskId: string): Promise<ExecutionTaskAgentWorkspace | null> =>
    typedInvokeWithTransform(
      "get_task_agent_workspace",
      { taskId },
      ExecutionTaskAgentWorkspaceSchema.nullable(),
      (workspace) =>
        workspace ? transformExecutionTaskAgentWorkspace(workspace) : null,
    ),

  /**
   * Create a new task
   * @param input Task creation data
   * @returns The created task
   */
  create: (input: CreateTask): Promise<Task> =>
    typedInvokeWithTransform("create_task", { input }, TaskSchema, transformTask),

  /**
   * Update an existing task
   * @param taskId The task ID
   * @param input Partial task data to update
   * @returns The updated task
   */
  update: (taskId: string, input: UpdateTask): Promise<Task> =>
    typedInvokeWithTransform("update_task", { taskId, input }, TaskSchema, transformTask),

  /**
   * Delete a task (bare delete, no cleanup)
   * @deprecated Use `cleanupTask` instead — it handles force-stop, branch cleanup, and event emission.
   * @param taskId The task ID
   * @returns true if deleted
   */
  delete: (taskId: string) =>
    typedInvoke("delete_task", { id: taskId }, z.boolean()),

  /**
   * Archive a task (soft delete)
   * @param taskId The task ID
   * @returns The archived task
   */
  archive: (taskId: string): Promise<Task> =>
    typedInvokeWithTransform("archive_task", { taskId }, TaskSchema, transformTask),

  /**
   * Restore an archived task
   * @param taskId The task ID
   * @returns The restored task
   */
  restore: (taskId: string): Promise<Task> =>
    typedInvokeWithTransform("restore_task", { taskId }, TaskSchema, transformTask),

  /**
   * Pause a running task
   * Transitions the task to Paused state, which can be resumed later
   * @param taskId The task ID
   * @returns The paused task
   */
  pause: (taskId: string): Promise<Task> =>
    typedInvokeWithTransform("pause_task", { taskId }, TaskSchema, transformTask),

  /**
   * Resume a paused task
   * Reads previous_status from pause metadata and restores the task
   * @param taskId The task ID
   * @returns The resumed task
   */
  resume: (taskId: string): Promise<Task> =>
    typedInvokeWithTransform("resume_task", { taskId }, TaskSchema, transformTask),

  /**
   * Stop a running task
   * Transitions the task to Stopped state (terminal, requires manual restart)
   * @param taskId The task ID
   * @param reason Optional reason for stopping (captured in metadata for smart resume)
   * @returns The stopped task
   */
  stop: (taskId: string, reason?: string): Promise<Task> =>
    typedInvokeWithTransform("stop_task", { taskId, reason }, TaskSchema, transformTask),

  /**
   * Smart resume for stopped tasks.
   *
   * Restarts a task that was stopped mid-execution, using the captured stop metadata
   * to determine the appropriate resume behavior:
   *
   * - **Direct**: Resume directly to the original state (Executing, ReExecuting, Reviewing, etc.)
   * - **Validated**: Validate git state before resuming (Merging, PendingMerge, etc.)
   * - **Redirect**: Resume to successor state (QaPassed→PendingReview, RevisionNeeded→ReExecuting)
   *
   * @param taskId The task ID
   * @param force If true, skip validation (use with caution)
   * @param note Optional note to communicate intent to the re-executing agent
   * @returns RestartResult with Success, ValidationFailed, or Blocked variants
   */
  restart: (
    taskId: string,
    force?: boolean,
    note?: string
  ): Promise<RestartResult> =>
    typedInvokeWithTransform(
      "restart_task",
      {
        taskId,
        force: force ?? false,
        note: note ?? null,
      },
      RestartResultSchemaRaw,
      transformRestartResult
    ),

  /**
   * Get count of archived tasks for a project
   * @param projectId The project ID
   * @param ideationSessionId Optional ideation session ID to filter tasks by plan
   * @returns Count of archived tasks
   */
  getArchivedCount: (projectId: string, ideationSessionId?: string | null) =>
    typedInvoke("get_archived_count", { projectId, ideationSessionId }, z.number()),

  /**
   * Get valid status transitions for a task
   * @param taskId The task ID
   * @returns Array of valid status transitions
   */
  getValidTransitions: (taskId: string) =>
    typedInvoke(
      "get_valid_transitions",
      { taskId },
      z.array(StatusTransitionSchema)
    ),

  /**
   * Move a task to a new status
   * @param taskId The task ID
   * @param toStatus The target status
   * @param note Optional note to communicate intent to the re-executing agent
   * @returns The updated task
   */
  move: (taskId: string, toStatus: string, note?: string): Promise<Task> =>
    typedInvokeWithTransform(
      "move_task",
      { taskId, toStatus, note: note ?? null },
      TaskSchema,
      transformTask
    ),

  /**
   * Retry a merge using the merge-specific backend path.
   *
   * Do not replace this with a generic move to pending_merge; the backend command
   * preserves hook-failure classification and can reroute corrective failures.
   */
  retryMerge: (taskId: string, skipValidation?: boolean): Promise<null> =>
    typedInvoke(
      "retry_merge",
      { taskId, skipValidation: skipValidation ?? null },
      TauriVoidSchema
    ),

  retryBranchUpdate: (taskId: string): Promise<Task> =>
    typedInvokeWithTransform(
      "retry_branch_update",
      { taskId },
      TaskSchema,
      transformTask
    ),

  /**
   * Inject a task mid-loop
   * Tasks can be sent to backlog (deferred) or planned (immediate queue).
   * If makeNext is true and target is "planned", the task gets the highest priority.
   * Emits a task:created event on success.
   * @param input Inject task input
   * @returns The inject task response with created task and injection details
   */
  inject: (input: InjectTaskInput): Promise<InjectTaskResponse> =>
    typedInvokeWithTransform(
      "inject_task",
      { input },
      InjectTaskResponseSchemaRaw,
      transformInjectTaskResponse
    ),

  /**
   * Get tasks awaiting review for a project
   * Returns tasks in review-related statuses (pending_review, reviewing, review_passed, escalated)
   * @param projectId The project ID
   * @returns Array of tasks awaiting review
   */
  getTasksAwaitingReview: (projectId: string): Promise<Task[]> =>
    typedInvokeWithTransform(
      "get_tasks_awaiting_review",
      { projectId },
      TaskListSchema,
      (tasks) => tasks.map(transformTask)
    ),

  /**
   * Block a task with an optional reason
   * Transitions the task to 'blocked' status and sets the blocked_reason field.
   * @param taskId The task ID
   * @param reason Optional reason for blocking
   * @returns The updated task
   */
  block: (taskId: string, reason?: string): Promise<Task> =>
    typedInvokeWithTransform("block_task", { taskId, reason }, TaskSchema, transformTask),

  /**
   * Unblock a task
   * Transitions the task back to 'ready' status and clears the blocked_reason field.
   * Returns an optional warning when the task has failed dependencies — the unblock
   * still succeeds (manual override), but callers should surface the warning to the user.
   * @param taskId The task ID
   * @returns { task, warning } — task is the updated task; warning is non-null when failed deps exist
   */
  unblock: (taskId: string): Promise<UnblockTaskResponse> =>
    typedInvokeWithTransform("unblock_task", { taskId }, UnblockTaskResponseSchemaRaw, transformUnblockTaskResponse),

  /**
   * Get historical state transitions for a task
   * Returns chronological list of all state changes the task has gone through.
   * Used by StateTimelineNav for displaying task history and enabling time travel.
   * @param taskId The task ID
   * @returns Array of state transitions in chronological order
   */
  getStateTransitions: (taskId: string): Promise<StateTransition[]> =>
    typedInvokeWithTransform(
      "get_task_state_transitions",
      { taskId },
      z.array(StateTransitionResponseSchemaRaw),
      (transitions) => transitions.map(transformStateTransition)
    ),

  /**
   * Clean delete a single task (force-stop agent if active, cleanup git branch, delete from DB)
   * @param taskId The task ID to clean delete
   */
  cleanupTask: (taskId: string) =>
    typedInvoke("cleanup_task", { taskId }, TauriVoidSchema),

  /**
   * Clean delete all tasks in a group
   * @param groupKind "status" | "session" | "uncategorized"
   * @param groupId The status name or session ID
   * @param projectId The project ID
   * @returns Cleanup report with counts
   */
  cleanupTasksInGroup: (
    groupKind: string,
    groupId: string,
    projectId: string
  ): Promise<CleanupReport> =>
    typedInvokeWithTransform(
      "cleanup_tasks_in_group",
      { groupKind, groupId, projectId },
      CleanupReportResponseSchemaRaw,
      transformCleanupReport
    ),

  /**
   * Cancel all tasks in a group (non-destructive alternative to cleanup)
   * @param groupKind "status" | "session" | "uncategorized"
   * @param groupId The status name or session ID
   * @param projectId The project ID
   * @returns Bulk cancel report with count of cancelled tasks
   */
  cancelTasksInGroup: (
    groupKind: string,
    groupId: string,
    projectId: string
  ): Promise<BulkCancelResponse> =>
    typedInvokeWithTransform(
      "cancel_tasks_in_group",
      { groupKind, groupId, projectId },
      BulkCancelResponseSchemaRaw,
      transformBulkCancelResponse
    ),

  /**
   * Pause all tasks in a group
   * @param groupKind "status" | "session" | "uncategorized"
   * @param groupId The status name or session ID
   * @param projectId The project ID
   * @returns Bulk pause report with count of paused tasks
   */
  pauseTasksInGroup: (
    groupKind: string,
    groupId: string,
    projectId: string
  ): Promise<BulkPauseResponse> =>
    typedInvokeWithTransform(
      "pause_tasks_in_group",
      { groupKind, groupId, projectId },
      BulkPauseResponseSchemaRaw,
      transformBulkPauseResponse
    ),

  /**
   * Resume all paused tasks in a group
   * @param groupKind "status" | "session" | "uncategorized"
   * @param groupId The status name or session ID
   * @param projectId The project ID
   * @returns Bulk resume report with count of resumed tasks
   */
  resumeTasksInGroup: (
    groupKind: string,
    groupId: string,
    projectId: string
  ): Promise<BulkResumeResponse> =>
    typedInvokeWithTransform(
      "resume_tasks_in_group",
      { groupKind, groupId, projectId },
      BulkResumeResponseSchemaRaw,
      transformBulkResumeResponse
    ),

  /**
   * Pause the current implementation work for one execution plan.
   */
  pauseExecutionPlan: (input: {
    projectId: string;
    sessionId: string;
    executionPlanId?: string | null;
  }): Promise<ExecutionPlanControlResponse> =>
    typedInvokeWithTransform(
      "pause_execution_plan",
      {
        input: {
          projectId: input.projectId,
          sessionId: input.sessionId,
          executionPlanId: input.executionPlanId ?? null,
        },
      },
      ExecutionPlanControlResponseSchemaRaw,
      transformExecutionPlanControlResponse
    ),

  /**
   * Resume paused work for one execution plan.
   */
  resumeExecutionPlan: (input: {
    projectId: string;
    sessionId: string;
    executionPlanId?: string | null;
  }): Promise<ExecutionPlanControlResponse> =>
    typedInvokeWithTransform(
      "resume_execution_plan",
      {
        input: {
          projectId: input.projectId,
          sessionId: input.sessionId,
          executionPlanId: input.executionPlanId ?? null,
        },
      },
      ExecutionPlanControlResponseSchemaRaw,
      transformExecutionPlanControlResponse
    ),

  /**
   * Stop running work for one execution plan.
   */
  stopExecutionPlan: (input: {
    projectId: string;
    sessionId: string;
    executionPlanId?: string | null;
  }): Promise<ExecutionPlanControlResponse> =>
    typedInvokeWithTransform(
      "stop_execution_plan",
      {
        input: {
          projectId: input.projectId,
          sessionId: input.sessionId,
          executionPlanId: input.executionPlanId ?? null,
        },
      },
      ExecutionPlanControlResponseSchemaRaw,
      transformExecutionPlanControlResponse
    ),

  /**
   * Archive all tasks in a group
   * @param groupKind "status" | "session" | "uncategorized"
   * @param groupId The status name or session ID
   * @param projectId The project ID
   * @returns Bulk archive report with count of archived tasks
   */
  archiveTasksInGroup: (
    groupKind: string,
    groupId: string,
    projectId: string
  ): Promise<BulkArchiveResponse> =>
    typedInvokeWithTransform(
      "archive_tasks_in_group",
      { groupKind, groupId, projectId },
      BulkArchiveResponseSchemaRaw,
      transformBulkArchiveResponse
    ),
} as const;

// ============================================================================
// Steps API Object
// ============================================================================

/**
 * Task steps API wrappers for Tauri commands
 */
export const stepsApi = {
  /**
   * Get all steps for a task
   * @param taskId The task ID
   * @returns Array of task steps
   */
  getByTask: (taskId: string): Promise<TaskStep[]> =>
    typedInvokeWithTransform(
      "get_task_steps",
      { taskId },
      TaskStepListResponseSchema,
      (steps) => steps.map(transformTaskStep)
    ),

  /**
   * Create a new task step
   * @param taskId The task ID
   * @param data Step creation data (title, description, sortOrder)
   * @returns The created task step
   */
  create: (
    taskId: string,
    data: { title: string; description?: string; sortOrder?: number }
  ): Promise<TaskStep> =>
    typedInvokeWithTransform(
      "create_task_step",
      { taskId, input: data },
      TaskStepResponseSchema,
      transformTaskStep
    ),

  /**
   * Update an existing task step
   * @param stepId The step ID
   * @param data Partial step data to update (title, description, sortOrder)
   * @returns The updated task step
   */
  update: (
    stepId: string,
    data: { title?: string; description?: string; sortOrder?: number }
  ): Promise<TaskStep> =>
    typedInvokeWithTransform(
      "update_task_step",
      { stepId, ...data },
      TaskStepResponseSchema,
      transformTaskStep
    ),

  /**
   * Reorder task steps
   * @param taskId The task ID
   * @param stepIds Array of step IDs in desired order
   * @returns Array of reordered task steps
   */
  reorder: (taskId: string, stepIds: string[]): Promise<TaskStep[]> =>
    typedInvokeWithTransform(
      "reorder_task_steps",
      { taskId, stepIds },
      TaskStepListResponseSchema,
      (steps) => steps.map(transformTaskStep)
    ),

  /**
   * Get step progress summary for a task
   * @param taskId The task ID
   * @returns Step progress summary with counts and percentages
   */
  getProgress: (taskId: string): Promise<StepProgressSummary> =>
    typedInvokeWithTransform(
      "get_step_progress",
      { taskId },
      StepProgressSummaryResponseSchema,
      transformStepProgressSummary
    ),

  /**
   * Start a task step (marks as in_progress)
   * @param stepId The step ID
   * @returns The updated task step
   */
  start: (stepId: string): Promise<TaskStep> =>
    typedInvokeWithTransform(
      "start_step",
      { stepId },
      TaskStepResponseSchema,
      transformTaskStep
    ),

  /**
   * Complete a task step (marks as completed)
   * @param stepId The step ID
   * @param note Optional completion note
   * @returns The updated task step
   */
  complete: (stepId: string, note?: string): Promise<TaskStep> =>
    typedInvokeWithTransform(
      "complete_step",
      { stepId, note },
      TaskStepResponseSchema,
      transformTaskStep
    ),

  /**
   * Skip a task step (marks as skipped)
   * @param stepId The step ID
   * @param reason Reason for skipping
   * @returns The updated task step
   */
  skip: (stepId: string, reason: string): Promise<TaskStep> =>
    typedInvokeWithTransform(
      "skip_step",
      { stepId, reason },
      TaskStepResponseSchema,
      transformTaskStep
    ),

  /**
   * Fail a task step (marks as failed)
   * @param stepId The step ID
   * @param error Error message
   * @returns The updated task step
   */
  fail: (stepId: string, error: string): Promise<TaskStep> =>
    typedInvokeWithTransform(
      "fail_step",
      { stepId, error },
      TaskStepResponseSchema,
      transformTaskStep
    ),
} as const;
