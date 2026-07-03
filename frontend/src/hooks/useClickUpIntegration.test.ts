import { act, renderHook, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { createElement, type ReactNode } from "react";

import type { ClickUpIntegrationSettings } from "@/api/clickup";
import { clickupApi } from "@/api/clickup";
import { ticketingKeys } from "@/hooks/useTicketing";

import {
  clickupIntegrationKeys,
  isClickUpConnected,
  useClickUpIntegration,
} from "./useClickUpIntegration";

vi.mock("@/api/clickup", () => ({
  clickupApi: {
    getSettings: vi.fn(),
    listWorkspaces: vi.fn(),
    saveSettings: vi.fn(),
    validate: vi.fn(),
    disconnect: vi.fn(),
  },
}));

function settings(
  overrides: Partial<ClickUpIntegrationSettings> = {},
): ClickUpIntegrationSettings {
  return {
    enabled: true,
    hasApiToken: true,
    workspaceId: "team-1",
    validationStatus: "valid",
    taskSearchAvailable: true,
    lastValidatedAt: null,
    lastError: null,
    updatedAt: new Date(0).toISOString(),
    ...overrides,
  };
}

function renderClickUpHook() {
  const queryClient = new QueryClient({
    defaultOptions: {
      queries: { retry: false },
      mutations: { retry: false },
    },
  });
  const wrapper = ({ children }: { children: ReactNode }) => (
    createElement(QueryClientProvider, { client: queryClient }, children)
  );
  return {
    queryClient,
    ...renderHook(() => useClickUpIntegration(), { wrapper }),
  };
}

describe("useClickUpIntegration", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("loads settings and skips workspaces until ClickUp is connected", async () => {
    vi.mocked(clickupApi.getSettings).mockResolvedValue(
      settings({ enabled: false, validationStatus: "pending" }),
    );
    vi.mocked(clickupApi.listWorkspaces).mockResolvedValue([
      { id: "team-1", name: "Acme", color: null },
    ]);

    const { result } = renderClickUpHook();

    await waitFor(() => expect(result.current.isLoading).toBe(false));

    expect(result.current.connected).toBe(false);
    expect(result.current.workspaces).toEqual([]);
    expect(clickupApi.listWorkspaces).not.toHaveBeenCalled();
  });

  it("loads workspaces after the settings query reports a usable connection", async () => {
    vi.mocked(clickupApi.getSettings).mockResolvedValue(settings());
    vi.mocked(clickupApi.listWorkspaces).mockResolvedValue([
      { id: "team-1", name: "Acme", color: "#ff6b35" },
    ]);

    const { result } = renderClickUpHook();

    await waitFor(() => expect(result.current.connected).toBe(true));
    await waitFor(() =>
      expect(result.current.workspaces).toEqual([
        { id: "team-1", name: "Acme", color: "#ff6b35" },
      ]),
    );

    expect(clickupApi.listWorkspaces).toHaveBeenCalledTimes(1);
  });

  it("caches saved settings and invalidates unified ticketing providers", async () => {
    const pending = settings({
      enabled: false,
      validationStatus: "pending",
      taskSearchAvailable: false,
    });
    vi.mocked(clickupApi.getSettings).mockResolvedValue(pending);
    vi.mocked(clickupApi.saveSettings).mockResolvedValue(pending);

    const { queryClient, result } = renderClickUpHook();
    const invalidateSpy = vi.spyOn(queryClient, "invalidateQueries");

    await waitFor(() => expect(result.current.isLoading).toBe(false));
    await act(async () => {
      await result.current.saveSettingsAsync({ apiToken: "pk_clickup" });
    });

    expect(clickupApi.saveSettings).toHaveBeenCalledWith({ apiToken: "pk_clickup" });
    expect(queryClient.getQueryData(clickupIntegrationKeys.settings())).toEqual(pending);
    expect(invalidateSpy).toHaveBeenCalledWith({ queryKey: ticketingKeys.all });
  });

  it("validation updates settings and refreshes workspace data", async () => {
    const connected = settings({ workspaceId: null });
    vi.mocked(clickupApi.getSettings).mockResolvedValue(
      settings({ enabled: false, validationStatus: "pending", taskSearchAvailable: false }),
    );
    vi.mocked(clickupApi.validate).mockResolvedValue(connected);

    const { queryClient, result } = renderClickUpHook();
    const invalidateSpy = vi.spyOn(queryClient, "invalidateQueries");

    await waitFor(() => expect(result.current.isLoading).toBe(false));
    await act(async () => {
      await result.current.validateAsync();
    });

    expect(queryClient.getQueryData(clickupIntegrationKeys.settings())).toEqual(connected);
    expect(invalidateSpy).toHaveBeenCalledWith({
      queryKey: clickupIntegrationKeys.workspaces(),
    });
  });

  it("disconnect clears cached workspaces and invalidates ticketing providers", async () => {
    const disconnected = settings({
      enabled: false,
      hasApiToken: false,
      workspaceId: null,
      validationStatus: "not_configured",
      taskSearchAvailable: false,
    });
    vi.mocked(clickupApi.getSettings).mockResolvedValue(settings());
    vi.mocked(clickupApi.listWorkspaces).mockResolvedValue([
      { id: "team-1", name: "Acme", color: null },
    ]);
    vi.mocked(clickupApi.disconnect).mockResolvedValue(disconnected);

    const { queryClient, result } = renderClickUpHook();
    const invalidateSpy = vi.spyOn(queryClient, "invalidateQueries");

    await waitFor(() => expect(result.current.workspaces).toHaveLength(1));
    await act(async () => {
      await result.current.disconnectAsync();
    });

    expect(queryClient.getQueryData(clickupIntegrationKeys.settings())).toEqual(disconnected);
    expect(queryClient.getQueryData(clickupIntegrationKeys.workspaces())).toBeUndefined();
    expect(invalidateSpy).toHaveBeenCalledWith({ queryKey: ticketingKeys.all });
  });
});

describe("isClickUpConnected", () => {
  it("is true only when enabled, token stored, valid, and task search available", () => {
    expect(isClickUpConnected(settings())).toBe(true);
  });

  it("is false when any gate is unmet", () => {
    expect(isClickUpConnected(undefined)).toBe(false);
    expect(isClickUpConnected(settings({ enabled: false }))).toBe(false);
    expect(isClickUpConnected(settings({ hasApiToken: false }))).toBe(false);
    expect(isClickUpConnected(settings({ validationStatus: "invalid" }))).toBe(
      false,
    );
    expect(isClickUpConnected(settings({ taskSearchAvailable: false }))).toBe(
      false,
    );
  });
});

describe("clickupIntegrationKeys", () => {
  it("namespaces settings and workspaces query keys", () => {
    expect(clickupIntegrationKeys.settings()).toEqual([
      "clickup-integration",
      "settings",
    ]);
    expect(clickupIntegrationKeys.workspaces()).toEqual([
      "clickup-integration",
      "workspaces",
    ]);
  });
});
