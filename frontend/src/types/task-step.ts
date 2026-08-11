// Task Step types and Zod schema
// Must match the Rust backend TaskStep struct

import { z } from "zod";

/**
 * Task step status enum
 * Matches Rust TaskStepStatus enum with serde(rename_all = "snake_case")
 */
export const TaskStepStatusSchema = z.enum([
  "pending",
  "in_progress",
  "completed",
  "skipped",
  "failed",
  "cancelled",
]);

export type TaskStepStatus = z.infer<typeof TaskStepStatusSchema>;

/**
 * Task step response schema matching Rust backend serialization (snake_case)
 * Backend outputs snake_case (Rust default). Transform layer converts to camelCase for UI.
 */
export const TaskStepResponseSchema = z.object({
  id: z.string().min(1),
  task_id: z.string().min(1),
  title: z.string().min(1),
  description: z.string().nullable(),
  status: TaskStepStatusSchema,
  sort_order: z.number().int(),
  depends_on: z.string().nullable(),
  created_by: z.string().min(1),
  completion_note: z.string().nullable(),
  // Accept RFC3339 timestamps with offset (e.g., +00:00)
  created_at: z.string().datetime({ offset: true }),
  updated_at: z.string().datetime({ offset: true }),
  started_at: z.string().datetime({ offset: true }).nullable(),
  completed_at: z.string().datetime({ offset: true }).nullable(),
});

/**
 * Frontend TaskStep type (camelCase)
 * This is what components and stores use. Transformed from snake_case API responses.
 */
export interface TaskStep {
  id: string;
  taskId: string;
  title: string;
  description: string | null;
  status: TaskStepStatus;
  sortOrder: number;
  dependsOn: string | null;
  createdBy: string;
  completionNote: string | null;
  createdAt: string;
  updatedAt: string;
  startedAt: string | null;
  completedAt: string | null;
}

/**
 * Transform function to convert snake_case API response to camelCase frontend type
 */
export function transformTaskStep(raw: z.infer<typeof TaskStepResponseSchema>): TaskStep {
  return {
    id: raw.id,
    taskId: raw.task_id,
    title: raw.title,
    description: raw.description,
    status: raw.status,
    sortOrder: raw.sort_order,
    dependsOn: raw.depends_on,
    createdBy: raw.created_by,
    completionNote: raw.completion_note,
    createdAt: raw.created_at,
    updatedAt: raw.updated_at,
    startedAt: raw.started_at,
    completedAt: raw.completed_at,
  };
}

// Legacy export for backward compatibility
export const TaskStepSchema = TaskStepResponseSchema;

/**
 * Step progress summary response schema (snake_case from Rust)
 * Backend outputs snake_case (Rust default). Transform layer converts to camelCase for UI.
 */
export const StepProgressSummaryResponseSchema = z.object({
  task_id: z.string().min(1),
  total: z.number().int().min(0),
  completed: z.number().int().min(0),
  in_progress: z.number().int().min(0),
  pending: z.number().int().min(0),
  skipped: z.number().int().min(0),
  failed: z.number().int().min(0),
  current_step: TaskStepResponseSchema.nullable(),
  next_step: TaskStepResponseSchema.nullable(),
  percent_complete: z.number().min(0).max(100),
});

/**
 * Frontend StepProgressSummary type (camelCase)
 */
export interface StepProgressSummary {
  taskId: string;
  total: number;
  completed: number;
  inProgress: number;
  pending: number;
  skipped: number;
  failed: number;
  currentStep: TaskStep | null;
  nextStep: TaskStep | null;
  percentComplete: number;
}

export interface StepProgressDisplay {
  completed: number;
  total: number;
  completedPercent: number;
  activePercent: number;
}

/**
 * Transform function for StepProgressSummary
 */
export function transformStepProgressSummary(
  raw: z.infer<typeof StepProgressSummaryResponseSchema>
): StepProgressSummary {
  return {
    taskId: raw.task_id,
    total: raw.total,
    completed: raw.completed,
    inProgress: raw.in_progress,
    pending: raw.pending,
    skipped: raw.skipped,
    failed: raw.failed,
    currentStep: raw.current_step ? transformTaskStep(raw.current_step) : null,
    nextStep: raw.next_step ? transformTaskStep(raw.next_step) : null,
    percentComplete: raw.percent_complete,
  };
}

export function getCompletableStepProgressCounts(
  progress: Pick<StepProgressSummary, "completed" | "skipped" | "total">
): { completed: number; total: number } {
  const total = Math.max(0, progress.total - progress.skipped);
  return {
    completed: Math.min(Math.max(0, progress.completed), total),
    total,
  };
}

export function getStepProgressDisplay(
  progress: Pick<
    StepProgressSummary,
    "completed" | "inProgress" | "skipped" | "total"
  >
): StepProgressDisplay {
  const { completed, total } = getCompletableStepProgressCounts(progress);
  const activeStepCount = Math.min(
    completed + Math.max(0, progress.inProgress),
    total
  );

  if (total === 0) {
    return {
      completed,
      total,
      completedPercent: 0,
      activePercent: 0,
    };
  }

  return {
    completed,
    total,
    completedPercent: (completed / total) * 100,
    activePercent: (activeStepCount / total) * 100,
  };
}

// Legacy export for backward compatibility
export const StepProgressSummarySchema = StepProgressSummaryResponseSchema;

/**
 * Status helpers
 */
export const isTaskStepPending = (status: TaskStepStatus) => status === "pending";
export const isTaskStepInProgress = (status: TaskStepStatus) => status === "in_progress";
export const isTaskStepCompleted = (status: TaskStepStatus) => status === "completed";
export const isTaskStepSkipped = (status: TaskStepStatus) => status === "skipped";
export const isTaskStepFailed = (status: TaskStepStatus) => status === "failed";
export const isTaskStepCancelled = (status: TaskStepStatus) => status === "cancelled";
export const isTaskStepTerminal = (status: TaskStepStatus) =>
  status === "completed" || status === "skipped" || status === "failed" || status === "cancelled";
export const isTaskStepActive = (status: TaskStepStatus) => status === "in_progress";

/**
 * All possible step status values
 */
export const TASK_STEP_STATUS_VALUES = TaskStepStatusSchema.options;
