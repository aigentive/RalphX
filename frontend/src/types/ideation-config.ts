// Ideation configuration types and Zod schemas
// Types for IdeationSettings and IdeationPlanMode

import { z } from "zod";

export const TasksFeatureStateSchema = z.enum(["enabled", "draining", "disabled"]);
export type TasksFeatureState = z.infer<typeof TasksFeatureStateSchema>;

export const TasksDisableImpactSchema = z.object({
  activeStandaloneTasks: z.number().int().nonnegative(),
  activeAttachedAgentWorkspaces: z.number().int().nonnegative(),
  pausedOrBlockedTasks: z.number().int().nonnegative(),
  activeBranchUpdateOperations: z.number().int().nonnegative(),
  affectedTaskIds: z.array(z.string()),
  affectedConversationIds: z.array(z.string()),
  affectedProjectIds: z.array(z.string()),
});
export type TasksDisableImpact = z.infer<typeof TasksDisableImpactSchema>;

// ============================================================================
// Ideation Settings
// ============================================================================

/**
 * Ideation settings schema matching Rust backend serialization
 */
export const ExternalIdeationOverridesSchema = z.object({
  autoVerifyPlans: z.boolean().nullable(),
  requireVerificationForAccept: z.boolean().nullable(),
  requireAcceptForFinalize: z.boolean().nullable(),
});

export type ExternalIdeationOverrides = z.infer<typeof ExternalIdeationOverridesSchema>;

export const IdeationSettingsSchema = z.object({
  tasksEnabled: z.boolean(),
  autoVerifyDraftPlans: z.boolean(),
  tasksFeatureState: TasksFeatureStateSchema,
  autoVerifyPlans: z.boolean(),
  requireAcceptForFinalize: z.boolean(),
  requireVerificationForAccept: z.boolean(),
  externalOverrides: ExternalIdeationOverridesSchema,
});

export type IdeationSettings = z.infer<typeof IdeationSettingsSchema>;

/**
 * Default ideation settings (matches Rust backend defaults)
 */
export const defaultIdeationSettings: IdeationSettings = {
  tasksEnabled: false,
  autoVerifyDraftPlans: true,
  tasksFeatureState: "disabled",
  autoVerifyPlans: false,
  requireAcceptForFinalize: false,
  requireVerificationForAccept: false,
  externalOverrides: {
    autoVerifyPlans: null,
    requireVerificationForAccept: null,
    requireAcceptForFinalize: null,
  },
};

// ============================================================================
// Response Schema (snake_case from Rust)
// ============================================================================

/**
 * Ideation settings response schema (snake_case from Rust)
 */
export const IdeationSettingsResponseSchema = z.object({
  tasks_enabled: z.boolean().default(false),
  tasks_feature_state: TasksFeatureStateSchema.default("disabled"),
  plan_mode: z.string().optional(),
  require_plan_approval: z.boolean().optional(),
  suggest_plans_for_complex: z.boolean().optional(),
  auto_link_proposals: z.boolean().optional(),
  auto_verify_plans: z.boolean().default(false),
  auto_verify_draft_plans: z.boolean().default(true),
  require_accept_for_finalize: z.boolean(),
  require_verification_for_accept: z.boolean().default(false),
  require_verification_for_proposals: z.boolean().default(false),
  external_overrides: z.object({
    auto_verify_plans: z.boolean().nullable().default(null),
    require_verification_for_accept: z.boolean().nullable().default(null),
    require_verification_for_proposals: z.boolean().nullable().default(null),
    require_accept_for_finalize: z.boolean().nullable().default(null),
  }).default({
    auto_verify_plans: null,
    require_verification_for_accept: null,
    require_verification_for_proposals: null,
    require_accept_for_finalize: null,
  }),
});

export type IdeationSettingsResponse = z.infer<typeof IdeationSettingsResponseSchema>;
