import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, renderHook, waitFor } from "@testing-library/react";
import type { ReactNode } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { harnessProvidersApi } from "@/api/harness-providers";
import { manualRoleDefaultKeys } from "@/hooks/useManualRoleDefaults";

import { harnessProviderKeys, useHarnessProviders } from "./useHarnessProviders";

vi.mock("@/api/harness-providers", () => ({
  harnessProvidersApi: {
    list: vi.fn(),
    update: vi.fn(),
  },
}));

function createWrapper(queryClient: QueryClient) {
  return function Wrapper({ children }: { children: ReactNode }) {
    return (
      <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
    );
  };
}

describe("useHarnessProviders", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("returns empty onboarding settings until provider settings load", async () => {
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
    });
    vi.mocked(harnessProvidersApi.list).mockResolvedValue({
      providers: [
        {
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
          status: "Available codex detected at /opt/homebrew/bin/codex.",
          error: null,
          missingCoreExecFeatures: [],
          supportsFastMode: true,
          fastModeSupportedModels: ["gpt-5.5", "gpt-5.4"],
          updatedAt: new Date().toISOString(),
        },
      ],
      defaultProvider: "codex",
      requiresOnboarding: false,
    });

    const { result } = renderHook(() => useHarnessProviders(), {
      wrapper: createWrapper(queryClient),
    });

    expect(result.current.settings.requiresOnboarding).toBe(true);
    expect(result.current.providers).toEqual([]);

    await waitFor(() => {
      expect(result.current.settings.defaultProvider).toBe("codex");
    });
    expect(result.current.providers).toHaveLength(1);
  });

  it("updates provider caches and invalidates runtime metadata after provider updates", async () => {
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
    });
    const invalidateQueries = vi.spyOn(queryClient, "invalidateQueries");
    const updatedSettings = {
      providers: [],
      defaultProvider: "codex" as const,
      requiresOnboarding: false,
    };
    vi.mocked(harnessProvidersApi.list).mockResolvedValue({
      providers: [],
      defaultProvider: null,
      requiresOnboarding: true,
    });
    vi.mocked(harnessProvidersApi.update).mockResolvedValue(updatedSettings);

    const { result } = renderHook(() => useHarnessProviders(), {
      wrapper: createWrapper(queryClient),
    });

    await waitFor(() => {
      expect(result.current.settings.requiresOnboarding).toBe(true);
    });

    await act(async () => {
      await result.current.updateProviderAsync({
        provider: "codex",
        enabled: true,
      });
    });

    expect(harnessProvidersApi.update).toHaveBeenCalledWith({
      provider: "codex",
      enabled: true,
    });
    expect(queryClient.getQueryData(harnessProviderKeys.list(false))).toEqual(
      updatedSettings,
    );
    expect(invalidateQueries).not.toHaveBeenCalledWith({
      queryKey: harnessProviderKeys.all,
    });
    expect(invalidateQueries).toHaveBeenCalledWith({
      queryKey: ["provider-cli-management"],
    });
    expect(invalidateQueries).toHaveBeenCalledWith({
      queryKey: ["agent", "harness"],
    });
    expect(invalidateQueries).toHaveBeenCalledWith({
      queryKey: manualRoleDefaultKeys.all,
    });
  });
});
