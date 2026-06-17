import { beforeEach, describe, expect, it, vi } from "vitest";

import { typedInvoke } from "@/lib/tauri";

import { linearApi } from "./linear";

vi.mock("@/lib/tauri", () => ({
  typedInvoke: vi.fn(),
}));

describe("linearApi", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("loads, saves, and validates Linear integration settings", async () => {
    const settings = {
      enabled: true,
      hasApiToken: true,
      validationStatus: "valid",
      issueSearchAvailable: true,
      lastValidatedAt: "2026-06-17T12:00:00Z",
      lastError: null,
      updatedAt: "2026-06-17T12:00:00Z",
    };
    vi.mocked(typedInvoke).mockResolvedValue(settings);

    await expect(linearApi.getSettings()).resolves.toEqual(settings);
    await expect(
      linearApi.saveSettings({ apiToken: "lin-api-token" }),
    ).resolves.toEqual(settings);
    await expect(linearApi.validate()).resolves.toEqual(settings);

    expect(typedInvoke).toHaveBeenNthCalledWith(
      1,
      "get_linear_integration_settings",
      {},
      expect.any(Object),
    );
    expect(typedInvoke).toHaveBeenNthCalledWith(
      2,
      "save_linear_integration_settings",
      { input: { apiToken: "lin-api-token" } },
      expect.any(Object),
    );
    expect(typedInvoke).toHaveBeenNthCalledWith(
      3,
      "validate_linear_integration",
      {},
      expect.any(Object),
    );
  });

  it("unwraps Linear issue search responses", async () => {
    vi.mocked(typedInvoke).mockResolvedValue({
      issues: [
        {
          id: "issue-id",
          key: "LIN-123",
          title: "Fix search",
          url: "https://linear.app/acme/issue/LIN-123/fix-search",
          excerpt: "Search issue body",
          stateName: "In Progress",
        },
      ],
    });

    await expect(
      linearApi.searchIssues({ query: "LIN-123", limit: 5 }),
    ).resolves.toEqual([
      {
        id: "issue-id",
        key: "LIN-123",
        title: "Fix search",
        url: "https://linear.app/acme/issue/LIN-123/fix-search",
        excerpt: "Search issue body",
        stateName: "In Progress",
      },
    ]);

    expect(typedInvoke).toHaveBeenCalledWith(
      "search_linear_issues",
      { input: { query: "LIN-123", limit: 5 } },
      expect.any(Object),
    );
  });

  it("invokes hidden webhook compatibility commands", async () => {
    vi.mocked(typedInvoke).mockResolvedValue({
      enabled: true,
      hasSigningSecret: true,
    });

    await expect(linearApi.getWebhookConfig()).resolves.toEqual({
      enabled: true,
      hasSigningSecret: true,
    });
    await expect(
      linearApi.saveWebhookSigningSecret({
        signingSecret: "secret",
        enabled: false,
      }),
    ).resolves.toEqual({
      enabled: true,
      hasSigningSecret: true,
    });

    expect(typedInvoke).toHaveBeenNthCalledWith(
      1,
      "get_linear_webhook_config",
      {},
      expect.any(Object),
    );
    expect(typedInvoke).toHaveBeenNthCalledWith(
      2,
      "save_linear_webhook_signing_secret",
      { input: { signingSecret: "secret", enabled: false } },
      expect.any(Object),
    );
  });
});
