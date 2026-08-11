import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";

import {
  workspaceReviewSettingsApi,
  type UpdateWorkspaceReviewRuntimeSettingsInput,
  type WorkspaceReviewRuntimeSettingsResponse,
} from "@/api/workspace-review-settings";
import { manualRoleDefaultKeys } from "@/hooks/useManualRoleDefaults";

export const workspaceReviewSettingsKeys = {
  all: ["workspace-review", "runtime-settings"] as const,
  list: (projectId: string | null) =>
    ["workspace-review", "runtime-settings", projectId] as const,
};

const EMPTY_ROWS: WorkspaceReviewRuntimeSettingsResponse[] = [];

export function useWorkspaceReviewRuntimeSettings(projectId: string | null) {
  const queryClient = useQueryClient();
  const queryKey = workspaceReviewSettingsKeys.list(projectId);

  const query = useQuery({
    queryKey,
    queryFn: () => workspaceReviewSettingsApi.list(projectId),
    staleTime: 1000 * 60 * 5,
    gcTime: 1000 * 60 * 10,
    placeholderData: EMPTY_ROWS,
  });

  const mutation = useMutation({
    mutationFn: (
      input: Omit<UpdateWorkspaceReviewRuntimeSettingsInput, "projectId">,
    ) => workspaceReviewSettingsApi.update({ projectId, ...input }),
    onSuccess: async () => {
      await Promise.all([
        queryClient.invalidateQueries({
          queryKey: workspaceReviewSettingsKeys.all,
        }),
        queryClient.invalidateQueries({
          queryKey: manualRoleDefaultKeys.all,
        }),
      ]);
    },
  });

  return {
    rows: query.data ?? EMPTY_ROWS,
    isLoading: query.isLoading,
    isPlaceholderData: query.isPlaceholderData,
    isError: query.isError,
    error: query.error,
    updateSettings: mutation.mutate,
    isUpdating: mutation.isPending,
    saveError: mutation.error,
  };
}
