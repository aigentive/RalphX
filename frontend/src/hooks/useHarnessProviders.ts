import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";

import {
  harnessProvidersApi,
  type AgentProvidersSettingsResponse,
  type UpdateAgentProviderSettingsInput,
} from "@/api/harness-providers";

export const harnessProviderKeys = {
  all: ["agent", "providers"] as const,
};

const EMPTY_PROVIDER_SETTINGS: AgentProvidersSettingsResponse = {
  providers: [],
  defaultProvider: null,
  requiresOnboarding: true,
};

export function useHarnessProviders() {
  const queryClient = useQueryClient();
  const query = useQuery({
    queryKey: harnessProviderKeys.all,
    queryFn: harnessProvidersApi.list,
    staleTime: 1000 * 30,
    gcTime: 1000 * 60 * 5,
    placeholderData: EMPTY_PROVIDER_SETTINGS,
  });

  const mutation = useMutation({
    mutationFn: (input: UpdateAgentProviderSettingsInput) =>
      harnessProvidersApi.update(input),
    onSuccess: async () => {
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: harnessProviderKeys.all }),
        queryClient.invalidateQueries({ queryKey: ["agent", "harness"] }),
      ]);
    },
  });

  return {
    settings: query.data ?? EMPTY_PROVIDER_SETTINGS,
    providers: query.data?.providers ?? [],
    isLoading: query.isLoading,
    isPlaceholderData: query.isPlaceholderData,
    isError: query.isError,
    error: query.error,
    refetchProviders: query.refetch,
    updateProviderAsync: mutation.mutateAsync,
    isUpdating: mutation.isPending,
    updateError: mutation.error,
  };
}
