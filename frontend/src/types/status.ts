// Internal status types and Zod schema
// Must match the internal statuses from the Rust backend

import { z } from "zod";

/**
 * All internal status values matching the Rust backend
 * Uses snake_case to match Rust serde serialization
 */
export const InternalStatusSchema = z.enum([
  "backlog",
  "ready",
  "blocked",
  "executing",
  "qa_refining",
  "qa_testing",
  "qa_passed",
  "qa_failed",
  "pending_review",
  "reviewing",
  "review_passed",
  "escalated",
  "revision_needed",
  "re_executing",
  "approved",
  "pending_merge",
  "merging",
  "waiting_on_pr",
  "updating_plan_branch",
  "updating_task_branch",
  "branch_update_blocked",
  "merge_incomplete",
  "merge_conflict",
  "merged",
  "failed",
  "cancelled",
  "paused",
  "stopped",
]);

export type InternalStatus = z.infer<typeof InternalStatusSchema>;

/**
 * All internal status values as a readonly array
 */
export const INTERNAL_STATUS_VALUES = InternalStatusSchema.options;

/**
 * Idle statuses where tasks are not being actively worked on
 */
export const IDLE_STATUSES: readonly InternalStatus[] = [
  "backlog",
  "ready",
  "blocked",
  "branch_update_blocked",
] as const;

/**
 * Active statuses where tasks are being worked on
 */
export const ACTIVE_STATUSES: readonly InternalStatus[] = [
  "executing",
  "re_executing",
  "qa_refining",
  "qa_testing",
  "pending_review",
  "reviewing",
  "review_passed",
  "escalated",
  "revision_needed",
  "pending_merge",
  "merging",
  "waiting_on_pr",
  "updating_plan_branch",
  "updating_task_branch",
] as const;

/**
 * Terminal statuses where tasks are complete
 * Note: 'stopped' is terminal (requires manual restart)
 * Note: 'paused' is NOT terminal (can resume to previous state)
 */
export const TERMINAL_STATUSES: readonly InternalStatus[] = [
  "approved",
  "merged",
  "failed",
  "cancelled",
  "stopped",
  "merge_incomplete",
] as const;

/**
 * Merge statuses where tasks are in the merge workflow
 */
export const MERGE_STATUSES: readonly InternalStatus[] = [
  "pending_merge",
  "merging",
  "waiting_on_pr",
  "merge_incomplete",
  "merge_conflict",
  "updating_plan_branch",
  "updating_task_branch",
  "branch_update_blocked",
  "merged",
] as const;

/**
 * Review statuses where tasks are in the review process
 */
export const REVIEW_STATUSES: readonly InternalStatus[] = [
  "pending_review",
  "reviewing",
  "review_passed",
  "escalated",
] as const;

/**
 * Check if a status is a terminal status
 */
export function isTerminalStatus(status: InternalStatus): boolean {
  return (TERMINAL_STATUSES as readonly string[]).includes(status);
}

/**
 * Check if a status is an active status
 */
export function isActiveStatus(status: InternalStatus): boolean {
  return (ACTIVE_STATUSES as readonly string[]).includes(status);
}

/**
 * Check if a status is an idle status
 */
export function isIdleStatus(status: InternalStatus): boolean {
  return (IDLE_STATUSES as readonly string[]).includes(status);
}

/**
 * Check if a status is a review status
 */
export function isReviewStatus(status: InternalStatus): boolean {
  return (REVIEW_STATUSES as readonly string[]).includes(status);
}

// ============================================================================
// Status Groups for UI Features
// ============================================================================

/**
 * Statuses where task is in execution phase (worker running)
 */
export const EXECUTION_STATUSES = [
  "executing",
  "re_executing",
  "qa_refining",
  "qa_testing",
  "qa_passed",
  "qa_failed",
] as const satisfies readonly InternalStatus[];

/**
 * Statuses where task is in AI review phase
 */
export const AI_REVIEW_STATUSES = [
  "pending_review",
  "reviewing",
] as const satisfies readonly InternalStatus[];

/**
 * Statuses where task awaits human review decision
 */
export const HUMAN_REVIEW_STATUSES = [
  "review_passed",
  "escalated",
] as const satisfies readonly InternalStatus[];

/**
 * All review-related statuses (AI + Human)
 */
export const ALL_REVIEW_STATUSES = [
  ...AI_REVIEW_STATUSES,
  ...HUMAN_REVIEW_STATUSES,
] as const;

/**
 * Statuses where drag-drop is disabled (system-managed states)
 */
export const NON_DRAGGABLE_STATUSES = [
  "executing",
  "re_executing",
  "qa_refining",
  "qa_testing",
  "qa_passed",
  "qa_failed",
  "pending_review",
  "reviewing",
  "review_passed",
  "escalated",
  "revision_needed",
  "pending_merge",
  "merging",
  "waiting_on_pr",
  "merge_incomplete",
  "merge_conflict",
  "updating_plan_branch",
  "updating_task_branch",
  "branch_update_blocked",
  "approved",
  "merged",
  "paused",
  "stopped",
] as const satisfies readonly InternalStatus[];

// ============================================================================
// Helper Functions for Status Groups
// ============================================================================

/**
 * Check if a status is an execution status
 */
export function isExecutionStatus(status: InternalStatus): boolean {
  return (EXECUTION_STATUSES as readonly string[]).includes(status);
}

/**
 * Check if a status is an AI review status
 */
export function isAiReviewStatus(status: InternalStatus): boolean {
  return (AI_REVIEW_STATUSES as readonly string[]).includes(status);
}

/**
 * Check if a status is a human review status
 */
export function isHumanReviewStatus(status: InternalStatus): boolean {
  return (HUMAN_REVIEW_STATUSES as readonly string[]).includes(status);
}

/**
 * Check if a status is a non-draggable status
 */
export function isNonDraggableStatus(status: InternalStatus): boolean {
  return (NON_DRAGGABLE_STATUSES as readonly string[]).includes(status);
}

/**
 * Check if a status is a merge status
 */
export function isMergeStatus(status: InternalStatus): boolean {
  return (MERGE_STATUSES as readonly string[]).includes(status);
}

// ============================================================================
// Status Counting Utilities
// ============================================================================

export interface StatusCounts {
  idle: number;
  active: number;
  done: number;
  total: number;
}

/**
 * Categorize a status into idle/active/done buckets.
 * Idle = IDLE_STATUSES, done = TERMINAL_STATUSES, everything else = active.
 */
export function categorizeStatus(status: InternalStatus): "idle" | "active" | "done" {
  if ((IDLE_STATUSES as readonly string[]).includes(status)) return "idle";
  if ((TERMINAL_STATUSES as readonly string[]).includes(status)) return "done";
  return "active";
}

/**
 * Count tasks by status category (idle/active/done).
 * Accepts any array of objects with an `internalStatus` field.
 */
export function getStatusCounts(tasks: { internalStatus: InternalStatus }[]): StatusCounts {
  const counts: StatusCounts = { idle: 0, active: 0, done: 0, total: tasks.length };
  for (const task of tasks) {
    const category = categorizeStatus(task.internalStatus);
    counts[category]++;
  }
  return counts;
}
