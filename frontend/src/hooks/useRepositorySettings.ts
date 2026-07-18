import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";

import {
  repositorySettingsApi,
  type RepositorySettings,
  type UpdateRepositorySettingsInput,
} from "@/api/repository-settings";

export const repositorySettingsKeys = {
  all: ["repository-settings"] as const,
};

export function useRepositorySettings() {
  return useQuery({
    queryKey: repositorySettingsKeys.all,
    queryFn: repositorySettingsApi.get,
  });
}

export function useUpdateRepositorySettings() {
  const queryClient = useQueryClient();

  return useMutation<RepositorySettings, Error, UpdateRepositorySettingsInput>({
    mutationFn: repositorySettingsApi.update,
    onSuccess: (settings) => {
      queryClient.setQueryData(repositorySettingsKeys.all, settings);
    },
  });
}
