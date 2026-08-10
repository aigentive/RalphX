/**
 * Tests for useFeatureFlags hook and isViewEnabled helper
 */

import { beforeEach, describe, expect, it, vi } from "vitest";
import { renderHook, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { createElement } from "react";
import {
  FEATURE_FLAGS_QUERY_KEY,
  applyFeatureFlagOverrides,
  isViewEnabled,
  useFeatureFlags,
  useUpdateFeatureFlags,
} from "./useFeatureFlags";
import { invoke } from "@tauri-apps/api/core";
import { featureFlagsSchema, type FeatureFlags } from "@/types/feature-flags";

function createWrapper() {
  const queryClient = new QueryClient({
    defaultOptions: {
      queries: { retry: false, gcTime: 0 },
    },
  });
  return function Wrapper({ children }: { children: React.ReactNode }) {
    return createElement(
      QueryClientProvider,
      { client: queryClient },
      children,
    );
  };
}

// ============================================================================
// isViewEnabled (pure helper — no React needed)
// ============================================================================

describe("isViewEnabled", () => {
  const allEnabled: FeatureFlags = {
    activityPage: true,
    extensibilityPage: true,
    automationsPage: true,
    atlassianOauth: false,
    ticketingDashboard: true,
    agentPersonas: false,
    agentConversationTeam: false,
    agentConversationWorkflows: false,
    standaloneConversations: false,
    agentConversationAutopilot: false,
  };
  const activityDisabled: FeatureFlags = {
    activityPage: false,
    extensibilityPage: true,
    automationsPage: true,
    atlassianOauth: false,
    ticketingDashboard: true,
    agentPersonas: false,
    agentConversationTeam: false,
    agentConversationWorkflows: false,
    standaloneConversations: false,
    agentConversationAutopilot: false,
  };
  const extensibilityDisabled: FeatureFlags = {
    activityPage: true,
    extensibilityPage: false,
    automationsPage: true,
    atlassianOauth: false,
    ticketingDashboard: true,
    agentPersonas: false,
    agentConversationTeam: false,
    agentConversationWorkflows: false,
    standaloneConversations: false,
    agentConversationAutopilot: false,
  };
  const allDisabled: FeatureFlags = {
    activityPage: false,
    extensibilityPage: false,
    automationsPage: false,
    atlassianOauth: false,
    ticketingDashboard: false,
    agentPersonas: false,
    agentConversationTeam: false,
    agentConversationWorkflows: false,
    standaloneConversations: false,
    agentConversationAutopilot: false,
  };

  it.each(["agents", "github", "granola", "insights"] as const)(
    "returns true for the always-enabled %s root",
    (view) => {
      expect(isViewEnabled(view, allDisabled)).toBe(true);
    },
  );

  it("returns flags.automationsPage for automations view", () => {
    expect(isViewEnabled("automations", allEnabled)).toBe(true);
    expect(isViewEnabled("automations", allDisabled)).toBe(false);
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

});
// ============================================================================
// applyFeatureFlagOverrides (compatibility identity)
// ============================================================================

describe("applyFeatureFlagOverrides", () => {
  const baseFlags: FeatureFlags = {
    activityPage: true,
    extensibilityPage: true,
    automationsPage: false,
    atlassianOauth: false,
    ticketingDashboard: true,
    agentPersonas: false,
    agentConversationTeam: false,
    agentConversationWorkflows: false,
    standaloneConversations: true,
    agentConversationAutopilot: false,
  };

  it("returns the same flags object unchanged", () => {
    expect(applyFeatureFlagOverrides(baseFlags)).toBe(baseFlags);
  });
});

describe("featureFlagsSchema", () => {
  it("defaults live optional flags and omits retired keys", () => {
    const flags = featureFlagsSchema.parse({
      activityPage: true,
      extensibilityPage: true,
    });

    expect(flags).toEqual({
      activityPage: true,
      extensibilityPage: true,
      automationsPage: true,
      atlassianOauth: false,
      ticketingDashboard: false,
      agentPersonas: false,
      agentConversationTeam: false,
      agentConversationWorkflows: false,
      standaloneConversations: false,
      agentConversationAutopilot: false,
      // Defaults ON since 2026-08-03 (owner decision) — matches the backend default.
      remoteEnvironments: true,
    });
    expect("ideationPage" in flags).toBe(false);
    expect("battleMode" in flags).toBe(false);
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

  it("returns placeholder data while the query is loading", () => {
    // Don't resolve invoke — hook should show placeholderData
    vi.mocked(invoke).mockReturnValue(new Promise(() => {}));

    const { result } = renderHook(() => useFeatureFlags(), {
      wrapper: createWrapper(),
    });

    // placeholderData is available synchronously
    expect(result.current.isPlaceholderData).toBe(true);
    expect(result.current.data).toEqual({
      activityPage: true,
      extensibilityPage: true,
      automationsPage: true,
      atlassianOauth: false,
      ticketingDashboard: false,
      agentPersonas: false,
      agentConversationTeam: false,
      agentConversationWorkflows: false,
      standaloneConversations: false,
      agentConversationAutopilot: false,
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
      automationsPage: true,
      atlassianOauth: false,
      ticketingDashboard: false,
      agentPersonas: false,
      agentConversationTeam: false,
      agentConversationWorkflows: false,
      standaloneConversations: false,
      agentConversationAutopilot: false,
    });
    expect(invoke).toHaveBeenCalledWith("get_ui_feature_flags");
  });

  it("uses correct query key", () => {
    expect(FEATURE_FLAGS_QUERY_KEY).toEqual(["featureFlags"]);
  });

  it("falls back to defaults when invoke fails", async () => {
    vi.mocked(invoke).mockRejectedValueOnce(new Error("Backend unavailable"));

    const { result } = renderHook(() => useFeatureFlags(), {
      wrapper: createWrapper(),
    });

    // Wait for the query to settle
    await waitFor(() => expect(result.current.isFetching).toBe(false));

    expect(result.current.isError).toBe(true);
    expect(result.current.data).toEqual({
      activityPage: true,
      extensibilityPage: true,
      automationsPage: true,
      atlassianOauth: false,
      ticketingDashboard: false,
      agentPersonas: false,
      agentConversationTeam: false,
      agentConversationWorkflows: false,
      standaloneConversations: false,
      agentConversationAutopilot: false,
    });
  });
});

describe("useUpdateFeatureFlags", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("updates Team independently without writing Workflows", async () => {
    vi.mocked(invoke).mockResolvedValueOnce({
      activityPage: true,
      extensibilityPage: true,
      agentConversationTeam: true,
      agentConversationWorkflows: false,
    });

    const { result } = renderHook(() => useUpdateFeatureFlags(), {
      wrapper: createWrapper(),
    });
    result.current.mutate({ agentConversationTeam: true });

    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    expect(invoke).toHaveBeenCalledWith("update_ui_feature_flags", {
      input: { agentConversationTeam: true },
    });
  });

  it("updates Autopilot independently", async () => {
    vi.mocked(invoke).mockResolvedValueOnce({
      activityPage: true,
      extensibilityPage: true,
      agentConversationAutopilot: true,
    });

    const { result } = renderHook(() => useUpdateFeatureFlags(), {
      wrapper: createWrapper(),
    });
    result.current.mutate({ agentConversationAutopilot: true });

    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    expect(invoke).toHaveBeenCalledWith("update_ui_feature_flags", {
      input: { agentConversationAutopilot: true },
    });
  });
});
