import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, renderHook } from "@testing-library/react";
import { createElement, type ReactNode } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { atlassianApi, type AtlassianIntegrationSettings } from "@/api/atlassian";
import { linearApi, type LinearIntegrationSettings } from "@/api/linear";

import {
  atlassianIntegrationKeys,
  useAtlassianIntegration,
} from "./useAtlassianIntegration";
import {
  linearIntegrationKeys,
  useLinearIntegration,
} from "./useLinearIntegration";
import { ticketingKeys } from "./useTicketing";

vi.mock("@/api/atlassian", () => ({
  atlassianApi: {
    getSettings: vi.fn(),
    saveSettings: vi.fn(),
    validate: vi.fn(),
    disconnect: vi.fn(),
    buildOAuthAuthorization: vi.fn(),
    startOAuthLocalCallback: vi.fn(),
    completeOAuthLocalCallback: vi.fn(),
    exchangeOAuthCode: vi.fn(),
  },
}));

vi.mock("@/api/linear", () => ({
  linearApi: {
    getSettings: vi.fn(),
    saveSettings: vi.fn(),
    validate: vi.fn(),
    disconnect: vi.fn(),
  },
}));

const ATLASSIAN_NOT_CONFIGURED: AtlassianIntegrationSettings = {
  enabled: false,
  authMethod: "api_token",
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
  updatedAt: "2026-06-23T00:00:00.000Z",
};

const ATLASSIAN_VALID: AtlassianIntegrationSettings = {
  ...ATLASSIAN_NOT_CONFIGURED,
  enabled: true,
  siteUrl: "https://example.atlassian.net",
  email: "dev@example.com",
  hasApiToken: true,
  validationStatus: "valid",
  jiraAvailable: true,
  confluenceAvailable: true,
  lastValidatedAt: "2026-06-23T01:00:00.000Z",
  updatedAt: "2026-06-23T01:00:00.000Z",
};

const LINEAR_NOT_CONFIGURED: LinearIntegrationSettings = {
  enabled: false,
  hasApiToken: false,
  validationStatus: "not_configured",
  issueSearchAvailable: false,
  lastValidatedAt: null,
  lastError: null,
  updatedAt: "2026-06-23T00:00:00.000Z",
};

const LINEAR_VALID: LinearIntegrationSettings = {
  ...LINEAR_NOT_CONFIGURED,
  enabled: true,
  hasApiToken: true,
  validationStatus: "valid",
  issueSearchAvailable: true,
  lastValidatedAt: "2026-06-23T01:00:00.000Z",
  updatedAt: "2026-06-23T01:00:00.000Z",
};

function createQueryClient() {
  return new QueryClient({
    defaultOptions: {
      queries: { retry: false, gcTime: Infinity },
      mutations: { retry: false },
    },
  });
}

function createWrapper(queryClient: QueryClient) {
  function Wrapper({ children }: { children: ReactNode }) {
    return createElement(QueryClientProvider, { client: queryClient }, children);
  }

  return Wrapper;
}

describe("integration hooks ticketing invalidation", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(atlassianApi.getSettings).mockResolvedValue(ATLASSIAN_NOT_CONFIGURED);
    vi.mocked(atlassianApi.saveSettings).mockResolvedValue(ATLASSIAN_VALID);
    vi.mocked(atlassianApi.validate).mockResolvedValue(ATLASSIAN_VALID);
    vi.mocked(atlassianApi.disconnect).mockResolvedValue(ATLASSIAN_NOT_CONFIGURED);
    vi.mocked(atlassianApi.completeOAuthLocalCallback).mockResolvedValue(ATLASSIAN_VALID);
    vi.mocked(atlassianApi.exchangeOAuthCode).mockResolvedValue(ATLASSIAN_VALID);
    vi.mocked(linearApi.getSettings).mockResolvedValue(LINEAR_NOT_CONFIGURED);
    vi.mocked(linearApi.saveSettings).mockResolvedValue(LINEAR_VALID);
    vi.mocked(linearApi.validate).mockResolvedValue(LINEAR_VALID);
    vi.mocked(linearApi.disconnect).mockResolvedValue(LINEAR_NOT_CONFIGURED);
  });

  it("invalidates ticketing providers after Atlassian validation succeeds", async () => {
    const queryClient = createQueryClient();
    const invalidateSpy = vi.spyOn(queryClient, "invalidateQueries");

    const { result } = renderHook(() => useAtlassianIntegration(), {
      wrapper: createWrapper(queryClient),
    });

    await act(() => result.current.validateAsync());

    expect(queryClient.getQueryData(atlassianIntegrationKeys.settings())).toEqual(
      ATLASSIAN_VALID,
    );
    expect(invalidateSpy).toHaveBeenCalledWith({ queryKey: ticketingKeys.all });
  });

  it("invalidates ticketing providers after Atlassian OAuth completes", async () => {
    const queryClient = createQueryClient();
    const invalidateSpy = vi.spyOn(queryClient, "invalidateQueries");

    const { result } = renderHook(() => useAtlassianIntegration(), {
      wrapper: createWrapper(queryClient),
    });

    await act(() => result.current.completeOAuthLocalCallbackAsync({ state: "state-1" }));

    expect(queryClient.getQueryData(atlassianIntegrationKeys.settings())).toEqual(
      ATLASSIAN_VALID,
    );
    expect(invalidateSpy).toHaveBeenCalledWith({ queryKey: ticketingKeys.all });
  });

  it("invalidates ticketing providers after Linear validation succeeds", async () => {
    const queryClient = createQueryClient();
    const invalidateSpy = vi.spyOn(queryClient, "invalidateQueries");

    const { result } = renderHook(() => useLinearIntegration(), {
      wrapper: createWrapper(queryClient),
    });

    await act(() => result.current.validateAsync());

    expect(queryClient.getQueryData(linearIntegrationKeys.settings())).toEqual(
      LINEAR_VALID,
    );
    expect(invalidateSpy).toHaveBeenCalledWith({ queryKey: ticketingKeys.all });
  });
});
