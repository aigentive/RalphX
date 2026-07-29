import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";

import {
  linearApi,
  type LinearIntegrationSettings,
  type SaveLinearIntegrationSettingsInput,
} from "@/api/linear";
import { invalidateTicketingQueries } from "@/hooks/useTicketing";

export const linearIntegrationKeys = {
  all: ["linear-integration"] as const,
  settings: () => [...linearIntegrationKeys.all, "settings"] as const,
};

/**
 * A Linear connection is usable once the token is stored, validated, and the
 * backend reports issue search is available.
 */
export function isLinearConnected(
  settings: LinearIntegrationSettings | undefined,
): boolean {
  return Boolean(
    settings?.enabled &&
      settings.hasApiToken &&
      settings.validationStatus === "valid" &&
      settings.issueSearchAvailable,
  );
}

function updateSettingsAndRefreshTicketing(
  queryClient: ReturnType<typeof useQueryClient>,
  settings: LinearIntegrationSettings,
) {
  queryClient.setQueryData(linearIntegrationKeys.settings(), settings);
  invalidateTicketingQueries(queryClient);
}

export function useLinearIntegration() {
  const queryClient = useQueryClient();
  const settingsQuery = useQuery({
    queryKey: linearIntegrationKeys.settings(),
    queryFn: () => linearApi.getSettings(),
    staleTime: 15_000,
  });

  const saveSettingsMutation = useMutation({
    mutationFn: (input: SaveLinearIntegrationSettingsInput) =>
      linearApi.saveSettings(input),
    onSuccess: (settings) => {
      updateSettingsAndRefreshTicketing(queryClient, settings);
    },
  });

  const validateMutation = useMutation({
    mutationFn: () => linearApi.validate(),
    onSuccess: (settings) => {
      updateSettingsAndRefreshTicketing(queryClient, settings);
    },
  });

  const disconnectMutation = useMutation({
    mutationFn: () => linearApi.disconnect(),
    onSuccess: (settings) => {
      updateSettingsAndRefreshTicketing(queryClient, settings);
    },
  });

  return {
    settings: settingsQuery.data,
    isLoading: settingsQuery.isLoading,
    isError: settingsQuery.isError,
    error: settingsQuery.error,
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
