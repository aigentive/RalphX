/**
 * Detail Views - State-specific task detail view components
 *
 * These components implement the View Registry Pattern where each internal
 * status maps to a specialized detail view component.
 *
 * Usage:
 * ```tsx
 * import { BasicTaskDetail } from "@/components/agents/task-details/detail-views";
 * ```
 */

export { BasicTaskDetail } from "./BasicTaskDetail";
export { RevisionTaskDetail } from "./RevisionTaskDetail";
export { ExecutionTaskDetail } from "./ExecutionTaskDetail";
export { ReviewingTaskDetail } from "./ReviewingTaskDetail";
export { HumanReviewTaskDetail } from "./HumanReviewTaskDetail";
export { EscalatedTaskDetail } from "./EscalatedTaskDetail";
export { WaitingTaskDetail } from "./WaitingTaskDetail";
export { CompletedTaskDetail } from "./CompletedTaskDetail";
export { MergingTaskDetail } from "./MergingTaskDetail";
export { MergeConflictTaskDetail } from "./MergeConflictTaskDetail";
export { MergeIncompleteTaskDetail } from "./MergeIncompleteTaskDetail";
export { MergedTaskDetail } from "./MergedTaskDetail";
