/**
 * Tests for useFeatureFlags hook and isViewEnabled helper
 */

import { describe, it, expect, vi, beforeEach } from "vitest";
import { renderHook, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { createElement } from "react";
import { useFeatureFlags, isViewEnabled, FEATURE_FLAGS_QUERY_KEY } from "./useFeatureFlags";
import { invoke } from "@tauri-apps/api/core";
import type { FeatureFlags } from "@/types/feature-flags";

function createWrapper() {
  const queryClient = new QueryClient({
    defaultOptions: {
      queries: { retry: false, gcTime: 0 },
    },
  });
  return function Wrapper({ children }: { children: React.ReactNode }) {
    return createElement(QueryClientProvider, { client: queryClient }, children);
  };
}

// ============================================================================
// isViewEnabled (pure helper — no React needed)
// ============================================================================

describe("isViewEnabled", () => {
  const allEnabled: FeatureFlags = { activityPage: true, extensibilityPage: true, battleMode: true, teamMode: false, atlassianOauth: false, ticketingDashboard: true };
  const activityDisabled: FeatureFlags = { activityPage: false, extensibilityPage: true, battleMode: true, teamMode: false, atlassianOauth: false, ticketingDashboard: true };
  const extensibilityDisabled: FeatureFlags = { activityPage: true, extensibilityPage: false, battleMode: true, teamMode: false, atlassianOauth: false, ticketingDashboard: true };
  const allDisabled: FeatureFlags = { activityPage: false, extensibilityPage: false, battleMode: true, teamMode: false, atlassianOauth: false, ticketingDashboard: false };

  it("returns true for kanban regardless of flags", () => {
    expect(isViewEnabled("kanban", allDisabled)).toBe(true);
  });

  it("returns true for ideation regardless of flags", () => {
    expect(isViewEnabled("ideation", allDisabled)).toBe(true);
  });

  it("returns true for graph regardless of flags", () => {
    expect(isViewEnabled("graph", allDisabled)).toBe(true);
  });

  it("returns true for settings regardless of flags", () => {
    expect(isViewEnabled("settings", allDisabled)).toBe(true);
  });

  it("returns flags.activityPage for activity view", () => {
    expect(isViewEnabled("activity", allEnabled)).toBe(true);
    expect(isViewEnabled("activity", activityDisabled)).toBe(false);
  });

  it("returns flags.extensibilityPage for extensibility view", () => {
    expect(isViewEnabled("extensibility", allEnabled)).toBe(true);
    expect(isViewEnabled("extensibility", extensibilityDisabled)).toBe(false);
  });

  it("returns flags.ticketingDashboard for ticketing view", () => {
    expect(isViewEnabled("ticketing", allEnabled)).toBe(true);
    expect(isViewEnabled("ticketing", allDisabled)).toBe(false);
  });

  it("returns true for unknown views (safe default)", () => {
    expect(isViewEnabled("unknown-view", allDisabled)).toBe(true);
  });
});

// ============================================================================
// useFeatureFlags hook
// ============================================================================

describe("useFeatureFlags", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("returns placeholder data (all enabled) before query resolves", () => {
    // Don't resolve invoke — hook should show placeholderData
    vi.mocked(invoke).mockReturnValue(new Promise(() => {}));

    const { result } = renderHook(() => useFeatureFlags(), {
      wrapper: createWrapper(),
    });

    // placeholderData is available synchronously
    expect(result.current.data).toEqual({
      activityPage: true,
      extensibilityPage: true,
      battleMode: true,
      teamMode: false,
      atlassianOauth: false,
      ticketingDashboard: false,
    });
  });

  it("returns backend data when query resolves", async () => {
    const flagsFromBackend = { activityPage: false, extensibilityPage: true };
    vi.mocked(invoke).mockResolvedValueOnce(flagsFromBackend);

    const { result } = renderHook(() => useFeatureFlags(), {
      wrapper: createWrapper(),
    });

    await waitFor(() => expect(result.current.isPlaceholderData).toBe(false));

    expect(result.current.data).toEqual({
      activityPage: false,
      extensibilityPage: true,
      battleMode: true,
      teamMode: false,
      atlassianOauth: false,
      ticketingDashboard: false,
    });
    expect(invoke).toHaveBeenCalledWith("get_ui_feature_flags");
  });

  it("uses correct query key", () => {
    expect(FEATURE_FLAGS_QUERY_KEY).toEqual(["featureFlags"]);
  });

  it("shows placeholder data (all enabled) when invoke fails (retry: false)", async () => {
    vi.mocked(invoke).mockRejectedValueOnce(new Error("Backend unavailable"));

    const { result } = renderHook(() => useFeatureFlags(), {
      wrapper: createWrapper(),
    });

    // Wait for the query to settle
    await waitFor(() => expect(result.current.isFetching).toBe(false));

    // placeholderData shown when error — pages remain visible (safe fallback)
    expect(result.current.data).toEqual({
      activityPage: true,
      extensibilityPage: true,
      battleMode: true,
      teamMode: false,
      atlassianOauth: false,
      ticketingDashboard: false,
    });
  });
});
