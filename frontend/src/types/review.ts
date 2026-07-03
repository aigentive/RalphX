// Review types and Zod schemas
// Must match the Rust backend Review, ReviewAction, ReviewNote structs

import { z } from "zod";

// ========================================
// Enums / Literal Types
// ========================================

/**
 * Who performed the review
 */
export const REVIEWER_TYPE_VALUES = ["ai", "human", "system"] as const;
export const ReviewerTypeSchema = z.enum(REVIEWER_TYPE_VALUES);
export type ReviewerType = z.infer<typeof ReviewerTypeSchema>;

/**
 * Status of a review
 */
export const REVIEW_STATUS_VALUES = [
  "pending",
  "approved",
  "changes_requested",
  "rejected",
] as const;
export const ReviewStatusSchema = z.enum(REVIEW_STATUS_VALUES);
export type ReviewStatus = z.infer<typeof ReviewStatusSchema>;

/**
 * Type of action taken during review
 */
export const REVIEW_ACTION_TYPE_VALUES = [
  "created_fix_task",
  "moved_to_backlog",
  "approved",
] as const;
export const ReviewActionTypeSchema = z.enum(REVIEW_ACTION_TYPE_VALUES);
export type ReviewActionType = z.infer<typeof ReviewActionTypeSchema>;

/**
 * Outcome of a review (for review notes history)
 */
export const REVIEW_OUTCOME_VALUES = [
  "approved",
  "approved_no_changes",
  "changes_requested",
  "rejected",
] as const;
export const ReviewOutcomeSchema = z.enum(REVIEW_OUTCOME_VALUES);
export type ReviewOutcome = z.infer<typeof ReviewOutcomeSchema>;

// ========================================
// Review Entity
// ========================================

/**
 * A code review for a task
 * Tracks whether work was verified by AI or human reviewers
 */
export const ReviewSchema = z.object({
  id: z.string().min(1),
  projectId: z.string().min(1),
  taskId: z.string().min(1),
  reviewerType: ReviewerTypeSchema,
  status: ReviewStatusSchema,
  notes: z.string().nullable(),
  createdAt: z.string().datetime(),
  completedAt: z.string().datetime().nullable(),
});

export type Review = z.infer<typeof ReviewSchema>;

// ========================================
// ReviewAction Entity
// ========================================

/**
 * An action taken during or after a review
 * Tracks what happened as a result: fix tasks created, moved to backlog, approvals
 */
export const ReviewActionSchema = z.object({
  id: z.string().min(1),
  reviewId: z.string().min(1),
  actionType: ReviewActionTypeSchema,
  targetTaskId: z.string().nullable(),
  createdAt: z.string().datetime(),
});

export type ReviewAction = z.infer<typeof ReviewActionSchema>;

// ========================================
// ReviewNote Entity
// ========================================

/**
 * A note from a reviewer (part of review history)
 * Stores feedback from each review attempt - a task can have multiple notes over time
 */
export const ReviewNoteSchema = z.object({
  id: z.string().min(1),
  taskId: z.string().min(1),
  reviewer: ReviewerTypeSchema,
  outcome: ReviewOutcomeSchema,
  notes: z.string().nullable(),
  createdAt: z.string().datetime(),
});

export type ReviewNote = z.infer<typeof ReviewNoteSchema>;

// ========================================
// List Schemas
// ========================================

/**
 * Schema for review list response
 */
export const ReviewListSchema = z.array(ReviewSchema);
export type ReviewList = z.infer<typeof ReviewListSchema>;

/**
 * Schema for review action list response
 */
export const ReviewActionListSchema = z.array(ReviewActionSchema);
export type ReviewActionList = z.infer<typeof ReviewActionListSchema>;

/**
 * Schema for review note list response
 */
export const ReviewNoteListSchema = z.array(ReviewNoteSchema);
export type ReviewNoteList = z.infer<typeof ReviewNoteListSchema>;

// ========================================
// Helper Functions
// ========================================

/**
 * Check if a review is pending
 */
export function isReviewPending(status: ReviewStatus): boolean {
  return status === "pending";
}

/**
 * Check if a review is complete (any non-pending status)
 */
export function isReviewComplete(status: ReviewStatus): boolean {
  return status !== "pending";
}

/**
 * Check if a review was approved
 */
export function isReviewApproved(status: ReviewStatus): boolean {
  return status === "approved";
}

/**
 * Check if a review outcome is positive
 */
export function isOutcomePositive(outcome: ReviewOutcome): boolean {
  return outcome === "approved" || outcome === "approved_no_changes";
}

/**
 * Check if a review outcome is negative
 */
export function isOutcomeNegative(outcome: ReviewOutcome): boolean {
  return outcome === "changes_requested" || outcome === "rejected";
}

// ========================================
// Review Settings / Configuration
// ========================================

/**
 * Default values for review settings
 * Matches Rust ReviewSettings::default()
 */
export const DEFAULT_REVIEW_SETTINGS = {
  aiReviewEnabled: true,
  aiReviewAutoFix: true,
  requireFixApproval: false,
  requireHumanReview: false,
  requireWorkspaceReview: true,
  runTaskValidations: true,
  maxFixAttempts: 3,
} as const;

/**
 * Global review settings stored in project settings
 * Controls how the review system behaves
 */
export const ReviewSettingsSchema = z.object({
  /** Master toggle for AI review system */
  aiReviewEnabled: z.boolean().default(DEFAULT_REVIEW_SETTINGS.aiReviewEnabled),
  /** Automatically create fix tasks when AI review fails (if false, goes to backlog) */
  aiReviewAutoFix: z.boolean().default(DEFAULT_REVIEW_SETTINGS.aiReviewAutoFix),
  /** Require human approval before executing AI-proposed fix tasks */
  requireFixApproval: z.boolean().default(DEFAULT_REVIEW_SETTINGS.requireFixApproval),
  /** Require human review even after AI approval */
  requireHumanReview: z.boolean().default(DEFAULT_REVIEW_SETTINGS.requireHumanReview),
  /** Require workspace Review before publishing agent workspace branches */
  requireWorkspaceReview: z
    .boolean()
    .default(DEFAULT_REVIEW_SETTINGS.requireWorkspaceReview),
  /** Allow execution agents to run backend-managed task validations */
  runTaskValidations: z
    .boolean()
    .default(DEFAULT_REVIEW_SETTINGS.runTaskValidations),
  /** Maximum fix attempts before giving up and moving to backlog */
  maxFixAttempts: z.number().int().nonnegative().default(DEFAULT_REVIEW_SETTINGS.maxFixAttempts),
});

export type ReviewSettings = z.infer<typeof ReviewSettingsSchema>;

// ========================================
// Review Settings Helper Functions
// ========================================

/**
 * Check if AI review should run
 */
export function shouldRunAiReview(settings: ReviewSettings): boolean {
  return settings.aiReviewEnabled;
}

/**
 * Check if fix tasks should be auto-created on review failure
 */
export function shouldAutoCreateFix(settings: ReviewSettings): boolean {
  return settings.aiReviewAutoFix;
}

/**
 * Check if human review is required after AI approval
 */
export function needsHumanReview(settings: ReviewSettings): boolean {
  return settings.requireHumanReview;
}

/**
 * Check if fix tasks need human approval before execution
 */
export function needsFixApproval(settings: ReviewSettings): boolean {
  return settings.requireFixApproval;
}

/**
 * Check if we've exceeded the max fix attempts
 */
export function exceededMaxAttempts(settings: ReviewSettings, attempts: number): boolean {
  return attempts >= settings.maxFixAttempts;
}
