import { z } from "zod";

import { HarnessSchema } from "./ideation-harness";

export const McpOverrideStateSchema = z.enum(["follow", "enabled", "disabled"]);
export const McpPolicySourceSchema = z.enum([
  "required_internal",
  "project_ui",
  "project_yaml",
  "global_ui",
  "global_yaml",
  "provider_native",
]);
export const NativeMcpStateSchema = z.enum([
  "unknown",
  "enabled",
  "disabled",
  "pending_approval",
  "auth_required",
  "untrusted",
  "unavailable",
]);
export const McpConflictKindSchema = z.enum([
  "ambiguous_reserved_id",
  "legacy_registration",
  "legacy_repair_failed",
]);
export const McpRepairStatusSchema = z.enum([
  "repairable",
  "repaired",
  "failed",
  "manual_only",
]);

export const RawMcpToolSchema = z.object({
  tool_name: z.string(),
  configured_state: McpOverrideStateSchema,
  effective_state: McpOverrideStateSchema,
  effective_source: McpPolicySourceSchema,
});

export const RawMcpServerSchema = z.object({
  provider: HarnessSchema,
  server_id: z.string(),
  native_scope: z.string().nullable().optional(),
  native_state: NativeMcpStateSchema,
  effective_enabled: z.boolean(),
  configured_state: McpOverrideStateSchema,
  effective_state: McpOverrideStateSchema,
  effective_source: McpPolicySourceSchema,
  known_tools: z.array(RawMcpToolSchema),
  disabled_tools: z.array(z.string()),
  locked: z.boolean(),
  locked_reason: z.string().nullable().optional(),
  diagnostic: z.string().nullable().optional(),
  conflict_kind: McpConflictKindSchema.nullable().optional(),
  repair_status: McpRepairStatusSchema.nullable().optional(),
});

export const RawMcpCatalogSchema = z.object({
  eligible_providers: z.array(HarnessSchema),
  eligible_default_provider: HarnessSchema.nullable().optional(),
  probed_at: z.string(),
  probe_stale: z.boolean(),
  provider_diagnostics: z.record(z.string(), z.string()),
  policy_diagnostics: z.array(z.string()),
  servers: z.array(RawMcpServerSchema),
});

export const McpMutationResponseSchema = z.object({ changed: z.boolean() });

export type RawMcpCatalog = z.infer<typeof RawMcpCatalogSchema>;
