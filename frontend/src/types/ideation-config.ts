// Ideation configuration types and Zod schemas
// Types for IdeationSettings and IdeationPlanMode

import { z } from "zod";

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
  plan_mode: z.string().optional(),
  require_plan_approval: z.boolean().optional(),
  suggest_plans_for_complex: z.boolean().optional(),
  auto_link_proposals: z.boolean().optional(),
  auto_verify_plans: z.boolean().default(false),
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
