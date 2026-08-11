import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { renderHook, waitFor } from "@testing-library/react";
import type { ReactNode } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import type { AgentSidebarConversationGroupsResponse } from "@/api/chat";
import {
  AGENT_SIDEBAR_GROUP_PAGE_SIZE,
  agentSidebarConversationKeys,
  useAgentSidebarAutomationGroup,
  useAgentSidebarAutomationGroupIndex,
  useAgentSidebarProjectGroup,
  useAgentSidebarPublicationGroup,
} from "./useAgentSidebarPublicationGroup";

const { listAgentSidebarConversations } = vi.hoisted(() => ({
  listAgentSidebarConversations: vi.fn(),
}));

vi.mock("@/api/chat", () => ({
  chatApi: {
    listAgentSidebarConversations,
  },
}));

function responseForGroup(key: string): AgentSidebarConversationGroupsResponse {
  return {
    groups: [
      {
        key,
        label: "",
        total: 0,
        offset: 0,
        limit: AGENT_SIDEBAR_GROUP_PAGE_SIZE,
        hasMore: false,
        rows: [],
      },
    ],
  };
}

function responseForPagedGroup(
  key: string,
  offset: number,
  rowCount: number,
  hasMore: boolean
): AgentSidebarConversationGroupsResponse {
  return {
    groups: [
      {
        key,
        label: "",
        total: hasMore ? offset + rowCount + 1 : offset + rowCount,
        offset,
        limit: rowCount,
        hasMore,
        rows: Array.from({ length: rowCount }, (_, index) => ({
          conversation: { id: `${key}-${offset + index}` },
          workspace: null,
          refKind: "branch",
          refLabel: "main",
          publicationState: "active",
          publicationLabel: null,
        })) as AgentSidebarConversationGroupsResponse["groups"][number]["rows"],
      },
    ],
  };
}

function wrapper({ children }: { children: ReactNode }) {
  const queryClient = new QueryClient({
    defaultOptions: {
      queries: {
        retry: false,
      },
    },
  });

  return <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>;
}

describe("useAgentSidebarPublicationGroup", () => {
  beforeEach(() => {
    listAgentSidebarConversations.mockReset();
  });

  it("requests eight rows per project group page", async () => {
    listAgentSidebarConversations.mockResolvedValueOnce(responseForGroup("project-1"));

    const { result } = renderHook(
      () =>
        useAgentSidebarProjectGroup({
          projectId: "project-1",
          archivedOnly: false,
          search: "",
          publicationStates: ["active"],
          pinnedConversationIds: [],
        }),
      { wrapper }
    );

    await waitFor(() => expect(result.current.isSuccess).toBe(true));

    expect(AGENT_SIDEBAR_GROUP_PAGE_SIZE).toBe(8);
    expect(listAgentSidebarConversations).toHaveBeenCalledWith(
      expect.objectContaining({
        groupBy: "project",
        limitPerGroup: 8,
        offsets: { "project-1": 0 },
        projectIds: ["project-1"],
      })
    );
  });

  it("requests the standalone pseudo-group with an empty project list", async () => {
    listAgentSidebarConversations.mockResolvedValueOnce(
      responseForGroup("__no_project__"),
    );

    const { result } = renderHook(
      () =>
        useAgentSidebarProjectGroup({
          projectId: "__no_project__",
          archivedOnly: false,
          search: "",
          publicationStates: ["active"],
          pinnedConversationIds: [],
        }),
      { wrapper },
    );

    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    expect(listAgentSidebarConversations).toHaveBeenCalledWith(
      expect.objectContaining({
        groupBy: "project",
        offsets: { __no_project__: 0 },
        projectIds: [],
      }),
    );
    expect(
      agentSidebarConversationKeys.noProjectGroup(false, "", ["active"]),
    ).toEqual(
      expect.arrayContaining(["project", "__no_project__", "states", ["active"]]),
    );
  });

  it("requests eight rows per publication-state group page", async () => {
    listAgentSidebarConversations.mockResolvedValueOnce(responseForGroup("active"));

    const { result } = renderHook(
      () =>
        useAgentSidebarPublicationGroup({
          projectIds: ["project-1"],
          publicationState: "active",
          archivedOnly: false,
          search: "",
          pinnedConversationIds: [],
          sort: "latest",
        }),
      { wrapper }
    );

    await waitFor(() => expect(result.current.isSuccess).toBe(true));

    expect(listAgentSidebarConversations).toHaveBeenCalledWith(
      expect.objectContaining({
        groupBy: "publication",
        limitPerGroup: 8,
        offsets: { active: 0 },
        projectIds: ["project-1"],
        publicationStates: ["active"],
      })
    );
  });

  it("partitions automation group keys by group key, filters, pinned ids, priority ids, and sort", () => {
    expect(
      agentSidebarConversationKeys.automationIndex(
        ["project-1"],
        true,
        " Build ",
        ["active"],
        ["pinned-1"],
        ["priority-1"],
        "za"
      )
    ).toEqual([
      "agents",
      "sidebar-conversations",
      "automation",
      "index",
      "projects",
      ["project-1"],
      "archived",
      true,
      "search",
      "build",
      "states",
      ["active"],
      "pinned",
      ["pinned-1"],
      "priority",
      ["priority-1"],
      "sort",
      "za",
    ]);
    expect(
      agentSidebarConversationKeys.automationGroup(
        "automation-1",
        ["project-1"],
        true,
        " Build ",
        ["active"],
        ["pinned-1"],
        ["priority-1"],
        "za"
      )
    ).toEqual([
      "agents",
      "sidebar-conversations",
      "automation",
      "automation-1",
      "projects",
      ["project-1"],
      "archived",
      true,
      "search",
      "build",
      "states",
      ["active"],
      "pinned",
      ["pinned-1"],
      "priority",
      ["priority-1"],
      "sort",
      "za",
    ]);
  });

  it("requests one row per automation group for the dynamic group index", async () => {
    listAgentSidebarConversations.mockResolvedValueOnce(responseForGroup("automation-1"));

    const { result } = renderHook(
      () =>
        useAgentSidebarAutomationGroupIndex({
          projectIds: ["project-1", "project-2"],
          archivedOnly: true,
          search: " release ",
          publicationStates: ["active", "merged"],
          pinnedConversationIds: ["pinned-1"],
          priorityConversationIds: ["priority-1"],
          sort: "az",
        }),
      { wrapper }
    );

    await waitFor(() => expect(result.current.isSuccess).toBe(true));

    expect(result.current.data?.[0]?.key).toBe("automation-1");
    expect(listAgentSidebarConversations).toHaveBeenCalledWith(
      expect.objectContaining({
        groupBy: "automation",
        limitPerGroup: 1,
        projectIds: ["project-1", "project-2"],
        includeArchived: true,
        archivedOnly: true,
        search: "release",
        publicationStates: ["active", "merged"],
        pinnedConversationIds: ["pinned-1"],
        priorityConversationIds: ["priority-1"],
        sort: "az",
      })
    );
  });

  it("requests per-automation group pages with stable backend group keys", async () => {
    listAgentSidebarConversations.mockResolvedValueOnce(responseForGroup("automation-1"));

    const { result } = renderHook(
      () =>
        useAgentSidebarAutomationGroup({
          groupKey: "automation-1",
          projectIds: ["project-1", "project-2"],
          archivedOnly: true,
          search: " release ",
          publicationStates: ["active", "merged"],
          pinnedConversationIds: ["pinned-1"],
          priorityConversationIds: ["priority-1"],
          sort: "za",
        }),
      { wrapper }
    );

    await waitFor(() => expect(result.current.isSuccess).toBe(true));

    expect(listAgentSidebarConversations).toHaveBeenCalledWith(
      expect.objectContaining({
        groupBy: "automation",
        limitPerGroup: 8,
        offsets: { "automation-1": 0 },
        projectIds: ["project-1", "project-2"],
        includeArchived: true,
        archivedOnly: true,
        search: "release",
        publicationStates: ["active", "merged"],
        pinnedConversationIds: ["pinned-1"],
        priorityConversationIds: ["priority-1"],
        sort: "za",
      })
    );
  });

  it("uses remembered row depth for the initial project page and normal page size afterward", async () => {
    listAgentSidebarConversations
      .mockResolvedValueOnce(responseForPagedGroup("project-1", 0, 24, true))
      .mockResolvedValueOnce(responseForPagedGroup("project-1", 24, 8, false));

    const { result } = renderHook(
      () =>
        useAgentSidebarProjectGroup({
          projectId: "project-1",
          archivedOnly: false,
          search: "",
          publicationStates: ["active"],
          pinnedConversationIds: [],
          minimumRowCount: 24,
        }),
      { wrapper }
    );

    await waitFor(() => expect(result.current.group.rows).toHaveLength(24));

    expect(listAgentSidebarConversations).toHaveBeenNthCalledWith(
      1,
      expect.objectContaining({
        limitPerGroup: 24,
        offsets: { "project-1": 0 },
      })
    );

    await result.current.fetchNextPage();

    await waitFor(() => expect(result.current.group.rows).toHaveLength(32));
    expect(listAgentSidebarConversations).toHaveBeenNthCalledWith(
      2,
      expect.objectContaining({
        limitPerGroup: 8,
        offsets: { "project-1": 24 },
      })
    );
  });

  it("uses adaptive page size for initial and subsequent project group pages", async () => {
    listAgentSidebarConversations
      .mockResolvedValueOnce(responseForPagedGroup("project-1", 0, 18, true))
      .mockResolvedValueOnce(responseForPagedGroup("project-1", 18, 18, false));

    const { result } = renderHook(
      () =>
        useAgentSidebarProjectGroup({
          projectId: "project-1",
          archivedOnly: false,
          search: "",
          publicationStates: ["active"],
          pinnedConversationIds: [],
          pageSize: 18,
        }),
      { wrapper }
    );

    await waitFor(() => expect(result.current.group.rows).toHaveLength(18));

    expect(listAgentSidebarConversations).toHaveBeenNthCalledWith(
      1,
      expect.objectContaining({
        limitPerGroup: 18,
        offsets: { "project-1": 0 },
      })
    );

    await result.current.fetchNextPage();

    await waitFor(() => expect(result.current.group.rows).toHaveLength(36));
    expect(listAgentSidebarConversations).toHaveBeenNthCalledWith(
      2,
      expect.objectContaining({
        limitPerGroup: 18,
        offsets: { "project-1": 18 },
      })
    );
  });

  it("refetches offset zero when remembered depth increases beyond fresh page-one cache", async () => {
    listAgentSidebarConversations
      .mockResolvedValueOnce(responseForPagedGroup("project-1", 0, 8, true))
      .mockResolvedValueOnce(responseForPagedGroup("project-1", 0, 24, true));

    const { result, rerender } = renderHook(
      ({ minimumRowCount }: { minimumRowCount: number }) =>
        useAgentSidebarProjectGroup({
          projectId: "project-1",
          archivedOnly: false,
          search: "",
          publicationStates: ["active"],
          pinnedConversationIds: [],
          minimumRowCount,
        }),
      {
        initialProps: { minimumRowCount: 0 },
        wrapper,
      }
    );

    await waitFor(() => expect(result.current.group.rows).toHaveLength(8));

    rerender({ minimumRowCount: 24 });

    await waitFor(() => expect(result.current.group.rows).toHaveLength(24));
    expect(listAgentSidebarConversations).toHaveBeenNthCalledWith(
      2,
      expect.objectContaining({
        limitPerGroup: 24,
        offsets: { "project-1": 0 },
      })
    );
  });
});
