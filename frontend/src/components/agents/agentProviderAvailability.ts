import type { AgentProviderSettingsResponse } from "@/api/harness-providers";
import type {
  AgentProvider,
  AgentRuntimeSelection,
} from "@/stores/agentSessionStore";
import type { AgentModelRegistry } from "@/lib/agent-models";

import {
  AGENT_PROVIDER_OPTIONS,
  defaultModelForProvider,
  normalizeRuntimeSelection,
} from "./agentOptions";

export interface AgentProviderAvailabilityOption {
  id: AgentProvider;
  label: string;
  disabled?: boolean;
  disabledReason?: string;
  cliVersion?: string | null;
  supportedModelAliases?: readonly string[] | null;
  supportedEfforts?: readonly string[] | null;
  supportsFastMode?: boolean;
  fastModeSupportedModels?: readonly string[];
}

/**
 * How provider availability is decided.
 *
 * - `"local"`: the host ran CLI probes, so `available`/`binaryFound`/`status` are live truth.
 * - `"remote"`: the host served stored config only (`list_remote_agent_providers`). Probe truth
 *   is unavailable BY DESIGN — a remote provider is selectable iff it is `enabled`, the copy
 *   claims configuration rather than validation, and a start-time failure surfaces later through
 *   the conversation-start intent status. `available`/`binaryFound` are NOT consulted here.
 */
export type AgentProviderAvailabilityMode = "local" | "remote";

export function buildAgentProviderAvailabilityOptions({
  providers,
  isReady,
  mode = "local",
}: {
  providers: readonly AgentProviderSettingsResponse[];
  isReady: boolean;
  mode?: AgentProviderAvailabilityMode;
}): AgentProviderAvailabilityOption[] {
  return AGENT_PROVIDER_OPTIONS.map((option) => {
    const provider = providers.find((item) => item.provider === option.id);
    const disabledReason = provider
      ? mode === "remote"
        ? remoteProviderUnavailableReason(provider)
        : providerUnavailableReason(provider)
      : isReady
        ? mode === "remote"
          ? "Not configured on this host."
          : "Provider is not configured."
        : "Checking provider status.";

    return {
      ...option,
      ...(provider?.cliVersion !== undefined
        ? { cliVersion: provider.cliVersion }
        : {}),
      ...(provider?.supportedModelAliases !== undefined
        ? { supportedModelAliases: provider.supportedModelAliases }
        : {}),
      ...(provider?.supportedEfforts !== undefined
        ? { supportedEfforts: provider.supportedEfforts }
        : {}),
      ...(provider?.supportsFastMode !== undefined
        ? { supportsFastMode: provider.supportsFastMode }
        : {}),
      ...(provider?.fastModeSupportedModels !== undefined
        ? { fastModeSupportedModels: provider.fastModeSupportedModels }
        : {}),
      ...(disabledReason ? { disabled: true, disabledReason } : {}),
    };
  });
}

export function supportedEffortsForProvider(
  providerOptions: readonly AgentProviderAvailabilityOption[],
  provider: AgentProvider,
): readonly string[] | null {
  return providerOptions.find((option) => option.id === provider)?.supportedEfforts ?? null;
}

export function supportedModelAliasesForProvider(
  providerOptions: readonly AgentProviderAvailabilityOption[],
  provider: AgentProvider,
): readonly string[] | null {
  return (
    providerOptions.find((option) => option.id === provider)?.supportedModelAliases ?? null
  );
}

export function findSelectableAgentProvider(
  options: readonly AgentProviderAvailabilityOption[],
  preferredProvider?: AgentProvider | null,
): AgentProvider | null {
  const preferred =
    preferredProvider && options.find((option) => option.id === preferredProvider);
  if (preferred && !preferred.disabled) {
    return preferred.id;
  }
  return options.find((option) => !option.disabled)?.id ?? null;
}

export function normalizeRuntimeForSelectableProvider({
  runtime,
  providerOptions,
  defaultProvider,
  modelRegistry,
}: {
  runtime: AgentRuntimeSelection;
  providerOptions: readonly AgentProviderAvailabilityOption[];
  defaultProvider?: AgentProvider | null;
  modelRegistry?: AgentModelRegistry;
}): AgentRuntimeSelection | null {
  const normalizedRuntime = normalizeRuntimeSelection(
    runtime,
    modelRegistry,
    supportedEffortsForProvider(providerOptions, runtime.provider),
    supportedModelAliasesForProvider(providerOptions, runtime.provider),
  );
  const selectedProvider = findSelectableAgentProvider(
    providerOptions,
    normalizedRuntime.provider,
  );
  if (selectedProvider === normalizedRuntime.provider) {
    return normalizedRuntime;
  }

  const fallbackProvider =
    findSelectableAgentProvider(providerOptions, defaultProvider) ?? selectedProvider;
  if (!fallbackProvider) {
    return null;
  }

  return normalizeRuntimeSelection(
    {
      provider: fallbackProvider,
      modelId: defaultModelForProvider(
        fallbackProvider,
        modelRegistry,
        supportedModelAliasesForProvider(providerOptions, fallbackProvider),
      ),
    },
    modelRegistry,
    supportedEffortsForProvider(providerOptions, fallbackProvider),
    supportedModelAliasesForProvider(providerOptions, fallbackProvider),
  );
}

export function getProviderAvailabilityMessage({
  provider,
  providerOptions,
  isReady,
  isRemoteEnvironment = false,
}: {
  provider: AgentProvider;
  providerOptions: readonly AgentProviderAvailabilityOption[];
  isReady: boolean;
  /**
   * Remote clients cannot verify a provider's CLI: `harness-providers.ts` deliberately
   * projects `available: false` rather than faking it true, so every option arrives
   * disabled with a "Configured on this host" note. Treating that as a SEND BLOCKER left
   * the composer permanently dead on a paired device. The host is the authority — the
   * start and continuation intents re-validate provider/model at claim time and fail
   * closed with typed errors — so remotely this is informational only.
   */
  isRemoteEnvironment?: boolean;
}): string | null {
  if (isRemoteEnvironment) {
    return null;
  }
  if (!isReady) {
    return "Checking provider readiness.";
  }
  const selectedOption = providerOptions.find((option) => option.id === provider);
  if (selectedOption?.disabledReason) {
    return selectedOption.disabledReason;
  }
  if (!providerOptions.some((option) => !option.disabled)) {
    return "Enable a provider with a validated CLI in Settings.";
  }
  return null;
}

function providerUnavailableReason(
  provider: AgentProviderSettingsResponse,
): string | null {
  if (!provider.enabled) {
    return "Enable in Settings.";
  }
  if (provider.missingCoreExecFeatures.length > 0) {
    return `Missing CLI support: ${provider.missingCoreExecFeatures.join(", ")}.`;
  }
  if (!provider.available) {
    return provider.error ?? provider.status ?? "CLI validation failed.";
  }
  return null;
}

/**
 * Remote selectability rests on stored enablement alone. The host did not probe the CLI, so
 * `available`/`binaryFound`/`status` carry no validation truth and are deliberately ignored;
 * an enabled provider is offered and the copy says so honestly.
 */
function remoteProviderUnavailableReason(
  provider: AgentProviderSettingsResponse,
): string | null {
  if (!provider.enabled) {
    return "Enable this provider on the host.";
  }
  return null;
}
