import { typedInvoke } from "@/lib/tauri";
import { z } from "zod";

import { HarnessSchema } from "./ideation-harness";

export const ProviderCliManagementModeSchema = z.enum([
  "user_managed",
  "rx_managed",
]);

export const AgentProviderSettingsResponseSchema = z.object({
  provider: HarnessSchema,
  enabled: z.boolean(),
  isDefault: z.boolean(),
  model: z.string().nullable().optional(),
  effort: z.string().nullable().optional(),
  approvalPolicy: z.string().nullable().optional(),
  sandboxMode: z.string().nullable().optional(),
  claudePermissionMode: z.string().nullable().optional(),
  claudeDangerouslySkipPermissions: z.boolean(),
  claudeAllowDangerouslySkipPermissions: z.boolean(),
  cliManagementMode: ProviderCliManagementModeSchema.optional(),
  autoUpdateEnabled: z.boolean().optional(),
  customBinaryEnabled: z.boolean().optional(),
  customBinaryPath: z.string().nullable().optional(),
  available: z.boolean(),
  binaryFound: z.boolean(),
  binaryPath: z.string().nullable().optional(),
  status: z.string(),
  error: z.string().nullable().optional(),
  missingCoreExecFeatures: z.array(z.string()),
  cliVersion: z.string().nullable().optional(),
  supportedModelAliases: z.array(z.string().min(1)).nullable().optional(),
  supportedEfforts: z.array(z.string().min(1)).nullable().optional(),
  updatedAt: z.string(),
});

export type AgentProviderSettingsResponse = z.infer<
  typeof AgentProviderSettingsResponseSchema
>;

export const AgentProvidersSettingsResponseSchema = z.object({
  providers: z.array(AgentProviderSettingsResponseSchema),
  defaultProvider: HarnessSchema.nullable().optional(),
  requiresOnboarding: z.boolean(),
});

export type AgentProvidersSettingsResponse = z.infer<
  typeof AgentProvidersSettingsResponseSchema
>;

export interface UpdateAgentProviderSettingsInput {
  provider: string;
  enabled?: boolean;
  isDefault?: boolean;
  model?: string | null;
  effort?: string | null;
  approvalPolicy?: string | null;
  sandboxMode?: string | null;
  claudePermissionMode?: string | null;
  claudeDangerouslySkipPermissions?: boolean;
  claudeAllowDangerouslySkipPermissions?: boolean;
  cliManagementMode?: z.infer<typeof ProviderCliManagementModeSchema>;
  autoUpdateEnabled?: boolean;
  customBinaryEnabled?: boolean;
  customBinaryPath?: string | null;
  resetToDefaults?: boolean;
  applyToAllLanes?: boolean;
}

export interface ListAgentProviderSettingsOptions {
  refreshRuntime?: boolean;
}

export const harnessProvidersApi = {
  list(
    options: ListAgentProviderSettingsOptions = {},
  ): Promise<AgentProvidersSettingsResponse> {
    return typedInvoke(
      "get_agent_provider_settings",
      { input: { refreshRuntime: options.refreshRuntime ?? false } },
      AgentProvidersSettingsResponseSchema,
    );
  },

  update(
    input: UpdateAgentProviderSettingsInput,
  ): Promise<AgentProvidersSettingsResponse> {
    return typedInvoke(
      "update_agent_provider_settings",
      { input },
      AgentProvidersSettingsResponseSchema,
    );
  },
} as const;
