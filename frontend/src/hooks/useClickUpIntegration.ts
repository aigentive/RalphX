import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";

import {
  clickupApi,
  type ClickUpIntegrationSettings,
  type SaveClickUpIntegrationSettingsInput,
} from "@/api/clickup";

export const clickupIntegrationKeys = {
  all: ["clickup-integration"] as const,
  settings: () => [...clickupIntegrationKeys.all, "settings"] as const,
  workspaces: () => [...clickupIntegrationKeys.all, "workspaces"] as const,
};

/**
 * A ClickUp connection is usable once the token is stored, validated, and the
 * backend reports task search is available — the same gate the backend applies
 * before it will list workspaces.
 */
export function isClickUpConnected(
  settings: ClickUpIntegrationSettings | undefined,
): boolean {
  return Boolean(
    settings?.enabled &&
      settings.hasApiToken &&
      settings.validationStatus === "valid" &&
      settings.taskSearchAvailable,
  );
}

export function useClickUpIntegration() {
  const queryClient = useQueryClient();
  const settingsQuery = useQuery({
    queryKey: clickupIntegrationKeys.settings(),
    queryFn: () => clickupApi.getSettings(),
    staleTime: 15_000,
  });

  const connected = isClickUpConnected(settingsQuery.data);

  // Workspaces require a validated connection; only fetch once connected.
  const workspacesQuery = useQuery({
    queryKey: clickupIntegrationKeys.workspaces(),
    queryFn: () => clickupApi.listWorkspaces(),
    enabled: connected,
    staleTime: 60_000,
  });

  const saveSettingsMutation = useMutation({
    mutationFn: (input: SaveClickUpIntegrationSettingsInput) =>
      clickupApi.saveSettings(input),
    onSuccess: (settings) => {
      queryClient.setQueryData(clickupIntegrationKeys.settings(), settings);
    },
  });

  const validateMutation = useMutation({
    mutationFn: () => clickupApi.validate(),
    onSuccess: (settings) => {
      queryClient.setQueryData(clickupIntegrationKeys.settings(), settings);
      void queryClient.invalidateQueries({
        queryKey: clickupIntegrationKeys.workspaces(),
      });
    },
  });

  const disconnectMutation = useMutation({
    mutationFn: () => clickupApi.disconnect(),
    onSuccess: (settings) => {
      queryClient.setQueryData(clickupIntegrationKeys.settings(), settings);
      queryClient.removeQueries({
        queryKey: clickupIntegrationKeys.workspaces(),
      });
    },
  });

  return {
    settings: settingsQuery.data,
    isLoading: settingsQuery.isLoading,
    isError: settingsQuery.isError,
    error: settingsQuery.error,
    connected,
    workspaces: workspacesQuery.data ?? [],
    isLoadingWorkspaces: workspacesQuery.isLoading,
    workspacesError: workspacesQuery.error,
    saveSettingsAsync: saveSettingsMutation.mutateAsync,
    validateAsync: validateMutation.mutateAsync,
    disconnectAsync: disconnectMutation.mutateAsync,
    isSavingSettings: saveSettingsMutation.isPending,
    isValidating: validateMutation.isPending,
    isDisconnecting: disconnectMutation.isPending,
    saveSettingsError: saveSettingsMutation.error,
    validateError: validateMutation.error,
    disconnectError: disconnectMutation.error,
  };
}
