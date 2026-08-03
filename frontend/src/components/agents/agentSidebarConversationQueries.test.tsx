import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { renderHook, waitFor } from "@testing-library/react";
import type { ReactNode } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import type { AgentSidebarConversationGroupsResponse } from "@/api/chat";
import {
  AGENT_SIDEBAR_GROUP_PAGE_SIZE,
  useAgentSidebarGroup,
} from "./agentSidebarConversationQueries";

const { listAgentSidebarConversations } = vi.hoisted(() => ({
  listAgentSidebarConversations: vi.fn(),
}));

vi.mock("@/api/chat", () => ({
  chatApi: {
    listAgentSidebarConversations,
  },
}));

function responseWithRow(key: string): AgentSidebarConversationGroupsResponse {
  return {
    groups: [
      {
        key,
        label: "",
        total: 1,
        offset: 0,
        limit: AGENT_SIDEBAR_GROUP_PAGE_SIZE,
        hasMore: false,
        rows: [
          {
            conversation: { id: "conversation-1" },
            workspace: null,
            refKind: "branch",
            refLabel: "main",
            publicationState: "active",
            publicationLabel: null,
          },
        ] as AgentSidebarConversationGroupsResponse["groups"][number]["rows"],
      },
    ],
  };
}

function wrapper({ children }: { children: ReactNode }) {
  const queryClient = new QueryClient({
    defaultOptions: {
      queries: { retry: false },
    },
  });

  return <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>;
}

function useTestGroup({
  priorityConversationIds,
  minimumRowCount,
}: {
  priorityConversationIds: string[];
  minimumRowCount: number;
}) {
  return useAgentSidebarGroup({
    groupBy: "inbox",
    groupKey: "attention",
    projectIds: ["project-1"],
    archivedOnly: false,
    search: "",
    publicationStates: ["active"],
    pinnedConversationIds: [],
    priorityConversationIds,
    sort: "latest",
    enabled: true,
    minimumRowCount,
    queryKey: ["test-sidebar-group", priorityConversationIds],
  });
}

describe("useAgentSidebarGroup", () => {
  beforeEach(() => {
    listAgentSidebarConversations.mockReset();
  });

  it("keeps prior rows visible immediately after priority ids re-key the query", async () => {
    listAgentSidebarConversations
      .mockResolvedValueOnce(responseWithRow("attention"))
      .mockImplementationOnce(() => new Promise(() => {}));

    const { result, rerender } = renderHook(useTestGroup, {
      initialProps: { priorityConversationIds: ["priority-1"], minimumRowCount: 0 },
      wrapper,
    });

    await waitFor(() => expect(result.current.group.rows).toHaveLength(1));

    rerender({ priorityConversationIds: ["priority-2"], minimumRowCount: 0 });

    expect(result.current.group.rows).not.toHaveLength(0);
  });

  it("keeps prior rows visible immediately after the initial limit re-keys the query", async () => {
    listAgentSidebarConversations
      .mockResolvedValueOnce(responseWithRow("attention"))
      .mockImplementationOnce(() => new Promise(() => {}));

    const { result, rerender } = renderHook(useTestGroup, {
      initialProps: { priorityConversationIds: [], minimumRowCount: 0 },
      wrapper,
    });

    await waitFor(() => expect(result.current.group.rows).toHaveLength(1));

    rerender({ priorityConversationIds: [], minimumRowCount: 24 });

    expect(result.current.group.rows).not.toHaveLength(0);
  });
});
