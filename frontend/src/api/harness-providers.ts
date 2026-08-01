import { typedInvoke } from "@/lib/tauri";
import {
  getTransportEnvironmentId,
  isRemoteEnvironmentId,
} from "@/lib/remote/active-environment";
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
  serviceTier: z.string().nullable().optional(),
  approvalPolicy: z.string().nullable().optional(),
  sandboxMode: z.string().nullable().optional(),
  claudePermissionMode: z.string().nullable().optional(),
  claudeDangerouslySkipPermissions: z.boolean(),
  claudeAllowDangerouslySkipPermissions: z.boolean(),
  cliManagementMode: ProviderCliManagementModeSchema.optional(),
  autoUpdateEnabled: z.boolean().optional(),
  customBinaryEnabled: z.boolean().optional(),
  customBinaryPath: z.string().nullable().optional(),
  customEnvFileEnabled: z.boolean().optional(),
  customEnvFilePath: z.string().nullable().optional(),
  available: z.boolean(),
  binaryFound: z.boolean(),
  binaryPath: z.string().nullable().optional(),
  status: z.string(),
  error: z.string().nullable().optional(),
  missingCoreExecFeatures: z.array(z.string()),
  cliVersion: z.string().nullable().optional(),
  supportedModelAliases: z.array(z.string().min(1)).nullable().optional(),
  supportedEfforts: z.array(z.string().min(1)).nullable().optional(),
  ultraSupportedModels: z.array(z.string().min(1)).default([]),
  supportsFastMode: z.boolean().optional().default(false),
  fastModeSupportedModels: z.array(z.string().min(1)).optional().default([]),
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
  serviceTier?: string | null;
  approvalPolicy?: string | null;
  sandboxMode?: string | null;
  claudePermissionMode?: string | null;
  claudeDangerouslySkipPermissions?: boolean;
  claudeAllowDangerouslySkipPermissions?: boolean;
  cliManagementMode?: z.infer<typeof ProviderCliManagementModeSchema>;
  autoUpdateEnabled?: boolean;
  customBinaryEnabled?: boolean;
  customBinaryPath?: string | null;
  customEnvFileEnabled?: boolean;
  customEnvFilePath?: string | null;
  resetToDefaults?: boolean;
  applyToAllLanes?: boolean;
}

export interface ListAgentProviderSettingsOptions {
  refreshRuntime?: boolean;
}

/**
 * The host's spawn-free provider projection (`list_remote_agent_providers`): identity plus
 * stored selection only. The host deliberately does NOT serve `available`, `binaryFound`,
 * `status`, CLI paths, probe data, or `serviceTier` — those are the `Denied` provider-settings
 * surface. Availability is therefore unknowable remotely; the composer treats an `enabled`
 * provider as selectable and a start-time failure surfaces through the intent status.
 */
export const RemoteAgentProviderSchema = z
  .object({
    provider: HarnessSchema,
    enabled: z.boolean(),
    isDefault: z.boolean(),
    model: z.string().nullable().optional(),
    effort: z.string().nullable().optional(),
  })
  // Strict: a host that ever leaked a path/probe/credential field into this projection would
  // fail parsing loudly rather than smuggle it through as a silently-stripped extra key.
  .strict();

export type RemoteAgentProvider = z.infer<typeof RemoteAgentProviderSchema>;

const RemoteAgentProviderListSchema = z.array(RemoteAgentProviderSchema);

/**
 * True while the active environment is remote, so the composer's provider feed must use the
 * spawn-free `list_remote_agent_providers` twin rather than the `Denied`
 * `get_agent_provider_settings`.
 */
function remoteProviderReadsEnabled(): boolean {
  return isRemoteEnvironmentId(getTransportEnvironmentId());
}

/**
 * Lift a remote provider row into the shared envelope WITHOUT fabricating validation truth.
 * `available`/`binaryFound` stay `false` (the host never probed), `status` names the honest
 * remote condition, and `missingCoreExecFeatures` is empty. The composer's availability
 * builder is passed `mode: "remote"` so selectability derives from `enabled`, never from
 * these placeholders — see `buildAgentProviderAvailabilityOptions`.
 */
function remoteRowToEnvelope(
  row: RemoteAgentProvider,
): AgentProviderSettingsResponse {
  return {
    provider: row.provider,
    enabled: row.enabled,
    isDefault: row.isDefault,
    model: row.model ?? null,
    effort: row.effort ?? null,
    serviceTier: null,
    approvalPolicy: null,
    sandboxMode: null,
    claudePermissionMode: null,
    claudeDangerouslySkipPermissions: false,
    claudeAllowDangerouslySkipPermissions: false,
    customBinaryEnabled: false,
    customBinaryPath: null,
    customEnvFileEnabled: false,
    customEnvFilePath: null,
    // Availability is not validated remotely; do NOT fake it true.
    available: false,
    binaryFound: false,
    status: "Configured on this host",
    error: null,
    missingCoreExecFeatures: [],
    ultraSupportedModels: [],
    supportsFastMode: false,
    fastModeSupportedModels: [],
    updatedAt: "",
  };
}

export const harnessProvidersApi = {
  list(
    options: ListAgentProviderSettingsOptions = {},
  ): Promise<AgentProvidersSettingsResponse> {
    // Two literal `typedInvoke` call sites, never a computed name (P-11): the drift scan reads
    // the invoke argument literally. `refreshRuntime` is dropped on the remote branch — it is
    // the CLI-probe path, unavailable and unwanted remotely.
    if (remoteProviderReadsEnabled()) {
      return typedInvoke(
        "list_remote_agent_providers",
        {},
        RemoteAgentProviderListSchema,
      ).then((rows) => {
        const providers = rows.map(remoteRowToEnvelope);
        const defaultProvider =
          rows.find((row) => row.enabled && row.isDefault)?.provider ?? null;
        return {
          providers,
          defaultProvider,
          requiresOnboarding: !providers.some((row) => row.enabled),
        };
      });
    }
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
