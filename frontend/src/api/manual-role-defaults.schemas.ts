import { z } from "zod";

export const ManualServiceTierSchema = z.enum([
  "provider_default",
  "standard",
  "fast",
]);

export const ManualRoleDefaultResponseSchema = z.object({
  provider: z.string().min(1),
  model: z.string().nullable().optional(),
  effort: z.string().nullable().optional(),
  service_tier: ManualServiceTierSchema,
  coordination_mode: z.string().nullable().optional(),
  persona_id: z.string().nullable().optional(),
  approval_policy: z.string().nullable().optional(),
  sandbox_mode: z.string().nullable().optional(),
});

export const ManualRoleControlOptionResponseSchema = z.object({
  value: z.string().min(1),
  enabled: z.boolean(),
  disabled_reason: z.string().nullable().optional(),
});

export const ManualRoleControlsResponseSchema = z.object({
  capabilities: z.array(ManualRoleControlOptionResponseSchema),
  speeds: z.array(ManualRoleControlOptionResponseSchema),
  persona: z.object({
    enabled: z.boolean(),
    disabled_reason: z.string().nullable().optional(),
  }),
});

export const ManualRoleCatalogEntryResponseSchema = z.object({
  role: z.string().min(1),
  display_name: z.string().min(1),
  description: z.string().trim().min(1),
  family: z.string().min(1),
  family_display_name: z.string().min(1),
  requires_tasks: z.boolean().default(false),
  configured: ManualRoleDefaultResponseSchema.nullable().optional(),
  effective: ManualRoleDefaultResponseSchema.nullable().optional(),
  source: z.string().nullable().optional(),
  diagnostics: z.array(z.string()),
  controls: ManualRoleControlsResponseSchema,
});

export const ManualRoleCatalogResponseSchema = z.object({
  project_id: z.string().nullable().optional(),
  roles: z.array(ManualRoleCatalogEntryResponseSchema),
});

export const ComposerRoleDefaultResponseSchema = z.object({
  role: z.string().min(1),
  source: z.string().min(1),
  value: ManualRoleDefaultResponseSchema,
});
