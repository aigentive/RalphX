import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";

import {
  harnessProvidersApi,
  type AgentProvidersSettingsResponse,
  type UpdateAgentProviderSettingsInput,
} from "@/api/harness-providers";
import { manualRoleDefaultKeys } from "@/hooks/useManualRoleDefaults";

export const harnessProviderKeys = {
  all: ["agent", "providers"] as const,
  list: (refreshRuntime: boolean) =>
    ["agent", "providers", { refreshRuntime }] as const,
};

const EMPTY_PROVIDER_SETTINGS: AgentProvidersSettingsResponse = {
  providers: [],
  defaultProvider: null,
  requiresOnboarding: true,
};

interface UseHarnessProvidersOptions {
  refreshRuntime?: boolean;
  enabled?: boolean;
}

interface RefetchProviderOptions {
  forceRuntime?: boolean;
}

export function useHarnessProviders(options: UseHarnessProvidersOptions = {}) {
  const queryClient = useQueryClient();
  const refreshRuntime = options.refreshRuntime ?? false;
  const query = useQuery({
    queryKey: harnessProviderKeys.list(refreshRuntime),
    queryFn: () => harnessProvidersApi.list({ refreshRuntime }),
    staleTime: 1000 * 30,
    gcTime: 1000 * 60 * 5,
    placeholderData: EMPTY_PROVIDER_SETTINGS,
    enabled: options.enabled ?? true,
  });

  const mutation = useMutation({
    mutationFn: (input: UpdateAgentProviderSettingsInput) =>
      harnessProvidersApi.update(input),
    onSuccess: async (updatedSettings) => {
      queryClient.setQueriesData<AgentProvidersSettingsResponse>(
        { queryKey: harnessProviderKeys.all },
        updatedSettings,
      );
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: ["provider-cli-management"] }),
        queryClient.invalidateQueries({ queryKey: ["agent", "harness"] }),
        queryClient.invalidateQueries({ queryKey: manualRoleDefaultKeys.all }),
      ]);
    },
  });

  const refetchProviders = async (
    options: RefetchProviderOptions = {},
  ) => {
    if (!options.forceRuntime) {
      return query.refetch();
    }

    const settings = await queryClient.fetchQuery({
      queryKey: harnessProviderKeys.list(true),
      queryFn: () =>
        harnessProvidersApi.list({ refreshRuntime: true, forceRuntime: true }),
      staleTime: 0,
    });
    queryClient.setQueriesData<AgentProvidersSettingsResponse>(
      { queryKey: harnessProviderKeys.all },
      settings,
    );
    return { data: settings };
  };

  return {
    settings: query.data ?? EMPTY_PROVIDER_SETTINGS,
    providers: query.data?.providers ?? [],
    isLoading: query.isLoading,
    isPlaceholderData: query.isPlaceholderData,
    isError: query.isError,
    error: query.error,
    refetchProviders,
    updateProviderAsync: mutation.mutateAsync,
    isUpdating: mutation.isPending,
    updateError: mutation.error,
  };
}
