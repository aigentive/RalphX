import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { AtlassianIntegrationSettingsPanel } from "./AtlassianIntegrationSettingsPanel";

const NOT_CONFIGURED_SETTINGS = {
  enabled: false,
  authMethod: "api_token" as const,
  siteUrl: null,
  email: null,
  hasApiToken: false,
  oauthClientId: null,
  oauthRedirectUri: null,
  hasOauthClientSecret: false,
  hasOauthToken: false,
  oauthCloudId: null,
  oauthScopes: null,
  validationStatus: "not_configured",
  jiraAvailable: false,
  confluenceAvailable: false,
  lastValidatedAt: null,
  lastError: null,
  updatedAt: new Date(0).toISOString(),
};

const atlassianHook = vi.hoisted(() => ({
  disconnectAsync: vi.fn(),
  state: {
    settings: {
      enabled: false,
      authMethod: "api_token" as const,
      siteUrl: null as string | null,
      email: null as string | null,
      hasApiToken: false,
      oauthClientId: null as string | null,
      oauthRedirectUri: null as string | null,
      hasOauthClientSecret: false,
      hasOauthToken: false,
      oauthCloudId: null as string | null,
      oauthScopes: null as string | null,
      validationStatus: "not_configured",
      jiraAvailable: false,
      confluenceAvailable: false,
      lastValidatedAt: null as string | null,
      lastError: null as string | null,
      updatedAt: new Date(0).toISOString(),
    },
    isLoading: false,
    isError: false,
    error: null as Error | null,
    isSaving: false,
    isValidating: false,
    isDisconnecting: false,
    isBuildingOAuthAuthorization: false,
    isStartingOAuthLocalCallback: false,
    isCompletingOAuthLocalCallback: false,
    isExchangingOAuthCode: false,
    saveError: null as Error | null,
    validateError: null as Error | null,
    disconnectError: null as Error | null,
    buildOAuthAuthorizationError: null as Error | null,
    startOAuthLocalCallbackError: null as Error | null,
    completeOAuthLocalCallbackError: null as Error | null,
    exchangeOAuthCodeError: null as Error | null,
  },
}));

vi.mock("@tauri-apps/plugin-opener", () => ({
  openUrl: vi.fn().mockResolvedValue(undefined),
}));

vi.mock("@/hooks/useFeatureFlags", () => ({
  useFeatureFlags: () => ({ data: { atlassianOauth: false } }),
}));

vi.mock("@/hooks/useAtlassianIntegration", async () => ({
  ...(await vi.importActual<typeof import("@/hooks/useAtlassianIntegration")>(
    "@/hooks/useAtlassianIntegration",
  )),
  useAtlassianIntegration: () => ({
    ...atlassianHook.state,
    saveAsync: vi.fn(),
    validateAsync: vi.fn(),
    disconnectAsync: atlassianHook.disconnectAsync,
    buildOAuthAuthorizationAsync: vi.fn(),
    startOAuthLocalCallbackAsync: vi.fn(),
    completeOAuthLocalCallbackAsync: vi.fn(),
    exchangeOAuthCodeAsync: vi.fn(),
  }),
}));

describe("AtlassianIntegrationSettingsPanel", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    atlassianHook.state.settings = { ...NOT_CONFIGURED_SETTINGS };
    atlassianHook.state.isDisconnecting = false;
    atlassianHook.state.disconnectError = null;
    atlassianHook.disconnectAsync.mockResolvedValue({
      ...NOT_CONFIGURED_SETTINGS,
    });
  });

  it("paints a red status banner with no disconnect when not configured", () => {
    render(<AtlassianIntegrationSettingsPanel />);

    expect(screen.getByText("Not configured")).toBeInTheDocument();
    expect(screen.getByTestId("integration-status-banner")).toHaveAttribute(
      "data-connected",
      "false",
    );
    expect(
      screen.queryByRole("button", { name: "Disconnect" }),
    ).not.toBeInTheDocument();
  });

  it("paints the status banner green and offers disconnect when connected", () => {
    atlassianHook.state.settings = {
      ...NOT_CONFIGURED_SETTINGS,
      enabled: true,
      siteUrl: "https://example.atlassian.net",
      email: "dev@example.com",
      hasApiToken: true,
      validationStatus: "valid",
      jiraAvailable: true,
      confluenceAvailable: true,
      lastValidatedAt: new Date(0).toISOString(),
    };

    render(<AtlassianIntegrationSettingsPanel />);

    expect(screen.getByText("Enabled")).toBeInTheDocument();
    expect(screen.getByTestId("integration-status-banner")).toHaveAttribute(
      "data-connected",
      "true",
    );
    expect(
      screen.getByRole("button", { name: "Disconnect" }),
    ).toBeInTheDocument();
  });

  it("clears the connection after confirming disconnect", async () => {
    const user = userEvent.setup();
    atlassianHook.state.settings = {
      ...NOT_CONFIGURED_SETTINGS,
      enabled: true,
      siteUrl: "https://example.atlassian.net",
      email: "dev@example.com",
      hasApiToken: true,
      validationStatus: "valid",
      jiraAvailable: true,
      confluenceAvailable: true,
    };

    render(<AtlassianIntegrationSettingsPanel />);

    await user.click(screen.getByRole("button", { name: "Disconnect" }));
    expect(atlassianHook.disconnectAsync).not.toHaveBeenCalled();

    await user.click(screen.getByRole("button", { name: "Confirm disconnect" }));

    expect(atlassianHook.disconnectAsync).toHaveBeenCalledTimes(1);
  });
});
