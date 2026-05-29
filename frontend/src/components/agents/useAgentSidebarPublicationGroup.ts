import { useInfiniteQuery, useQuery } from "@tanstack/react-query";

import {
  chatApi,
  type AgentSidebarConversationGroup,
  type AgentSidebarPublicationState,
  type AgentSidebarGroupBy,
  type AgentSidebarSort,
} from "@/api/chat";

export const AGENT_SIDEBAR_GROUP_PAGE_SIZE = 8;

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
};

export function useAgentSidebarPublicationGroup({
  projectIds,
  publicationState,
  archivedOnly,
  search,
  pinnedConversationIds,
  priorityConversationIds = [],
  sort,
  enabled = true,
  minimumRowCount = 0,
}: {
  projectIds: string[];
  publicationState: AgentSidebarPublicationState;
  archivedOnly: boolean;
  search: string;
  pinnedConversationIds: string[];
  priorityConversationIds?: string[];
  sort: AgentSidebarSort;
  enabled?: boolean;
  minimumRowCount?: number;
}) {
  return useAgentSidebarGroup({
    groupBy: "publication",
    groupKey: publicationState,
    projectIds,
    archivedOnly,
    search,
    publicationStates: [publicationState],
    pinnedConversationIds,
    priorityConversationIds,
    sort,
    enabled,
    minimumRowCount,
    queryKey: agentSidebarConversationKeys.publicationGroup(
      projectIds,
      publicationState,
      archivedOnly,
      search,
      pinnedConversationIds,
      priorityConversationIds,
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
  priorityConversationIds = [],
  enabled = true,
  minimumRowCount = 0,
}: {
  projectId: string | null | undefined;
  archivedOnly: boolean;
  search: string;
  publicationStates: AgentSidebarPublicationState[];
  pinnedConversationIds: string[];
  priorityConversationIds?: string[];
  enabled?: boolean;
  minimumRowCount?: number;
}) {
  return useAgentSidebarGroup({
    groupBy: "project",
    groupKey: projectId ?? "",
    projectIds: projectId ? [projectId] : [],
    archivedOnly,
    search,
    publicationStates,
    pinnedConversationIds,
    priorityConversationIds,
    enabled: enabled && Boolean(projectId),
    minimumRowCount,
    queryKey: agentSidebarConversationKeys.projectGroup(
      projectId,
      archivedOnly,
      search,
      publicationStates,
      pinnedConversationIds,
      priorityConversationIds
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
  priorityConversationIds = [],
  sort,
  enabled,
  minimumRowCount,
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
  queryKey: readonly unknown[];
}) {
  const normalizedSearch = search.trim();
  const initialLimitPerGroup = Math.max(
    AGENT_SIDEBAR_GROUP_PAGE_SIZE,
    Math.ceil(minimumRowCount)
  );
  const query = useInfiniteQuery({
    queryKey: [...queryKey, "initial-limit", initialLimitPerGroup] as const,
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
          offset === 0 ? initialLimitPerGroup : AGENT_SIDEBAR_GROUP_PAGE_SIZE,
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

export function useProjectGroupLatestOrder({
  projectIds,
  archivedOnly,
  publicationStates,
  enabled = true,
}: {
  projectIds: string[];
  archivedOnly: boolean;
  publicationStates: AgentSidebarPublicationState[];
  enabled?: boolean;
}) {
  return useQuery({
    queryKey: [
      ...agentSidebarConversationKeys.all,
      "project-order",
      projectIds,
      archivedOnly,
      publicationStates,
    ],
    queryFn: async () => {
      const response = await chatApi.listAgentSidebarConversations({
        projectIds,
        archivedOnly,
        groupBy: "project",
        sort: "latest",
        limitPerGroup: 1,
        publicationStates,
      });
      return response.groups
        .filter((g) => g.rows.length > 0)
        .map((g) => g.key);
    },
    enabled: enabled && projectIds.length > 0,
    staleTime: 10_000,
  });
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
