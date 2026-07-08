import { useInfiniteQuery } from "@tanstack/react-query";

import {
  chatApi,
  type AgentSidebarConversationGroup,
  type AgentSidebarGroupBy,
  type AgentSidebarPublicationState,
  type AgentSidebarSort,
} from "@/api/chat";

export const AGENT_SIDEBAR_GROUP_PAGE_SIZE = 8;
export const AGENT_SIDEBAR_GROUP_MAX_PAGE_SIZE = 100;

export const agentSidebarConversationKeys = {
  all: ["agents", "sidebar-conversations"] as const,
  publicationGroup: (
    projectIds: string[],
    publicationState: AgentSidebarPublicationState,
    archivedOnly: boolean,
    search = "",
    pinnedConversationIds: string[] = [],
    priorityConversationIds: string[] = [],
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
      "priority",
      priorityConversationIds,
      "sort",
      sort,
    ] as const,
  projectGroup: (
    projectId: string | null | undefined,
    archivedOnly: boolean,
    search = "",
    publicationStates: AgentSidebarPublicationState[] = [],
    pinnedConversationIds: string[] = [],
    priorityConversationIds: string[] = []
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
      "priority",
      priorityConversationIds,
    ] as const,
  automationIndex: (
    projectIds: string[],
    archivedOnly: boolean,
    search = "",
    publicationStates: AgentSidebarPublicationState[] = [],
    pinnedConversationIds: string[] = [],
    priorityConversationIds: string[] = [],
    sort: AgentSidebarSort = "latest"
  ) =>
    [
      ...agentSidebarConversationKeys.all,
      "automation",
      "index",
      "projects",
      projectIds,
      "archived",
      archivedOnly,
      "search",
      search.trim().toLowerCase(),
      "states",
      publicationStates,
      "pinned",
      pinnedConversationIds,
      "priority",
      priorityConversationIds,
      "sort",
      sort,
    ] as const,
  automationGroup: (
    groupKey: string,
    projectIds: string[],
    archivedOnly: boolean,
    search = "",
    publicationStates: AgentSidebarPublicationState[] = [],
    pinnedConversationIds: string[] = [],
    priorityConversationIds: string[] = [],
    sort: AgentSidebarSort = "latest"
  ) =>
    [
      ...agentSidebarConversationKeys.all,
      "automation",
      groupKey,
      "projects",
      projectIds,
      "archived",
      archivedOnly,
      "search",
      search.trim().toLowerCase(),
      "states",
      publicationStates,
      "pinned",
      pinnedConversationIds,
      "priority",
      priorityConversationIds,
      "sort",
      sort,
    ] as const,
};

export function useAgentSidebarGroup({
  groupBy,
  groupKey,
  projectIds,
  archivedOnly,
  search,
  publicationStates,
  pinnedConversationIds,
  priorityConversationIds = [],
  sort,
  enabled,
  minimumRowCount,
  pageSize,
  queryKey,
}: {
  groupBy: AgentSidebarGroupBy;
  groupKey: string;
  projectIds: string[];
  archivedOnly: boolean;
  search: string;
  publicationStates: AgentSidebarPublicationState[];
  pinnedConversationIds: string[];
  priorityConversationIds?: string[];
  sort?: AgentSidebarSort;
  enabled: boolean;
  minimumRowCount: number;
  pageSize?: number;
  queryKey: readonly unknown[];
}) {
  const normalizedSearch = search.trim();
  const normalizedPageSize = Math.min(
    AGENT_SIDEBAR_GROUP_MAX_PAGE_SIZE,
    Math.max(
      AGENT_SIDEBAR_GROUP_PAGE_SIZE,
      Math.ceil(pageSize ?? AGENT_SIDEBAR_GROUP_PAGE_SIZE)
    )
  );
  const initialLimitPerGroup = Math.max(
    normalizedPageSize,
    Math.ceil(minimumRowCount)
  );
  const query = useInfiniteQuery({
    queryKey: [
      ...queryKey,
      "page-size",
      normalizedPageSize,
      "initial-limit",
      initialLimitPerGroup,
    ] as const,
    queryFn: async ({ pageParam = 0 }) => {
      const offset = Number(pageParam) || 0;
      const response = await chatApi.listAgentSidebarConversations({
        projectIds,
        includeArchived: archivedOnly,
        archivedOnly,
        ...(normalizedSearch ? { search: normalizedSearch } : {}),
        publicationStates,
        groupBy,
        ...(sort ? { sort } : {}),
        limitPerGroup:
          offset === 0 ? initialLimitPerGroup : normalizedPageSize,
        offsets: { [groupKey]: offset },
        pinnedConversationIds,
        priorityConversationIds,
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
