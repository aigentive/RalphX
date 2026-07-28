import { useQuery } from "@tanstack/react-query";

import {
  chatApi,
  type AgentSidebarAttentionLane,
  type AgentSidebarPublicationState,
  type AgentSidebarSort,
} from "@/api/chat";
import {
  agentSidebarConversationKeys,
  useAgentSidebarGroup,
} from "./agentSidebarConversationQueries";

export {
  AGENT_SIDEBAR_GROUP_MAX_PAGE_SIZE,
  AGENT_SIDEBAR_GROUP_PAGE_SIZE,
  agentSidebarConversationKeys,
} from "./agentSidebarConversationQueries";

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
  pageSize,
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
  pageSize?: number;
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
    ...(pageSize !== undefined ? { pageSize } : {}),
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

export function useAgentSidebarInboxGroup({
  lane,
  projectIds,
  archivedOnly,
  search,
  publicationStates,
  pinnedConversationIds,
  priorityConversationIds = [],
  sort,
  enabled = true,
  minimumRowCount = 0,
  pageSize,
}: {
  lane: AgentSidebarAttentionLane;
  projectIds: string[];
  archivedOnly: boolean;
  search: string;
  publicationStates: AgentSidebarPublicationState[];
  pinnedConversationIds: string[];
  priorityConversationIds?: string[];
  sort: AgentSidebarSort;
  enabled?: boolean;
  minimumRowCount?: number;
  pageSize?: number;
}) {
  return useAgentSidebarGroup({
    groupBy: "inbox",
    groupKey: lane,
    projectIds,
    archivedOnly,
    search,
    publicationStates,
    pinnedConversationIds,
    priorityConversationIds,
    sort,
    enabled,
    minimumRowCount,
    ...(pageSize !== undefined ? { pageSize } : {}),
    queryKey: agentSidebarConversationKeys.inboxGroup(
      projectIds,
      lane,
      archivedOnly,
      search,
      publicationStates,
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
  pageSize,
}: {
  projectId: string | null | undefined;
  archivedOnly: boolean;
  search: string;
  publicationStates: AgentSidebarPublicationState[];
  pinnedConversationIds: string[];
  priorityConversationIds?: string[];
  enabled?: boolean;
  minimumRowCount?: number;
  pageSize?: number;
}) {
  const isNoProjectGroup = projectId === "__no_project__";
  return useAgentSidebarGroup({
    groupBy: "project",
    groupKey: projectId ?? "",
    projectIds: projectId && !isNoProjectGroup ? [projectId] : [],
    archivedOnly,
    search,
    publicationStates,
    pinnedConversationIds,
    priorityConversationIds,
    enabled: enabled && Boolean(projectId),
    allowEmptyProjectIds: isNoProjectGroup,
    minimumRowCount,
    ...(pageSize !== undefined ? { pageSize } : {}),
    queryKey: isNoProjectGroup
      ? agentSidebarConversationKeys.noProjectGroup(
          archivedOnly,
          search,
          publicationStates,
          pinnedConversationIds,
          priorityConversationIds,
        )
      : agentSidebarConversationKeys.projectGroup(
          projectId,
          archivedOnly,
          search,
          publicationStates,
          pinnedConversationIds,
          priorityConversationIds,
        ),
  });
}

export function useAgentSidebarAutomationGroupIndex({
  projectIds,
  archivedOnly,
  search,
  publicationStates,
  pinnedConversationIds,
  priorityConversationIds = [],
  sort,
  enabled = true,
}: {
  projectIds: string[];
  archivedOnly: boolean;
  search: string;
  publicationStates: AgentSidebarPublicationState[];
  pinnedConversationIds: string[];
  priorityConversationIds?: string[];
  sort: AgentSidebarSort;
  enabled?: boolean;
}) {
  const normalizedSearch = search.trim();
  return useQuery({
    queryKey: agentSidebarConversationKeys.automationIndex(
      projectIds,
      archivedOnly,
      search,
      publicationStates,
      pinnedConversationIds,
      priorityConversationIds,
      sort
    ),
    queryFn: async () => {
      const response = await chatApi.listAgentSidebarConversations({
        projectIds,
        includeArchived: archivedOnly,
        archivedOnly,
        ...(normalizedSearch ? { search: normalizedSearch } : {}),
        publicationStates,
        groupBy: "automation",
        sort,
        limitPerGroup: 1,
        pinnedConversationIds,
        priorityConversationIds,
      });
      return response.groups;
    },
    enabled: enabled && projectIds.length > 0,
    staleTime: 10_000,
  });
}

export function useAgentSidebarAutomationGroup({
  groupKey,
  projectIds,
  archivedOnly,
  search,
  publicationStates,
  pinnedConversationIds,
  priorityConversationIds = [],
  sort,
  enabled = true,
  minimumRowCount = 0,
  pageSize,
}: {
  groupKey: string;
  projectIds: string[];
  archivedOnly: boolean;
  search: string;
  publicationStates: AgentSidebarPublicationState[];
  pinnedConversationIds: string[];
  priorityConversationIds?: string[];
  sort: AgentSidebarSort;
  enabled?: boolean;
  minimumRowCount?: number;
  pageSize?: number;
}) {
  return useAgentSidebarGroup({
    groupBy: "automation",
    groupKey,
    projectIds,
    archivedOnly,
    search,
    publicationStates,
    pinnedConversationIds,
    priorityConversationIds,
    sort,
    enabled,
    minimumRowCount,
    ...(pageSize !== undefined ? { pageSize } : {}),
    queryKey: agentSidebarConversationKeys.automationGroup(
      groupKey,
      projectIds,
      archivedOnly,
      search,
      publicationStates,
      pinnedConversationIds,
      priorityConversationIds,
      sort
    ),
  });
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
