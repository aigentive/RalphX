import { act, renderHook, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { createElement, type ReactNode } from "react";

import type { GranolaIntegrationSettings } from "@/api/granola";
import { granolaApi } from "@/api/granola";

import {
  granolaIntegrationKeys,
  isGranolaConnected,
  useGranolaIntegration,
} from "./useGranolaIntegration";

vi.mock("@/api/granola", () => ({
  granolaApi: {
    getSettings: vi.fn(),
    saveSettings: vi.fn(),
    validate: vi.fn(),
    disconnect: vi.fn(),
  },
}));

function settings(
  overrides: Partial<GranolaIntegrationSettings> = {},
): GranolaIntegrationSettings {
  return {
    enabled: true,
    hasApiToken: true,
    validationStatus: "valid",
    lastValidatedAt: null,
    lastError: null,
    updatedAt: new Date(0).toISOString(),
    ...overrides,
  };
}

function renderGranolaHook() {
  const queryClient = new QueryClient({
    defaultOptions: {
      queries: { retry: false },
      mutations: { retry: false },
    },
  });
  const wrapper = ({ children }: { children: ReactNode }) =>
    createElement(QueryClientProvider, { client: queryClient }, children);
  return {
    queryClient,
    ...renderHook(() => useGranolaIntegration(), { wrapper }),
  };
}

describe("useGranolaIntegration", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("loads settings and exposes the connected gate", async () => {
    vi.mocked(granolaApi.getSettings).mockResolvedValue(settings());

    const { result } = renderGranolaHook();

    await waitFor(() => expect(result.current.isLoading).toBe(false));

    expect(result.current.connected).toBe(true);
    expect(result.current.settings).toEqual(settings());
  });

  it("caches saved and validated settings", async () => {
    const pending = settings({
      enabled: false,
      validationStatus: "pending",
    });
    const connected = settings();
    vi.mocked(granolaApi.getSettings).mockResolvedValue(pending);
    vi.mocked(granolaApi.saveSettings).mockResolvedValue(pending);
    vi.mocked(granolaApi.validate).mockResolvedValue(connected);

    const { queryClient, result } = renderGranolaHook();

    await waitFor(() => expect(result.current.isLoading).toBe(false));
    await act(async () => {
      await result.current.saveSettingsAsync({ apiToken: "granola-token" });
    });
    expect(queryClient.getQueryData(granolaIntegrationKeys.settings())).toEqual(
      pending,
    );

    await act(async () => {
      await result.current.validateAsync();
    });
    expect(queryClient.getQueryData(granolaIntegrationKeys.settings())).toEqual(
      connected,
    );
  });

  it("disconnect updates the cached settings", async () => {
    const disconnected = settings({
      enabled: false,
      hasApiToken: false,
      validationStatus: "not_configured",
    });
    vi.mocked(granolaApi.getSettings).mockResolvedValue(settings());
    vi.mocked(granolaApi.disconnect).mockResolvedValue(disconnected);

    const { queryClient, result } = renderGranolaHook();

    await waitFor(() => expect(result.current.connected).toBe(true));
    await act(async () => {
      await result.current.disconnectAsync();
    });

    expect(queryClient.getQueryData(granolaIntegrationKeys.settings())).toEqual(
      disconnected,
    );
  });
});

describe("isGranolaConnected", () => {
  it("is true only when enabled, token stored, and valid", () => {
    expect(isGranolaConnected(settings())).toBe(true);
    expect(isGranolaConnected(undefined)).toBe(false);
    expect(isGranolaConnected(settings({ enabled: false }))).toBe(false);
    expect(isGranolaConnected(settings({ hasApiToken: false }))).toBe(false);
    expect(isGranolaConnected(settings({ validationStatus: "invalid" }))).toBe(
      false,
    );
  });
});
