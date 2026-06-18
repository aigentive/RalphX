import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";

import {
  providerCliManagementApi,
  type ManagedProviderCliActionInput,
  type ManagedProviderCliStatusesResponse,
} from "@/api/provider-cli-management";
import { harnessProviderKeys } from "@/hooks/useHarnessProviders";

export const providerCliManagementKeys = {
  all: ["provider-cli-management"] as const,
  status: () => ["provider-cli-management", "status"] as const,
};

const EMPTY_STATUS: ManagedProviderCliStatusesResponse = {
  providers: [],
};

export function useProviderCliManagement() {
  const queryClient = useQueryClient();
  const statusQuery = useQuery({
    queryKey: providerCliManagementKeys.status(),
    queryFn: () => providerCliManagementApi.status(),
    staleTime: 1000 * 60,
    gcTime: 1000 * 60 * 5,
    placeholderData: EMPTY_STATUS,
  });

  const installMutation = useMutation({
    mutationFn: (input: ManagedProviderCliActionInput) =>
      providerCliManagementApi.installOrUpdate(input),
    onSuccess: async () => {
      await Promise.all([
        queryClient.invalidateQueries({
          queryKey: providerCliManagementKeys.all,
        }),
        queryClient.invalidateQueries({ queryKey: harnessProviderKeys.all }),
        queryClient.invalidateQueries({ queryKey: ["agent", "harness"] }),
      ]);
    },
  });

  const autoUpdateMutation = useMutation({
    mutationFn: () => providerCliManagementApi.autoUpdate(),
    onSuccess: async () => {
      await Promise.all([
        queryClient.invalidateQueries({
          queryKey: providerCliManagementKeys.all,
        }),
        queryClient.invalidateQueries({ queryKey: harnessProviderKeys.all }),
        queryClient.invalidateQueries({ queryKey: ["agent", "harness"] }),
      ]);
    },
  });

  return {
    statuses: statusQuery.data ?? EMPTY_STATUS,
    statusByProvider: new Map(
      (statusQuery.data?.providers ?? []).map((provider) => [
        provider.provider,
        provider,
      ]),
    ),
    isLoadingStatus: statusQuery.isLoading,
    isStatusPlaceholderData: statusQuery.isPlaceholderData,
    isStatusError: statusQuery.isError,
    statusError: statusQuery.error,
    refetchStatus: statusQuery.refetch,
    installOrUpdateProviderAsync: installMutation.mutateAsync,
    isInstallingProvider: installMutation.isPending,
    installError: installMutation.error,
    autoUpdateProvidersAsync: autoUpdateMutation.mutateAsync,
    isAutoUpdatingProviders: autoUpdateMutation.isPending,
    autoUpdateError: autoUpdateMutation.error,
  };
}
