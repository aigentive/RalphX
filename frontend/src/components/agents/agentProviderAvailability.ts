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

export function buildAgentProviderAvailabilityOptions({
  providers,
  isReady,
}: {
  providers: readonly AgentProviderSettingsResponse[];
  isReady: boolean;
}): AgentProviderAvailabilityOption[] {
  return AGENT_PROVIDER_OPTIONS.map((option) => {
    const provider = providers.find((item) => item.provider === option.id);
    const disabledReason = provider
      ? providerUnavailableReason(provider)
      : isReady
        ? "Provider is not configured."
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
}: {
  provider: AgentProvider;
  providerOptions: readonly AgentProviderAvailabilityOption[];
  isReady: boolean;
}): string | null {
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
