import type { AgentProviderSettingsResponse } from "@/api/harness-providers";

export const CODEX_FAST_MODE_DESCRIPTION =
  "Use Codex priority service tier.";

export interface CodexFastModeAvailability {
  supported: boolean;
  reason: string | null;
}

export function codexFastModeAvailabilityForProvider({
  provider,
  modelId,
  isReady,
}: {
  provider: AgentProviderSettingsResponse | null | undefined;
  modelId: string | null | undefined;
  isReady: boolean;
}): CodexFastModeAvailability {
  if (!isReady) {
    return {
      supported: false,
      reason: "Checking Codex Fast support.",
    };
  }
  if (!provider) {
    return {
      supported: false,
      reason: "Codex provider settings are unavailable.",
    };
  }
  if (!provider.enabled) {
    return {
      supported: false,
      reason: "Enable Codex in Settings.",
    };
  }
  if (provider.missingCoreExecFeatures.length > 0) {
    return {
      supported: false,
      reason: `Missing CLI support: ${provider.missingCoreExecFeatures.join(", ")}.`,
    };
  }
  if (!provider.available) {
    return {
      supported: false,
      reason: provider.error ?? provider.status ?? "Codex CLI validation failed.",
    };
  }
  if (!provider.supportsFastMode) {
    return {
      supported: false,
      reason: "Fast mode is not available for this Codex CLI or model catalog.",
    };
  }

  const supportedModels = provider.fastModeSupportedModels ?? [];
  if (
    modelId &&
    supportedModels.length > 0 &&
    !supportedModels.includes(modelId)
  ) {
    return {
      supported: false,
      reason: `Fast mode is not available for ${modelId}.`,
    };
  }

  return {
    supported: true,
    reason: null,
  };
}
