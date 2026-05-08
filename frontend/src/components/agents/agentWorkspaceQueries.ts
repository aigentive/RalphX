import type { QueryClient } from "@tanstack/react-query";

export function invalidateWorkspaceQueries(
  queryClient: QueryClient,
  conversationId: string,
): Promise<unknown[]> {
  return Promise.all([
    queryClient.invalidateQueries({
      queryKey: ["agents", "conversation-workspace", conversationId],
    }),
    queryClient.invalidateQueries({
      queryKey: ["agents", "conversation-workspace-freshness", conversationId],
    }),
    queryClient.invalidateQueries({
      queryKey: ["agents", "conversation-workspace-publication-events", conversationId],
    }),
    queryClient.invalidateQueries({
      queryKey: ["agents", "workspace-diff", conversationId],
    }),
    queryClient.invalidateQueries({
      queryKey: ["agents", "workspace-commits", conversationId],
    }),
  ]);
}
