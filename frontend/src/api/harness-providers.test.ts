import { beforeEach, describe, expect, it, vi } from "vitest";

import { typedInvoke } from "@/lib/tauri";

import { harnessProvidersApi } from "./harness-providers";

vi.mock("@/lib/tauri", () => ({
  typedInvoke: vi.fn(),
}));

describe("harnessProvidersApi", () => {
  beforeEach(() => {
    vi.clearAllMocks();
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
      {},
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
    });

    expect(typedInvoke).toHaveBeenCalledWith(
      "update_agent_provider_settings",
      {
        input: {
          provider: "codex",
          enabled: true,
          isDefault: true,
        },
      },
      expect.any(Object),
    );
  });
});
