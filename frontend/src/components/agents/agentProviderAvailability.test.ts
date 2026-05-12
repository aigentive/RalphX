import { describe, expect, it } from "vitest";

import type { AgentProviderSettingsResponse } from "@/api/harness-providers";

import {
  buildAgentProviderAvailabilityOptions,
  getProviderAvailabilityMessage,
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
    claudePermissionMode: null,
    claudeDangerouslySkipPermissions: false,
    claudeAllowDangerouslySkipPermissions: false,
    available: true,
    binaryFound: true,
    binaryPath: "/opt/homebrew/bin/codex",
    status: "Available",
    error: null,
    missingCoreExecFeatures: [],
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
});
