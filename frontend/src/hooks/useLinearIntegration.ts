import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";

import {
  linearApi,
  type SaveLinearIntegrationSettingsInput,
} from "@/api/linear";

export const linearIntegrationKeys = {
  all: ["linear-integration"] as const,
  settings: () => [...linearIntegrationKeys.all, "settings"] as const,
};

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
      queryClient.setQueryData(linearIntegrationKeys.settings(), settings);
    },
  });

  const validateMutation = useMutation({
    mutationFn: () => linearApi.validate(),
    onSuccess: (settings) => {
      queryClient.setQueryData(linearIntegrationKeys.settings(), settings);
    },
  });

  const disconnectMutation = useMutation({
    mutationFn: () => linearApi.disconnect(),
    onSuccess: (settings) => {
      queryClient.setQueryData(linearIntegrationKeys.settings(), settings);
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
