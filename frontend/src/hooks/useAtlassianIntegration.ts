import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";

import {
  atlassianApi,
  type AtlassianIntegrationSettings,
  type CompleteAtlassianOAuthLocalCallbackInput,
  type ExchangeAtlassianOAuthCodeInput,
  type SaveAtlassianIntegrationSettingsInput,
} from "@/api/atlassian";
import { invalidateTicketingQueries } from "@/hooks/useTicketing";

export const atlassianIntegrationKeys = {
  all: ["atlassian-integration"] as const,
  settings: () => [...atlassianIntegrationKeys.all, "settings"] as const,
};

/** Atlassian is connected once the integration is enabled. */
export function isAtlassianConnected(
  settings: AtlassianIntegrationSettings | undefined,
): boolean {
  return Boolean(settings?.enabled);
}

export function isConfluenceConnected(
  settings: AtlassianIntegrationSettings | undefined,
): boolean {
  return Boolean(
    settings?.enabled &&
      settings.validationStatus === "valid" &&
      settings.confluenceAvailable,
  );
}

function updateSettingsAndRefreshTicketing(
  queryClient: ReturnType<typeof useQueryClient>,
  settings: AtlassianIntegrationSettings,
) {
  queryClient.setQueryData(atlassianIntegrationKeys.settings(), settings);
  invalidateTicketingQueries(queryClient);
}

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
      updateSettingsAndRefreshTicketing(queryClient, settings);
    },
  });

  const validateMutation = useMutation({
    mutationFn: () => atlassianApi.validate(),
    onSuccess: (settings) => {
      updateSettingsAndRefreshTicketing(queryClient, settings);
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
      updateSettingsAndRefreshTicketing(queryClient, settings);
    },
  });

  const exchangeOAuthCodeMutation = useMutation({
    mutationFn: (input: ExchangeAtlassianOAuthCodeInput) =>
      atlassianApi.exchangeOAuthCode(input),
    onSuccess: (settings) => {
      updateSettingsAndRefreshTicketing(queryClient, settings);
    },
  });

  const disconnectMutation = useMutation({
    mutationFn: () => atlassianApi.disconnect(),
    onSuccess: (settings) => {
      updateSettingsAndRefreshTicketing(queryClient, settings);
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
