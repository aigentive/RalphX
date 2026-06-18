import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, renderHook, waitFor } from "@testing-library/react";
import type { ReactNode } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { providerCliManagementApi } from "@/api/provider-cli-management";

import {
  providerCliManagementKeys,
  useProviderCliManagement,
} from "./useProviderCliManagement";

vi.mock("@/api/provider-cli-management", () => ({
  providerCliManagementApi: {
    status: vi.fn(),
    installOrUpdate: vi.fn(),
    autoUpdate: vi.fn(),
  },
}));

function createWrapper(queryClient: QueryClient) {
  return function Wrapper({ children }: { children: ReactNode }) {
    return (
      <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
    );
  };
}

describe("useProviderCliManagement", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("loads managed provider CLI statuses", async () => {
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
    });
    vi.mocked(providerCliManagementApi.status).mockResolvedValue({
      providers: [
        {
          provider: "codex",
          cliManagementMode: "rx_managed",
          autoUpdateEnabled: false,
          supported: true,
          installed: false,
          binaryPath: "/mock/codex",
          currentVersion: null,
          latestVersion: "0.137.0",
          updateAvailable: false,
          action: "install",
          status: "RX-managed codex is not installed.",
          error: null,
        },
      ],
    });

    const { result } = renderHook(() => useProviderCliManagement(), {
      wrapper: createWrapper(queryClient),
    });

    expect(result.current.statuses.providers).toEqual([]);

    await waitFor(() => {
      expect(result.current.statusByProvider.get("codex")?.action).toBe("install");
    });
  });

  it("invalidates managed CLI and provider queries after actions", async () => {
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false }, mutations: { retry: false } },
    });
    const invalidateQueries = vi.spyOn(queryClient, "invalidateQueries");
    vi.mocked(providerCliManagementApi.status).mockResolvedValue({ providers: [] });
    vi.mocked(providerCliManagementApi.installOrUpdate).mockResolvedValue({
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

    const { result } = renderHook(() => useProviderCliManagement(), {
      wrapper: createWrapper(queryClient),
    });

    await act(async () => {
      await result.current.installOrUpdateProviderAsync({ provider: "codex" });
    });

    expect(providerCliManagementApi.installOrUpdate).toHaveBeenCalledWith({
      provider: "codex",
    });
    expect(invalidateQueries).toHaveBeenCalledWith({
      queryKey: providerCliManagementKeys.all,
    });
    expect(invalidateQueries).toHaveBeenCalledWith({
      queryKey: ["agent", "providers"],
    });
    expect(invalidateQueries).toHaveBeenCalledWith({
      queryKey: ["agent", "harness"],
    });
  });
});
