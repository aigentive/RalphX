/**
 * useFeatureFlags - TanStack Query hook for UI feature flags
 *
 * Fetches flags once at startup (staleTime: Infinity).
 * Uses placeholderData to avoid startup flash before the Tauri command responds.
 * Defaults mirror the app's current flag baseline.
 */

import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { invoke } from "@tauri-apps/api/core";
import { applyFeatureFlagOverrides } from "@/lib/featureFlags";
import { useUiStore } from "@/stores/uiStore";
import { featureFlagsSchema } from "@/types/feature-flags";
import type { FeatureFlags } from "@/types/feature-flags";

export { applyFeatureFlagOverrides, isViewEnabled } from "@/lib/featureFlags";

export const FEATURE_FLAGS_QUERY_KEY = ["featureFlags"] as const;

const DEFAULT_FEATURE_FLAGS: FeatureFlags = {
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
};

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

export function useUpdateFeatureFlags() {
  const queryClient = useQueryClient();

  return useMutation({
    mutationFn: async ({
      agentPersonas,
      agentConversationTeam,
      agentConversationWorkflows,
      agentConversationAutopilot,
    }: {
      agentPersonas?: boolean;
      agentConversationTeam?: boolean;
      agentConversationWorkflows?: boolean;
      agentConversationAutopilot?: boolean;
    }) => {
      const raw = await invoke("update_ui_feature_flags", {
        input: {
          ...(agentPersonas !== undefined && { agentPersonas }),
          ...(agentConversationTeam !== undefined && { agentConversationTeam }),
          ...(agentConversationWorkflows !== undefined && {
            agentConversationWorkflows,
          }),
          ...(agentConversationAutopilot !== undefined && {
            agentConversationAutopilot,
          }),
        },
      });
      return applyFeatureFlagOverrides(featureFlagsSchema.parse(raw));
    },
    onSuccess: (flags) => {
      queryClient.setQueryData(FEATURE_FLAGS_QUERY_KEY, flags);
      useUiStore.getState().setFeatureFlags(flags);
    },
  });
}
