import { beforeEach, describe, expect, it, vi } from "vitest";

import { typedInvoke } from "@/lib/tauri";

import { providerCliManagementApi } from "./provider-cli-management";

vi.mock("@/lib/tauri", () => ({
  typedInvoke: vi.fn(),
}));

describe("providerCliManagementApi", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("invokes the managed provider CLI status command", async () => {
    vi.mocked(typedInvoke).mockResolvedValue({ providers: [] });

    await expect(providerCliManagementApi.status()).resolves.toEqual({
      providers: [],
    });

    expect(typedInvoke).toHaveBeenCalledWith(
      "get_managed_provider_cli_status",
      {},
      expect.any(Object),
    );
  });

  it("invokes the managed provider CLI install/update command", async () => {
    vi.mocked(typedInvoke).mockResolvedValue({
      provider: "codex",
      success: true,
      status: {
        provider: "codex",
        cliManagementMode: "rx_managed",
        autoUpdateEnabled: false,
        supported: true,
        installed: true,
        binaryPath: "/mock/codex",
        currentVersion: "0.137.0",
        latestVersion: "0.137.0",
        updateAvailable: false,
        action: "none",
        status: "ready",
        error: null,
      },
      stdout: null,
      stderr: null,
    });

    await providerCliManagementApi.installOrUpdate({ provider: "codex" });

    expect(typedInvoke).toHaveBeenCalledWith(
      "install_or_update_managed_provider_cli",
      { input: { provider: "codex" } },
      expect.any(Object),
    );
  });

  it("invokes the managed provider CLI auto-update command", async () => {
    vi.mocked(typedInvoke).mockResolvedValue({
      updated: [],
      skipped: [],
    });

    await providerCliManagementApi.autoUpdate();

    expect(typedInvoke).toHaveBeenCalledWith(
      "auto_update_managed_provider_clis",
      {},
      expect.any(Object),
    );
  });
});
