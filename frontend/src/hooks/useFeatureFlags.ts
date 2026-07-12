/**
 * useFeatureFlags - TanStack Query hook for UI feature flags
 *
 * Fetches flags once at startup (staleTime: Infinity).
 * Uses placeholderData to avoid startup flash before the Tauri command responds.
 * Defaults mirror the app's current flag baseline.
 */

import { useQuery } from "@tanstack/react-query";
import { invoke } from "@tauri-apps/api/core";
import { featureFlagsSchema } from "@/types/feature-flags";
import type { FeatureFlags } from "@/types/feature-flags";

export const FEATURE_FLAGS_QUERY_KEY = ["featureFlags"] as const;

const DEFAULT_FEATURE_FLAGS: FeatureFlags = {
  activityPage: true,
  extensibilityPage: true,
  ideationPage: false,
  automationsPage: true,
  battleMode: true,
  teamMode: false,
  atlassianOauth: false,
  ticketingDashboard: false,
  agentPersonas: false,
};

export function applyFeatureFlagOverrides(flags: FeatureFlags): FeatureFlags {
  return flags;
}

export function useFeatureFlags() {
  const query = useQuery<FeatureFlags>({
    queryKey: FEATURE_FLAGS_QUERY_KEY,
    queryFn: async () => {
      const raw = await invoke("get_ui_feature_flags");
      return applyFeatureFlagOverrides(featureFlagsSchema.parse(raw));
    },
    staleTime: Infinity,
    // placeholderData shows defaults immediately (prevents startup flash) while
    // the real fetch happens. Unlike initialData, it doesn't block the initial fetch.
    placeholderData: DEFAULT_FEATURE_FLAGS,
    retry: false,
  });

  return {
    ...query,
    // Always return a defined FeatureFlags. Falls back to defaults on error
    // (placeholderData is not shown in error state; query.data would be undefined).
    data: applyFeatureFlagOverrides(query.data ?? DEFAULT_FEATURE_FLAGS),
  };
}

/**
 * Pure helper to check if a view is enabled given the current flags.
 * Usable outside of React components.
 */
export function isViewEnabled(view: string, flags: FeatureFlags): boolean {
  switch (view) {
    case "activity":
      return flags.activityPage;
    case "extensibility":
      return flags.extensibilityPage;
    case "ideation":
      return flags.ideationPage;
    case "automations":
      return flags.automationsPage;
    case "ticketing":
      return true;
    default:
      return true;
  }
}
