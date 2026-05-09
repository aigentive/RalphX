import { useInfiniteQuery } from "@tanstack/react-query";

import {
  chatApi,
  type AgentSidebarConversationGroup,
  type AgentSidebarPublicationState,
  type AgentSidebarGroupBy,
  type AgentSidebarSort,
} from "@/api/chat";

const AGENT_SIDEBAR_GROUP_PAGE_SIZE = 6;

export const agentSidebarConversationKeys = {
  all: ["agents", "sidebar-conversations"] as const,
  publicationGroup: (
    projectIds: string[],
    publicationState: AgentSidebarPublicationState,
    archivedOnly: boolean,
    search = "",
    pinnedConversationIds: string[] = [],
    sort: AgentSidebarSort = "latest"
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
      "sort",
      sort,
    ] as const,
  projectGroup: (
    projectId: string | null | undefined,
    archivedOnly: boolean,
    search = "",
    publicationStates: AgentSidebarPublicationState[] = [],
    pinnedConversationIds: string[] = []
  ) =>
    [
      ...agentSidebarConversationKeys.all,
      "project",
      projectId ?? "",
      "archived",
      archivedOnly,
      "search",
      search.trim().toLowerCase(),
      "states",
      publicationStates,
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
  sort,
  enabled = true,
}: {
  projectIds: string[];
  publicationState: AgentSidebarPublicationState;
  archivedOnly: boolean;
  search: string;
  pinnedConversationIds: string[];
  sort: AgentSidebarSort;
  enabled?: boolean;
}) {
  return useAgentSidebarGroup({
    groupBy: "publication",
    groupKey: publicationState,
    projectIds,
    archivedOnly,
    search,
    publicationStates: [publicationState],
    pinnedConversationIds,
    sort,
    enabled,
    queryKey: agentSidebarConversationKeys.publicationGroup(
      projectIds,
      publicationState,
      archivedOnly,
      search,
      pinnedConversationIds,
      sort
    ),
  });
}

export function useAgentSidebarProjectGroup({
  projectId,
  archivedOnly,
  search,
  publicationStates,
  pinnedConversationIds,
  enabled = true,
}: {
  projectId: string | null | undefined;
  archivedOnly: boolean;
  search: string;
  publicationStates: AgentSidebarPublicationState[];
  pinnedConversationIds: string[];
  enabled?: boolean;
}) {
  return useAgentSidebarGroup({
    groupBy: "project",
    groupKey: projectId ?? "",
    projectIds: projectId ? [projectId] : [],
    archivedOnly,
    search,
    publicationStates,
    pinnedConversationIds,
    enabled: enabled && Boolean(projectId),
    queryKey: agentSidebarConversationKeys.projectGroup(
      projectId,
      archivedOnly,
      search,
      publicationStates,
      pinnedConversationIds
    ),
  });
}

function useAgentSidebarGroup({
  groupBy,
  groupKey,
  projectIds,
  archivedOnly,
  search,
  publicationStates,
  pinnedConversationIds,
  sort,
  enabled,
  queryKey,
}: {
  groupBy: AgentSidebarGroupBy;
  groupKey: string;
  projectIds: string[];
  archivedOnly: boolean;
  search: string;
  publicationStates: AgentSidebarPublicationState[];
  pinnedConversationIds: string[];
  sort?: AgentSidebarSort;
  enabled: boolean;
  queryKey: readonly unknown[];
}) {
  const normalizedSearch = search.trim();
  const query = useInfiniteQuery({
    queryKey,
    queryFn: async ({ pageParam = 0 }) => {
      const response = await chatApi.listAgentSidebarConversations({
        projectIds,
        includeArchived: archivedOnly,
        archivedOnly,
        ...(normalizedSearch ? { search: normalizedSearch } : {}),
        publicationStates,
        groupBy,
        ...(sort ? { sort } : {}),
        limitPerGroup: AGENT_SIDEBAR_GROUP_PAGE_SIZE,
        offsets: { [groupKey]: pageParam },
        pinnedConversationIds,
      });

      return response.groups.find((group) => group.key === groupKey) ?? emptyGroup(groupKey);
    },
    getNextPageParam: (lastPage) =>
      lastPage.hasMore ? lastPage.offset + lastPage.rows.length : undefined,
    initialPageParam: 0,
    enabled: enabled && projectIds.length > 0 && groupKey.length > 0,
    staleTime: 5_000,
  });

  const pages = query.data?.pages ?? [];
  const firstPage = pages[0] ?? emptyGroup(groupKey);
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

function emptyGroup(groupKey: string): AgentSidebarConversationGroup {
  return {
    key: groupKey,
    label: "",
    total: 0,
    offset: 0,
    limit: AGENT_SIDEBAR_GROUP_PAGE_SIZE,
    hasMore: false,
    rows: [],
  };
}
