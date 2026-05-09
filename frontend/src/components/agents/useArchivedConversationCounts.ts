import { useMemo } from "react";
import { useQueries } from "@tanstack/react-query";

import { chatApi } from "@/api/chat";

export const archivedConversationCountKey = (projectId: string) =>
  [
    "agents",
    "project-conversations",
    projectId,
    "archived-count",
  ] as const;

export function useArchivedConversationCounts(projectIds: string[]) {
  const archivedCountQueries = useQueries({
    queries: projectIds.map((projectId) => ({
      queryKey: archivedConversationCountKey(projectId),
      queryFn: () =>
        chatApi.listConversationsPage(
          "project",
          projectId,
          1,
          0,
          true,
          undefined,
          true
        ),
      enabled: Boolean(projectId),
      staleTime: 60_000,
      gcTime: 5 * 60_000,
      refetchOnWindowFocus: false,
    })),
  });

  return useMemo(() => {
    const byProjectId = Object.fromEntries(
      projectIds.map((projectId, index) => [
        projectId,
        archivedCountQueries[index]?.data?.total ?? 0,
      ])
    );
    const totalArchivedCount = Object.values(byProjectId).reduce(
      (sum, count) => sum + count,
      0
    );

    return {
      byProjectId,
      totalArchivedCount,
      isLoading: archivedCountQueries.some((query) => query.isLoading),
    };
  }, [archivedCountQueries, projectIds]);
}
