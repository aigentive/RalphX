import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";

import {
  granolaApi,
  type GranolaIntegrationSettings,
  type SaveGranolaIntegrationSettingsInput,
} from "@/api/granola";

export const granolaIntegrationKeys = {
  all: ["granola-integration"] as const,
  settings: () => [...granolaIntegrationKeys.all, "settings"] as const,
};

export function isGranolaConnected(
  settings: GranolaIntegrationSettings | undefined,
): boolean {
  return Boolean(
    settings?.enabled &&
      settings.hasApiToken &&
      settings.validationStatus === "valid",
  );
}

export function useGranolaIntegration() {
  const queryClient = useQueryClient();
  const settingsQuery = useQuery({
    queryKey: granolaIntegrationKeys.settings(),
    queryFn: () => granolaApi.getSettings(),
    staleTime: 15_000,
  });

  const saveSettingsMutation = useMutation({
    mutationFn: (input: SaveGranolaIntegrationSettingsInput) =>
      granolaApi.saveSettings(input),
    onSuccess: (settings) => {
      queryClient.setQueryData(granolaIntegrationKeys.settings(), settings);
    },
  });

  const validateMutation = useMutation({
    mutationFn: () => granolaApi.validate(),
    onSuccess: (settings) => {
      queryClient.setQueryData(granolaIntegrationKeys.settings(), settings);
    },
  });

  const disconnectMutation = useMutation({
    mutationFn: () => granolaApi.disconnect(),
    onSuccess: (settings) => {
      queryClient.setQueryData(granolaIntegrationKeys.settings(), settings);
    },
  });

  return {
    settings: settingsQuery.data,
    isLoading: settingsQuery.isLoading,
    isError: settingsQuery.isError,
    error: settingsQuery.error,
    connected: isGranolaConnected(settingsQuery.data),
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
