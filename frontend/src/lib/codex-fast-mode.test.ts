import { describe, expect, it } from "vitest";

import type { AgentProviderSettingsResponse } from "@/api/harness-providers";

import { codexFastModeAvailabilityForProvider } from "./codex-fast-mode";

function codexProvider(
  overrides: Partial<AgentProviderSettingsResponse> = {},
): AgentProviderSettingsResponse {
  return {
    provider: "codex",
    enabled: true,
    isDefault: true,
    model: "gpt-5.5",
    effort: "xhigh",
    serviceTier: null,
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
    supportsFastMode: true,
    fastModeSupportedModels: ["gpt-5.5", "gpt-5.4"],
    updatedAt: "2026-06-28T00:00:00.000Z",
    ...overrides,
  };
}

describe("codexFastModeAvailabilityForProvider", () => {
  it("allows supported Codex models", () => {
    expect(
      codexFastModeAvailabilityForProvider({
        provider: codexProvider(),
        modelId: "gpt-5.5",
        isReady: true,
      }),
    ).toEqual({ supported: true, reason: null });
  });

  it("rejects models without a Fast service tier", () => {
    expect(
      codexFastModeAvailabilityForProvider({
        provider: codexProvider(),
        modelId: "gpt-5.4-mini",
        isReady: true,
      }),
    ).toEqual({
      supported: false,
      reason: "Fast mode is not available for gpt-5.4-mini.",
    });
  });

  it("rejects unavailable or non-Fast-capable Codex providers", () => {
    expect(
      codexFastModeAvailabilityForProvider({
        provider: codexProvider({ supportsFastMode: false }),
        modelId: "gpt-5.5",
        isReady: true,
      }).reason,
    ).toBe("Fast mode is not available for this Codex CLI or model catalog.");

    expect(
      codexFastModeAvailabilityForProvider({
        provider: codexProvider({
          available: false,
          error: "Codex CLI validation failed",
        }),
        modelId: "gpt-5.5",
        isReady: true,
      }).reason,
    ).toBe("Codex CLI validation failed");
  });
});
