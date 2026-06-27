import { beforeEach, describe, expect, it, vi } from "vitest";

import { typedInvoke } from "@/lib/tauri";

import { GranolaIntegrationSettingsSchema, granolaApi } from "./granola";

vi.mock("@/lib/tauri", () => ({
  typedInvoke: vi.fn(),
}));

describe("granolaApi", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("loads, saves, validates, and disconnects Granola integration settings", async () => {
    const settings = {
      enabled: true,
      hasApiToken: true,
      validationStatus: "valid",
      lastValidatedAt: "2026-06-17T12:00:00Z",
      lastError: null,
      updatedAt: "2026-06-17T12:00:00Z",
    };
    vi.mocked(typedInvoke).mockResolvedValue(settings);

    await expect(granolaApi.getSettings()).resolves.toEqual(settings);
    await expect(
      granolaApi.saveSettings({ apiToken: "granola-token" }),
    ).resolves.toEqual(settings);
    await expect(granolaApi.validate()).resolves.toEqual(settings);
    await expect(granolaApi.disconnect()).resolves.toEqual(settings);

    expect(typedInvoke).toHaveBeenNthCalledWith(
      1,
      "get_granola_integration_settings",
      {},
      expect.any(Object),
    );
    expect(typedInvoke).toHaveBeenNthCalledWith(
      2,
      "save_granola_integration_settings",
      { input: { apiToken: "granola-token" } },
      expect.any(Object),
    );
    expect(typedInvoke).toHaveBeenNthCalledWith(
      3,
      "validate_granola_integration_settings",
      {},
      expect.any(Object),
    );
    expect(typedInvoke).toHaveBeenNthCalledWith(
      4,
      "save_granola_integration_settings",
      { input: { apiToken: "" } },
      expect.any(Object),
    );
  });

  it("parses the camelCase settings response", () => {
    const parsed = GranolaIntegrationSettingsSchema.parse({
      enabled: true,
      hasApiToken: true,
      validationStatus: "valid",
      lastValidatedAt: "2026-06-17T12:00:00Z",
      lastError: null,
      updatedAt: "2026-06-17T12:00:00Z",
    });

    expect(parsed.hasApiToken).toBe(true);
    expect(parsed.validationStatus).toBe("valid");
  });
});
