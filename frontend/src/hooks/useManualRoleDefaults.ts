import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";

import { manualRoleDefaultsApi } from "@/api/manual-role-defaults";
import type { ManualRoleDefault } from "@/api/manual-role-defaults.types";

export const manualRoleDefaultKeys = {
  all: ["agent", "manual-role-defaults"] as const,
  scope: (projectId: string | null) =>
    [...manualRoleDefaultKeys.all, projectId] as const,
  startComposer: (projectId: string, mode: string) =>
    [...manualRoleDefaultKeys.all, "start-composer", projectId, mode] as const,
  conversation: (conversationId: string) =>
    [...manualRoleDefaultKeys.all, "conversation", conversationId] as const,
};

export function useStartComposerRoleDefault(projectId: string, mode: string) {
  return useQuery({
    queryKey: manualRoleDefaultKeys.startComposer(projectId, mode),
    queryFn: () =>
      manualRoleDefaultsApi.getStartComposerDefault({ projectId, mode }),
    enabled: Boolean(projectId && mode),
    staleTime: 30_000,
  });
}

export function useConversationRoleDefault(conversationId: string | null) {
  return useQuery({
    queryKey: manualRoleDefaultKeys.conversation(conversationId ?? ""),
    queryFn: () =>
      manualRoleDefaultsApi.getConversationDefault({
        conversationId: conversationId ?? "",
      }),
    enabled: Boolean(conversationId),
    staleTime: 30_000,
  });
}

export function useManualRoleDefaults(projectId: string | null) {
  const queryClient = useQueryClient();
  const queryKey = manualRoleDefaultKeys.scope(projectId);
  const query = useQuery({
    queryKey,
    queryFn: () => manualRoleDefaultsApi.list(projectId),
    staleTime: 30_000,
  });

  const updateMutation = useMutation({
    mutationFn: ({ role, value }: { role: string; value: ManualRoleDefault }) =>
      manualRoleDefaultsApi.update({ projectId, role, value }),
    onSuccess: async () => {
      await queryClient.invalidateQueries({
        queryKey: manualRoleDefaultKeys.all,
      });
    },
  });
  const clearMutation = useMutation({
    mutationFn: (role: string) =>
      manualRoleDefaultsApi.clear({ projectId, role }),
    onSuccess: async () => {
      await queryClient.invalidateQueries({
        queryKey: manualRoleDefaultKeys.all,
      });
    },
  });

  return {
    catalog: query.data ?? null,
    isLoading: query.isLoading,
    isError: query.isError,
    error: query.error,
    isSaving: updateMutation.isPending || clearMutation.isPending,
    updateDefault: (role: string, value: ManualRoleDefault) =>
      updateMutation.mutate({ role, value }),
    clearDefault: clearMutation.mutate,
  };
}
