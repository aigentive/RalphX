import { describe, expect, it } from "vitest";

import type { AgentProviderSettingsResponse } from "@/api/harness-providers";

import {
  buildAgentProviderAvailabilityOptions,
  getProviderAvailabilityMessage,
  supportedEffortsForProvider,
  supportedModelAliasesForProvider,
} from "./agentProviderAvailability";

function provider(
  overrides: Partial<AgentProviderSettingsResponse>
): AgentProviderSettingsResponse {
  return {
    provider: "codex",
    enabled: true,
    isDefault: true,
    model: "gpt-5.5",
    effort: "xhigh",
    approvalPolicy: "never",
    sandboxMode: "danger-full-access",
    serviceTier: null,
    claudePermissionMode: null,
    claudeDangerouslySkipPermissions: false,
    claudeAllowDangerouslySkipPermissions: false,
    available: true,
    binaryFound: true,
    binaryPath: "/opt/homebrew/bin/codex",
    status: "Available",
    error: null,
    missingCoreExecFeatures: [],
    supportsFastMode: false,
    fastModeSupportedModels: [],
    updatedAt: "2026-05-12T00:00:00.000Z",
    ...overrides,
  };
}

describe("agent provider availability", () => {
  it("reports readiness while provider validation is still loading", () => {
    expect(
      getProviderAvailabilityMessage({
        provider: "codex",
        providerOptions: buildAgentProviderAvailabilityOptions({
          providers: [],
          isReady: false,
        }),
        isReady: false,
      })
    ).toBe("Checking provider readiness.");
  });

  it("never blocks sending on a remote client, where provider availability is unverifiable", () => {
    // `harness-providers.ts` projects `available: false` remotely on purpose ("do NOT fake
    // it true"), so every option arrives disabled with a host-only note. That note is
    // informational: the host re-validates provider/model when it claims the intent. Feeding
    // it into `sendDisabledReason` left the composer permanently dead on a paired device.
    const providerOptions = buildAgentProviderAvailabilityOptions({
      providers: [provider({ enabled: true, available: false })],
      isReady: true,
    });

    expect(
      getProviderAvailabilityMessage({
        provider: "codex",
        providerOptions,
        isReady: true,
        isRemoteEnvironment: true,
      })
    ).toBeNull();

    // Still loading remotely must not block either — readiness is equally unverifiable.
    expect(
      getProviderAvailabilityMessage({
        provider: "codex",
        providerOptions,
        isReady: false,
        isRemoteEnvironment: true,
      })
    ).toBeNull();

    // Local behaviour is unchanged: the same disabled option still reports its reason.
    expect(
      getProviderAvailabilityMessage({
        provider: "codex",
        providerOptions,
        isReady: true,
      })
    ).not.toBeNull();
  });

  it("reports missing CLI feature validation and all-disabled providers", () => {
    const providerOptions = buildAgentProviderAvailabilityOptions({
      providers: [
        provider({
          enabled: true,
          available: true,
          missingCoreExecFeatures: ["json output", "resume"],
        }),
      ],
      isReady: true,
    });

    expect(providerOptions.find((option) => option.id === "codex")).toMatchObject({
      disabled: true,
      disabledReason: "Missing CLI support: json output, resume.",
    });
    expect(
      getProviderAvailabilityMessage({
        provider: "claude",
        providerOptions,
        isReady: true,
      })
    ).toBe("Provider is not configured.");
    expect(
      getProviderAvailabilityMessage({
        provider: "codex",
        providerOptions: [{ id: "codex", label: "Codex", disabled: true }],
        isReady: true,
      })
    ).toBe("Enable a provider with a validated CLI in Settings.");
  });

  it("preserves provider-supported effort capabilities", () => {
    const providerOptions = buildAgentProviderAvailabilityOptions({
      providers: [
        provider({
          provider: "claude",
          supportedEfforts: ["low", "medium", "high", "max"],
        }),
      ],
      isReady: true,
    });

    expect(supportedEffortsForProvider(providerOptions, "claude")).toEqual([
      "low",
      "medium",
      "high",
      "max",
    ]);
  });

  it("preserves provider-supported model aliases", () => {
    const providerOptions = buildAgentProviderAvailabilityOptions({
      providers: [
        provider({
          provider: "claude",
          supportedModelAliases: ["sonnet", "opus", "haiku", "fable"],
        }),
      ],
      isReady: true,
    });

    expect(supportedModelAliasesForProvider(providerOptions, "claude")).toEqual([
      "sonnet",
      "opus",
      "haiku",
      "fable",
    ]);
  });

  it("preserves Codex Fast capability metadata", () => {
    const providerOptions = buildAgentProviderAvailabilityOptions({
      providers: [
        provider({
          supportsFastMode: true,
          fastModeSupportedModels: ["gpt-5.5", "gpt-5.4"],
        }),
      ],
      isReady: true,
    });

    expect(providerOptions.find((option) => option.id === "codex")).toMatchObject({
      supportsFastMode: true,
      fastModeSupportedModels: ["gpt-5.5", "gpt-5.4"],
    });
  });
});
