/**
 * Tests for useFeatureFlags hook and isViewEnabled helper
 */

import { describe, it, expect, vi, beforeEach } from "vitest";
import { renderHook, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { createElement } from "react";
import {
  FEATURE_FLAGS_QUERY_KEY,
  applyFeatureFlagOverrides,
  isViewEnabled,
  useFeatureFlags,
} from "./useFeatureFlags";
import { invoke } from "@tauri-apps/api/core";
import {
  featureFlagsSchema,
  type FeatureFlags,
} from "@/types/feature-flags";

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
  const allEnabled: FeatureFlags = {
    activityPage: true,
    extensibilityPage: true,
    ideationPage: true,
    automationsPage: true,
    battleMode: true,
    teamMode: false,
    atlassianOauth: false,
    ticketingDashboard: true,
    agentPersonas: false,
  };
  const activityDisabled: FeatureFlags = {
    activityPage: false,
    extensibilityPage: true,
    ideationPage: true,
    automationsPage: true,
    battleMode: true,
    teamMode: false,
    atlassianOauth: false,
    ticketingDashboard: true,
    agentPersonas: false,
  };
  const extensibilityDisabled: FeatureFlags = {
    activityPage: true,
    extensibilityPage: false,
    ideationPage: true,
    automationsPage: true,
    battleMode: true,
    teamMode: false,
    atlassianOauth: false,
    ticketingDashboard: true,
  };
  const allDisabled: FeatureFlags = {
    activityPage: false,
    extensibilityPage: false,
    ideationPage: false,
    automationsPage: false,
    battleMode: true,
    teamMode: false,
    atlassianOauth: false,
    ticketingDashboard: false,
    agentPersonas: false,
  };

  it("returns true for kanban regardless of flags", () => {
    expect(isViewEnabled("kanban", allDisabled)).toBe(true);
  });

  it("returns flags.ideationPage for ideation view", () => {
    expect(isViewEnabled("ideation", allEnabled)).toBe(true);
    expect(isViewEnabled("ideation", allDisabled)).toBe(false);
  });

  it("returns flags.automationsPage for automations view", () => {
    expect(isViewEnabled("automations", allEnabled)).toBe(true);
    expect(isViewEnabled("automations", allDisabled)).toBe(false);
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

  it("always enables ticketing because provider validity controls access", () => {
    expect(isViewEnabled("ticketing", allEnabled)).toBe(true);
    expect(isViewEnabled("ticketing", allDisabled)).toBe(true);
  });

  it("returns true for unknown views (safe default)", () => {
    expect(isViewEnabled("unknown-view", allDisabled)).toBe(true);
  });
});

// ============================================================================
// applyFeatureFlagOverrides (compatibility identity)
// ============================================================================

describe("applyFeatureFlagOverrides", () => {
  const baseFlags: FeatureFlags = {
    activityPage: true,
    extensibilityPage: true,
    ideationPage: false,
    automationsPage: false,
    battleMode: true,
    teamMode: false,
    atlassianOauth: false,
    ticketingDashboard: true,
    agentPersonas: false,
  };

  it("returns flags unchanged", () => {
    expect(applyFeatureFlagOverrides(baseFlags)).toEqual(baseFlags);
  });
});

describe("featureFlagsSchema", () => {
  it("defaults agentPersonas to false when the backend omits it", () => {
    expect(
      featureFlagsSchema.parse({
        activityPage: true,
        extensibilityPage: true,
      }).agentPersonas,
    ).toBe(false);
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

  it("returns placeholder data before query resolves", () => {
    // Don't resolve invoke — hook should show placeholderData
    vi.mocked(invoke).mockReturnValue(new Promise(() => {}));

    const { result } = renderHook(() => useFeatureFlags(), {
      wrapper: createWrapper(),
    });

    // placeholderData is available synchronously
    expect(result.current.data).toEqual({
      activityPage: true,
      extensibilityPage: true,
      ideationPage: false,
      automationsPage: true,
      battleMode: true,
      teamMode: false,
      atlassianOauth: false,
      ticketingDashboard: false,
      agentPersonas: false,
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
      ideationPage: false,
      automationsPage: true,
      battleMode: true,
      teamMode: false,
      atlassianOauth: false,
      ticketingDashboard: false,
      agentPersonas: false,
    });
    expect(invoke).toHaveBeenCalledWith("get_ui_feature_flags");
  });

  it("uses correct query key", () => {
    expect(FEATURE_FLAGS_QUERY_KEY).toEqual(["featureFlags"]);
  });

  it("shows placeholder data when invoke fails (retry: false)", async () => {
    vi.mocked(invoke).mockRejectedValueOnce(new Error("Backend unavailable"));

    const { result } = renderHook(() => useFeatureFlags(), {
      wrapper: createWrapper(),
    });

    // Wait for the query to settle
    await waitFor(() => expect(result.current.isFetching).toBe(false));

    // placeholderData shown when error; standalone Ideation stays hidden by default.
    expect(result.current.data).toEqual({
      activityPage: true,
      extensibilityPage: true,
      ideationPage: false,
      automationsPage: true,
      battleMode: true,
      teamMode: false,
      atlassianOauth: false,
      ticketingDashboard: false,
      agentPersonas: false,
    });
  });
});
