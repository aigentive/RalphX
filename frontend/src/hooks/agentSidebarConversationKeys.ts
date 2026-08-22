import type { QueryClient } from "@tanstack/react-query";

import type {
  AgentSidebarAttentionLane,
  AgentSidebarPublicationState,
  AgentSidebarSort,
} from "@/api/chat";

export const agentSidebarConversationKeys = {
  all: ["agents", "sidebar-conversations"] as const,
  noProjectGroup: (
    archivedOnly: boolean,
    search = "",
    publicationStates: AgentSidebarPublicationState[] = [],
    pinnedConversationIds: string[] = [],
    priorityConversationIds: string[] = [],
  ) =>
    [
      ...agentSidebarConversationKeys.all,
      "project",
      "__no_project__",
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
  automationScope: () => [...agentSidebarConversationKeys.all, "automation"] as const,
  inboxGroup: (
    projectIds: string[],
    lane: AgentSidebarAttentionLane,
    archivedOnly: boolean,
    search = "",
    publicationStates: AgentSidebarPublicationState[] = [],
    pinnedConversationIds: string[] = [],
    priorityConversationIds: string[] = [],
    sort: AgentSidebarSort = "latest"
  ) =>
    [
      ...agentSidebarConversationKeys.all,
      "inbox",
      lane,
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

/**
 * Invalidate every sidebar listing query without cancelling one that is already
 * running.
 *
 * TanStack Query v5 defaults `refetchQueries` to `cancelRefetch: true`, so a
 * plain `invalidateQueries` aborts and restarts any in-flight fetch. The sidebar
 * listing takes minutes on a large database and the publication poll invalidates
 * every 5s, so the default turns a slow query into one that can never finish.
 *
 * `cancelRefetch: false` alone is not enough: when a fetch is already running it
 * is simply awaited, and its success clears `isInvalidated` — so the cache can
 * settle on a payload that predates the change that triggered us. One trailing
 * pass closes that hole. It is a single pass, not a loop, because the 5s drift
 * poll is the backstop.
 */
export async function invalidateAgentSidebarConversations(
  queryClient: QueryClient,
): Promise<void> {
  const filters = { queryKey: agentSidebarConversationKeys.all };
  const wasFetching = queryClient.isFetching(filters) > 0;
  await queryClient.invalidateQueries(filters, { cancelRefetch: false });
  if (!wasFetching) return;
  await queryClient.invalidateQueries(filters, { cancelRefetch: false });
}
