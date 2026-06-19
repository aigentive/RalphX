import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";

import {
  atlassianApi,
  type CompleteAtlassianOAuthLocalCallbackInput,
  type ExchangeAtlassianOAuthCodeInput,
  type SaveAtlassianIntegrationSettingsInput,
} from "@/api/atlassian";

export const atlassianIntegrationKeys = {
  all: ["atlassian-integration"] as const,
  settings: () => [...atlassianIntegrationKeys.all, "settings"] as const,
};

export function useAtlassianIntegration() {
  const queryClient = useQueryClient();
  const settingsQuery = useQuery({
    queryKey: atlassianIntegrationKeys.settings(),
    queryFn: () => atlassianApi.getSettings(),
    staleTime: 15_000,
  });

  const saveMutation = useMutation({
    mutationFn: (input: SaveAtlassianIntegrationSettingsInput) =>
      atlassianApi.saveSettings(input),
    onSuccess: (settings) => {
      queryClient.setQueryData(atlassianIntegrationKeys.settings(), settings);
    },
  });

  const validateMutation = useMutation({
    mutationFn: () => atlassianApi.validate(),
    onSuccess: (settings) => {
      queryClient.setQueryData(atlassianIntegrationKeys.settings(), settings);
    },
  });

  const buildOAuthAuthorizationMutation = useMutation({
    mutationFn: () => atlassianApi.buildOAuthAuthorization(),
  });

  const startOAuthLocalCallbackMutation = useMutation({
    mutationFn: () => atlassianApi.startOAuthLocalCallback(),
  });

  const completeOAuthLocalCallbackMutation = useMutation({
    mutationFn: (input: CompleteAtlassianOAuthLocalCallbackInput) =>
      atlassianApi.completeOAuthLocalCallback(input),
    onSuccess: (settings) => {
      queryClient.setQueryData(atlassianIntegrationKeys.settings(), settings);
    },
  });

  const exchangeOAuthCodeMutation = useMutation({
    mutationFn: (input: ExchangeAtlassianOAuthCodeInput) =>
      atlassianApi.exchangeOAuthCode(input),
    onSuccess: (settings) => {
      queryClient.setQueryData(atlassianIntegrationKeys.settings(), settings);
    },
  });

  const disconnectMutation = useMutation({
    mutationFn: () => atlassianApi.disconnect(),
    onSuccess: (settings) => {
      queryClient.setQueryData(atlassianIntegrationKeys.settings(), settings);
    },
  });

  return {
    settings: settingsQuery.data,
    isLoading: settingsQuery.isLoading,
    isError: settingsQuery.isError,
    error: settingsQuery.error,
    saveAsync: saveMutation.mutateAsync,
    validateAsync: validateMutation.mutateAsync,
    disconnectAsync: disconnectMutation.mutateAsync,
    buildOAuthAuthorizationAsync: buildOAuthAuthorizationMutation.mutateAsync,
    startOAuthLocalCallbackAsync: startOAuthLocalCallbackMutation.mutateAsync,
    completeOAuthLocalCallbackAsync: completeOAuthLocalCallbackMutation.mutateAsync,
    exchangeOAuthCodeAsync: exchangeOAuthCodeMutation.mutateAsync,
    isSaving: saveMutation.isPending,
    isValidating: validateMutation.isPending,
    isDisconnecting: disconnectMutation.isPending,
    isBuildingOAuthAuthorization: buildOAuthAuthorizationMutation.isPending,
    isStartingOAuthLocalCallback: startOAuthLocalCallbackMutation.isPending,
    isCompletingOAuthLocalCallback: completeOAuthLocalCallbackMutation.isPending,
    isExchangingOAuthCode: exchangeOAuthCodeMutation.isPending,
    saveError: saveMutation.error,
    validateError: validateMutation.error,
    disconnectError: disconnectMutation.error,
    buildOAuthAuthorizationError: buildOAuthAuthorizationMutation.error,
    startOAuthLocalCallbackError: startOAuthLocalCallbackMutation.error,
    completeOAuthLocalCallbackError: completeOAuthLocalCallbackMutation.error,
    exchangeOAuthCodeError: exchangeOAuthCodeMutation.error,
  };
}
