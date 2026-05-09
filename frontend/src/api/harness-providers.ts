import { typedInvoke } from "@/lib/tauri";
import { z } from "zod";

import { HarnessSchema } from "./ideation-harness";

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
  available: z.boolean(),
  binaryFound: z.boolean(),
  binaryPath: z.string().nullable().optional(),
  status: z.string(),
  error: z.string().nullable().optional(),
  missingCoreExecFeatures: z.array(z.string()),
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
  resetToDefaults?: boolean;
  applyToAllLanes?: boolean;
}

export const harnessProvidersApi = {
  list(): Promise<AgentProvidersSettingsResponse> {
    return typedInvoke(
      "get_agent_provider_settings",
      {},
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
