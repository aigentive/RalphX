import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { typedInvoke } from "@/lib/tauri";
import {
  resetTransportEnvironmentId,
  setTransportEnvironmentId,
} from "@/lib/remote/active-environment";

import {
  AgentProviderSettingsResponseSchema,
  RemoteAgentProviderSchema,
  harnessProvidersApi,
} from "./harness-providers";

vi.mock("@/lib/tauri", () => ({
  typedInvoke: vi.fn(),
}));

describe("harnessProvidersApi", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  afterEach(() => {
    resetTransportEnvironmentId();
  });

  it("invokes the provider settings list command", async () => {
    vi.mocked(typedInvoke).mockResolvedValue({
      providers: [],
      defaultProvider: null,
      requiresOnboarding: true,
    });

    await expect(harnessProvidersApi.list()).resolves.toEqual({
      providers: [],
      defaultProvider: null,
      requiresOnboarding: true,
    });

    expect(typedInvoke).toHaveBeenCalledWith(
      "get_agent_provider_settings",
      { input: { refreshRuntime: false, forceRuntime: false } },
      expect.any(Object),
    );
  });

  it("can request a live runtime refresh when listing providers", async () => {
    vi.mocked(typedInvoke).mockResolvedValue({
      providers: [],
      defaultProvider: null,
      requiresOnboarding: true,
    });

    await harnessProvidersApi.list({ refreshRuntime: true });

    expect(typedInvoke).toHaveBeenCalledWith(
      "get_agent_provider_settings",
      { input: { refreshRuntime: true, forceRuntime: false } },
      expect.any(Object),
    );
  });

  it("can force a live runtime re-check when listing providers", async () => {
    vi.mocked(typedInvoke).mockResolvedValue({
      providers: [],
      defaultProvider: null,
      requiresOnboarding: true,
    });

    await harnessProvidersApi.list({ refreshRuntime: true, forceRuntime: true });

    expect(typedInvoke).toHaveBeenCalledWith(
      "get_agent_provider_settings",
      { input: { refreshRuntime: true, forceRuntime: true } },
      expect.any(Object),
    );
  });

  it("invokes the provider settings update command", async () => {
    vi.mocked(typedInvoke).mockResolvedValue({
      providers: [],
      defaultProvider: "codex",
      requiresOnboarding: false,
    });

    await harnessProvidersApi.update({
      provider: "codex",
      enabled: true,
      isDefault: true,
      serviceTier: "fast",
      customEnvFileEnabled: true,
      customEnvFilePath: "/Users/example/.codex.env",
    });

    expect(typedInvoke).toHaveBeenCalledWith(
      "update_agent_provider_settings",
      {
        input: {
          provider: "codex",
          enabled: true,
          isDefault: true,
          serviceTier: "fast",
          customEnvFileEnabled: true,
          customEnvFilePath: "/Users/example/.codex.env",
        },
      },
      expect.any(Object),
    );
  });

  it("routes to the spawn-free remote command under a remote environment", async () => {
    setTransportEnvironmentId("remote-host-1");
    vi.mocked(typedInvoke).mockResolvedValue([
      {
        provider: "codex",
        enabled: true,
        isDefault: true,
        model: "gpt-5.6-sol",
        effort: "high",
      },
      { provider: "claude", enabled: false, isDefault: false },
    ]);

    const result = await harnessProvidersApi.list({ refreshRuntime: true });

    // Literal remote command name, empty args — `refreshRuntime` is NOT forwarded remotely.
    expect(typedInvoke).toHaveBeenCalledWith(
      "list_remote_agent_providers",
      {},
      expect.any(Object),
    );
    expect(result.defaultProvider).toBe("codex");
    expect(result.requiresOnboarding).toBe(false);
    const codex = result.providers.find((p) => p.provider === "codex");
    // Availability is not validated remotely — it must NOT be faked true.
    expect(codex?.available).toBe(false);
    expect(codex?.binaryFound).toBe(false);
    expect(codex?.status).toBe("Configured on this host");
  });

  it("keeps the local command when the environment is local", async () => {
    vi.mocked(typedInvoke).mockResolvedValue({
      providers: [],
      defaultProvider: null,
      requiresOnboarding: true,
    });

    await harnessProvidersApi.list({ refreshRuntime: true });

    expect(typedInvoke).toHaveBeenCalledWith(
      "get_agent_provider_settings",
      { input: { refreshRuntime: true, forceRuntime: false } },
      expect.any(Object),
    );
  });

  it("rejects any field leaked beyond the remote provider allowlist", () => {
    expect(() =>
      RemoteAgentProviderSchema.parse({
        provider: "codex",
        enabled: true,
        isDefault: true,
        model: "gpt-5.6-sol",
        effort: "high",
        binaryPath: "/opt/homebrew/bin/codex",
      }),
    ).toThrow();
  });

  it("defaults absent Fast capability fields for compatibility", () => {
    const parsed = AgentProviderSettingsResponseSchema.parse({
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
      updatedAt: "2026-06-28T00:00:00.000Z",
    });

    expect(parsed.supportsFastMode).toBe(false);
    expect(parsed.fastModeSupportedModels).toEqual([]);
  });
});
