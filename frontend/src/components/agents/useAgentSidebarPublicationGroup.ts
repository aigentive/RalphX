import { useInfiniteQuery } from "@tanstack/react-query";

import {
  chatApi,
  type AgentSidebarConversationGroup,
  type AgentSidebarPublicationState,
} from "@/api/chat";

const AGENT_SIDEBAR_GROUP_PAGE_SIZE = 20;

export const agentSidebarConversationKeys = {
  all: ["agents", "sidebar-conversations"] as const,
  publicationGroup: (
    projectIds: string[],
    publicationState: AgentSidebarPublicationState,
    archivedOnly: boolean,
    search = "",
    pinnedConversationIds: string[] = []
  ) =>
    [
      ...agentSidebarConversationKeys.all,
      "publication",
      publicationState,
      "projects",
      projectIds,
      "archived",
      archivedOnly,
      "search",
      search.trim().toLowerCase(),
      "pinned",
      pinnedConversationIds,
    ] as const,
};

export function useAgentSidebarPublicationGroup({
  projectIds,
  publicationState,
  archivedOnly,
  search,
  pinnedConversationIds,
  enabled = true,
}: {
  projectIds: string[];
  publicationState: AgentSidebarPublicationState;
  archivedOnly: boolean;
  search: string;
  pinnedConversationIds: string[];
  enabled?: boolean;
}) {
  const normalizedSearch = search.trim();
  const query = useInfiniteQuery({
    queryKey: agentSidebarConversationKeys.publicationGroup(
      projectIds,
      publicationState,
      archivedOnly,
      normalizedSearch,
      pinnedConversationIds
    ),
    queryFn: async ({ pageParam = 0 }) => {
      const response = await chatApi.listAgentSidebarConversations({
        projectIds,
        includeArchived: archivedOnly,
        archivedOnly,
        ...(normalizedSearch ? { search: normalizedSearch } : {}),
        publicationStates: [publicationState],
        groupBy: "publication",
        limitPerGroup: AGENT_SIDEBAR_GROUP_PAGE_SIZE,
        offsets: { [publicationState]: pageParam },
        pinnedConversationIds,
      });

      return response.groups[0] ?? emptyPublicationGroup(publicationState);
    },
    getNextPageParam: (lastPage) =>
      lastPage.hasMore ? lastPage.offset + lastPage.rows.length : undefined,
    initialPageParam: 0,
    enabled: enabled && projectIds.length > 0,
    staleTime: 5_000,
  });

  const pages = query.data?.pages ?? [];
  const firstPage = pages[0] ?? emptyPublicationGroup(publicationState);
  const lastPage = pages.length > 0 ? pages[pages.length - 1] : null;

  return {
    ...query,
    group: {
      ...firstPage,
      hasMore: lastPage?.hasMore ?? firstPage.hasMore,
      offset: 0,
      rows: pages.flatMap((page) => page.rows),
    },
  };
}

function emptyPublicationGroup(
  publicationState: AgentSidebarPublicationState
): AgentSidebarConversationGroup {
  return {
    key: publicationState,
    label: "",
    total: 0,
    offset: 0,
    limit: AGENT_SIDEBAR_GROUP_PAGE_SIZE,
    hasMore: false,
    rows: [],
  };
}
