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
const TASKS_DRAIN_INCOMPLETE_ERROR_CODE = "ralphx:tasks_drain_incomplete";

function isCommittedTasksOffError(error: unknown): boolean {
  return error instanceof Error && error.message.includes(TASKS_DRAIN_INCOMPLETE_ERROR_CODE);
}

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

      // Optimistically update
      queryClient.setQueryData(IDEATION_SETTINGS_KEY, newSettings);

      return { previousSettings };
    },
    onError: (_err, _newSettings, context) => {
      if (isCommittedTasksOffError(_err)) {
        void queryClient.invalidateQueries({ queryKey: IDEATION_SETTINGS_KEY });
        return;
      }
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

  return {
    settings: query.data ?? defaultIdeationSettings,
    isLoading: query.isLoading,
    isError: query.isError,
    error: query.error,
    updateSettings: mutation.mutate,
    isUpdating: mutation.isPending,
    updateError: mutation.error,
  };
}
