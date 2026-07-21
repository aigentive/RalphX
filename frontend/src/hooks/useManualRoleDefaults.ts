import { useEffect } from "react";
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

export function useStartComposerRoleDefault(
  projectId: string | null,
  mode: string,
) {
  return useQuery({
    queryKey: manualRoleDefaultKeys.startComposer(projectId ?? "", mode),
    queryFn: () =>
      manualRoleDefaultsApi.getStartComposerDefault({
        projectId: projectId ?? "",
        mode,
      }),
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
    placeholderData: () => undefined,
    staleTime: 30_000,
  });

  const updateMutation = useMutation({
    mutationFn: ({
      projectId: mutationProjectId,
      role,
      value,
    }: {
      projectId: string | null;
      role: string;
      value: ManualRoleDefault;
    }) => manualRoleDefaultsApi.update({
      projectId: mutationProjectId,
      role,
      value,
    }),
    onSuccess: async () => {
      await queryClient.invalidateQueries({
        queryKey: manualRoleDefaultKeys.all,
      });
    },
  });
  const clearMutation = useMutation({
    mutationFn: ({
      projectId: mutationProjectId,
      role,
    }: {
      projectId: string | null;
      role: string;
    }) => manualRoleDefaultsApi.clear({ projectId: mutationProjectId, role }),
    onSuccess: async () => {
      await queryClient.invalidateQueries({
        queryKey: manualRoleDefaultKeys.all,
      });
    },
  });
  const resetUpdate = updateMutation.reset;
  const resetClear = clearMutation.reset;

  useEffect(() => {
    resetUpdate();
    resetClear();
  }, [projectId, resetClear, resetUpdate]);

  const updateMatchesScope = updateMutation.variables?.projectId === projectId;
  const clearMatchesScope = clearMutation.variables?.projectId === projectId;

  return {
    catalog: query.data ?? null,
    isLoading: query.isLoading,
    isError: query.isError,
    error: query.error,
    saveError:
      (updateMatchesScope ? updateMutation.error : null) ??
      (clearMatchesScope ? clearMutation.error : null) ??
      null,
    isSaving:
      (updateMatchesScope && updateMutation.isPending) ||
      (clearMatchesScope && clearMutation.isPending),
    updateDefault: (role: string, value: ManualRoleDefault) => {
      updateMutation.reset();
      clearMutation.reset();
      updateMutation.mutate({ projectId, role, value });
    },
    clearDefaultAsync: (role: string) => {
      updateMutation.reset();
      clearMutation.reset();
      return clearMutation.mutateAsync({ projectId, role });
    },
    dismissSaveError: () => {
      updateMutation.reset();
      clearMutation.reset();
    },
  };
}
