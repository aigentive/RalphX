import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { renderHook, waitFor } from "@testing-library/react";
import type { ReactNode } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import type { AgentSidebarConversationGroupsResponse } from "@/api/chat";
import {
  AGENT_SIDEBAR_GROUP_PAGE_SIZE,
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
