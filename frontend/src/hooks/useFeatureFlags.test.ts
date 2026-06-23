/**
 * Tests for useFeatureFlags hook and isViewEnabled helper
 */

import { describe, it, expect, vi, beforeEach } from "vitest";
import { renderHook, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { createElement } from "react";
import {
  FEATURE_FLAGS_QUERY_KEY,
  TICKETING_DASHBOARD_OVERRIDE_KEY,
  applyFeatureFlagOverrides,
  getTicketingDashboardFeatureFlagOverride,
  isViewEnabled,
  setTicketingDashboardFeatureFlagOverride,
  useFeatureFlags,
} from "./useFeatureFlags";
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
// getTicketingDashboardFeatureFlagOverride (localStorage reader)
// ============================================================================

describe("getTicketingDashboardFeatureFlagOverride", () => {
  beforeEach(() => {
    localStorage.clear();
  });

  it("returns null when the localStorage key is not set", () => {
    expect(getTicketingDashboardFeatureFlagOverride()).toBeNull();
  });

  it("returns true when localStorage value is the string 'true'", () => {
    localStorage.setItem(TICKETING_DASHBOARD_OVERRIDE_KEY, "true");
    expect(getTicketingDashboardFeatureFlagOverride()).toBe(true);
  });

  it("returns false when localStorage value is the string 'false'", () => {
    localStorage.setItem(TICKETING_DASHBOARD_OVERRIDE_KEY, "false");
    expect(getTicketingDashboardFeatureFlagOverride()).toBe(false);
  });

  it("returns null for invalid stored values (non 'true'/'false' strings)", () => {
    localStorage.setItem(TICKETING_DASHBOARD_OVERRIDE_KEY, "yes");
    expect(getTicketingDashboardFeatureFlagOverride()).toBeNull();

    localStorage.setItem(TICKETING_DASHBOARD_OVERRIDE_KEY, "1");
    expect(getTicketingDashboardFeatureFlagOverride()).toBeNull();

    localStorage.setItem(TICKETING_DASHBOARD_OVERRIDE_KEY, "");
    expect(getTicketingDashboardFeatureFlagOverride()).toBeNull();
  });
});

// ============================================================================
// applyFeatureFlagOverrides (pure overlay)
// ============================================================================

describe("applyFeatureFlagOverrides", () => {
  const baseFlags: FeatureFlags = {
    activityPage: true,
    extensibilityPage: true,
    battleMode: true,
    teamMode: false,
    atlassianOauth: false,
    ticketingDashboard: true,
  };

  beforeEach(() => {
    localStorage.clear();
  });

  it("returns flags unchanged when no override is set (override null)", () => {
    expect(applyFeatureFlagOverrides(baseFlags)).toEqual(baseFlags);
  });

  it("sets ticketingDashboard=true when override is true", () => {
    setTicketingDashboardFeatureFlagOverride(true);
    expect(
      applyFeatureFlagOverrides({ ...baseFlags, ticketingDashboard: false }),
    ).toEqual({ ...baseFlags, ticketingDashboard: true });
  });

  it("sets ticketingDashboard=false when override is false", () => {
    setTicketingDashboardFeatureFlagOverride(false);
    expect(
      applyFeatureFlagOverrides({ ...baseFlags, ticketingDashboard: true }),
    ).toEqual({ ...baseFlags, ticketingDashboard: false });
  });
});

// ============================================================================
// setTicketingDashboardFeatureFlagOverride (localStorage writer)
// ============================================================================

describe("setTicketingDashboardFeatureFlagOverride", () => {
  beforeEach(() => {
    localStorage.clear();
  });

  it("writes 'true' to localStorage when passed true", () => {
    setTicketingDashboardFeatureFlagOverride(true);
    expect(localStorage.getItem(TICKETING_DASHBOARD_OVERRIDE_KEY)).toBe("true");
  });

  it("writes 'false' to localStorage when passed false", () => {
    setTicketingDashboardFeatureFlagOverride(false);
    expect(localStorage.getItem(TICKETING_DASHBOARD_OVERRIDE_KEY)).toBe("false");
  });
});

// ============================================================================
// useFeatureFlags hook
// ============================================================================

describe("useFeatureFlags", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    localStorage.clear();
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

  it("overlays the persisted ticketing dashboard override on backend flags", async () => {
    setTicketingDashboardFeatureFlagOverride(true);
    vi.mocked(invoke).mockResolvedValueOnce({
      activityPage: true,
      extensibilityPage: true,
      ticketingDashboard: false,
    });

    const { result } = renderHook(() => useFeatureFlags(), {
      wrapper: createWrapper(),
    });

    await waitFor(() => expect(result.current.isPlaceholderData).toBe(false));

    expect(result.current.data.ticketingDashboard).toBe(true);
    expect(localStorage.getItem(TICKETING_DASHBOARD_OVERRIDE_KEY)).toBe("true");
  });

  it("applies ticketing dashboard overrides without changing other flags", () => {
    setTicketingDashboardFeatureFlagOverride(false);

    expect(
      applyFeatureFlagOverrides({
        activityPage: false,
        extensibilityPage: true,
        battleMode: true,
        teamMode: false,
        atlassianOauth: false,
        ticketingDashboard: true,
      }),
    ).toEqual({
      activityPage: false,
      extensibilityPage: true,
      battleMode: true,
      teamMode: false,
      atlassianOauth: false,
      ticketingDashboard: false,
    });
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
