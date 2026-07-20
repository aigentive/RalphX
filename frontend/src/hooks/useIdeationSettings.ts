/**
 * useIdeationSettings hook - TanStack Query integration for ideation settings
 *
 * Provides:
 * - Loading state
 * - Error handling
 * - Optimistic updates
 * - Auto-save functionality
 */

import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { ideationApi } from "@/api/ideation";
import type { IdeationSettings } from "@/types/ideation-config";
import { defaultIdeationSettings } from "@/types/ideation-config";

const IDEATION_SETTINGS_KEY = ["ideation", "settings"];
/**
 * Hook to fetch and update ideation settings
 */
export function useIdeationSettings(enabled = true) {
  const queryClient = useQueryClient();

  // Fetch settings
  const query = useQuery({
    queryKey: IDEATION_SETTINGS_KEY,
    queryFn: () => ideationApi.settings.get(),
    enabled,
    staleTime: 1000 * 60 * 5, // 5 minutes
    gcTime: 1000 * 60 * 10, // 10 minutes (formerly cacheTime)
    placeholderData: defaultIdeationSettings,
  });

  // Update settings mutation
  const mutation = useMutation({
    mutationFn: (settings: IdeationSettings) => ideationApi.settings.update(settings),
    onMutate: async (newSettings) => {
      // Cancel outgoing refetches
      await queryClient.cancelQueries({ queryKey: IDEATION_SETTINGS_KEY });

      // Snapshot previous value
      const previousSettings = queryClient.getQueryData<IdeationSettings>(IDEATION_SETTINGS_KEY);

      // Tasks state is backend-owned by the dedicated drain-aware mutation.
      const currentTasksSettings = previousSettings ?? defaultIdeationSettings;
      const optimisticSettings = {
        ...newSettings,
        tasksEnabled: currentTasksSettings.tasksEnabled,
        tasksFeatureState: currentTasksSettings.tasksFeatureState,
      };
      queryClient.setQueryData(IDEATION_SETTINGS_KEY, optimisticSettings);

      return { previousSettings };
    },
    onError: (_err, _newSettings, context) => {
      // Rollback on error
      if (context?.previousSettings) {
        queryClient.setQueryData(IDEATION_SETTINGS_KEY, context.previousSettings);
      }
    },
    onSuccess: (updatedSettings) => {
      // Update cache with server response
      queryClient.setQueryData(IDEATION_SETTINGS_KEY, updatedSettings);
    },
  });

  const tasksFeatureMutation = useMutation({
    mutationFn: (tasksEnabled: boolean) =>
      ideationApi.settings.setTasksEnabled(tasksEnabled),
    onSuccess: (updatedSettings) => {
      queryClient.setQueryData(IDEATION_SETTINGS_KEY, updatedSettings);
    },
    onError: () => {
      void queryClient.invalidateQueries({ queryKey: IDEATION_SETTINGS_KEY });
    },
  });

  return {
    settings: query.data ?? defaultIdeationSettings,
    isLoading: query.isLoading,
    isError: query.isError,
    error: query.error,
    updateSettings: mutation.mutate,
    fetchTasksDisableImpact: ideationApi.settings.getDisableImpact,
    setTasksEnabled: tasksFeatureMutation.mutateAsync,
    isUpdating: mutation.isPending || tasksFeatureMutation.isPending,
    updateError: tasksFeatureMutation.error ?? mutation.error,
  };
}
