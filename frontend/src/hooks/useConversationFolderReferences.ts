import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { conversationFolderReferencesApi } from "@/api/conversation-folder-references";

export const conversationFolderReferenceKeys = {
  all: ["conversation-folder-references"] as const,
  list: (conversationId: string) =>
    [...conversationFolderReferenceKeys.all, conversationId] as const,
};

export function useConversationFolderReferences(
  conversationId: string | null,
  enabled: boolean,
) {
  return useQuery({
    queryKey: conversationFolderReferenceKeys.list(conversationId ?? ""),
    queryFn: () => conversationFolderReferencesApi.list(conversationId!),
    enabled: enabled && conversationId !== null,
  });
}

export function useAddConversationFolderReference() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: conversationFolderReferencesApi.add,
    onSuccess: async (_reference, input) => {
      await queryClient.invalidateQueries({
        queryKey: conversationFolderReferenceKeys.list(input.conversationId),
      });
    },
  });
}

export function useRemoveConversationFolderReference() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: conversationFolderReferencesApi.remove,
    onSuccess: async (_result, input) => {
      await queryClient.invalidateQueries({
        queryKey: conversationFolderReferenceKeys.list(input.conversationId),
      });
    },
  });
}
