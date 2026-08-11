import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";

import { updateChannelApi, type UpdateChannel } from "@/api/update-channel";

export const updateChannelKeys = {
  all: ["update-channel"] as const,
  current: () => updateChannelKeys.all,
};

const STABLE_UPDATE_CHANNEL: UpdateChannel = "stable";

export function useUpdateChannel() {
  const queryClient = useQueryClient();
  const query = useQuery({
    queryKey: updateChannelKeys.current(),
    queryFn: updateChannelApi.get,
    retry: false,
    staleTime: Infinity,
  });
  const mutation = useMutation({
    mutationFn: (updateChannel: UpdateChannel) => updateChannelApi.set(updateChannel),
    onSuccess: async (updateChannel) => {
      queryClient.setQueryData(updateChannelKeys.current(), updateChannel);
      await queryClient.invalidateQueries({
        queryKey: updateChannelKeys.current(),
      });
    },
  });

  return {
    updateChannel: query.data ?? STABLE_UPDATE_CHANNEL,
    isSettled: query.isSuccess || query.isError,
    isLoading: query.isPending,
    isError: query.isError,
    loadError: query.error,
    setUpdateChannel: mutation.mutate,
    isSaving: mutation.isPending,
    saveError: mutation.error,
  };
}
