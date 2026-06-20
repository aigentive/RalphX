import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, renderHook, waitFor } from "@testing-library/react";
import { createElement, type ReactNode } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { ticketingApi } from "@/api/ticketing";

import {
  flattenTicketPages,
  ticketingKeys,
  useStartWorkFromTicket,
  useTicketingProviders,
  useTickets,
} from "./useTicketing";

vi.mock("@/api/ticketing", async (importActual) => {
  const actual = await importActual<typeof import("@/api/ticketing")>();
  return {
    ...actual,
    ticketingApi: {
      listProviders: vi.fn(),
      listContainers: vi.fn(),
      listColumns: vi.fn(),
      listTickets: vi.fn(),
      getTicketDetail: vi.fn(),
      listTicketTransitions: vi.fn(),
      getTicketAssociations: vi.fn(),
      startWorkFromTicket: vi.fn(),
      refreshTickets: vi.fn(),
    },
  };
});

function createQueryClient() {
  return new QueryClient({
    defaultOptions: {
      queries: { retry: false, gcTime: 0 },
    },
  });
}

function createWrapper(queryClient = createQueryClient()) {

  return function Wrapper({ children }: { children: ReactNode }) {
    return createElement(QueryClientProvider, { client: queryClient }, children);
  };
}

describe("useTicketing hooks", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("loads provider summaries when enabled", async () => {
    vi.mocked(ticketingApi.listProviders).mockResolvedValueOnce([
      {
        provider: "jira",
        label: "Jira",
        enabled: true,
        connectionStatus: "connected",
        capabilities: {
          supportsBoards: true,
          supportsKanban: true,
          kanbanWrite: false,
          statusWrite: false,
          assignmentWrite: false,
          commentWrite: false,
          freshness: "manual",
        },
        fetchedAt: "2026-06-19T22:00:00.000Z",
      },
    ]);

    const { result } = renderHook(
      () => useTicketingProviders("project-1", { enabled: true }),
      { wrapper: createWrapper() },
    );

    await waitFor(() => expect(result.current.isSuccess).toBe(true));

    expect(ticketingApi.listProviders).toHaveBeenCalledWith({
      projectId: "project-1",
    });
    expect(result.current.data?.[0]?.provider).toBe("jira");
  });

  it("threads nextCursor through the infinite ticket query", async () => {
    vi.mocked(ticketingApi.listTickets)
      .mockResolvedValueOnce({
        items: [
          {
            ref: { provider: "jira", id: "10001", key: "RX-1" },
            title: "First ticket",
            state: { id: "todo", name: "To Do", category: "todo" },
            labels: [],
            updatedAt: "2026-06-19T22:00:00.000Z",
            url: null,
            associationCount: 0,
          },
        ],
        nextCursor: "cursor-2",
        total: 2,
        fetchedAt: "2026-06-19T22:00:00.000Z",
      })
      .mockResolvedValueOnce({
        items: [
          {
            ref: { provider: "jira", id: "10002", key: "RX-2" },
            title: "Second ticket",
            state: { id: "done", name: "Done", category: "done" },
            labels: [],
            updatedAt: "2026-06-19T22:01:00.000Z",
            url: null,
            associationCount: 1,
          },
        ],
        nextCursor: null,
        total: 2,
        fetchedAt: "2026-06-19T22:01:00.000Z",
      });

    const query = {
      provider: "jira" as const,
      projectId: "project-1",
      containerId: "board-1",
      limit: 1,
    };
    const { result } = renderHook(() => useTickets(query, { enabled: true }), {
      wrapper: createWrapper(),
    });

    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    const nextPageResult = await act(() => result.current.fetchNextPage());

    expect(ticketingApi.listTickets).toHaveBeenNthCalledWith(1, query);
    expect(ticketingApi.listTickets).toHaveBeenNthCalledWith(2, {
      ...query,
      cursor: "cursor-2",
    });
    expect(flattenTicketPages(nextPageResult.data)).toHaveLength(2);
  });

  it("starts RalphX work from a ticket and refreshes association caches", async () => {
    const queryClient = createQueryClient();
    const invalidateSpy = vi.spyOn(queryClient, "invalidateQueries");
    const ticketRef = { provider: "jira" as const, id: "10001", key: "RX-1" };
    vi.mocked(ticketingApi.startWorkFromTicket).mockResolvedValueOnce({
      conversation: {
        id: "conversation-ticket",
        contextType: "project",
        contextId: "project-1",
        title: "RX-1",
        messageCount: 1,
        createdAt: "2026-06-19T22:00:00Z",
        updatedAt: "2026-06-19T22:00:00Z",
        archivedAt: null,
        lastMessageAt: null,
        claudeSessionId: null,
        providerSessionId: null,
        providerHarness: null,
        agentMode: "edit",
      },
      workspace: null,
      sendResult: {
        conversationId: "conversation-ticket",
        agentRunId: "",
        isNewConversation: true,
        wasQueued: true,
        queuedAsPending: false,
        queuedMessageId: "queued-1",
      },
    });
    const { result } = renderHook(() => useStartWorkFromTicket(), {
      wrapper: createWrapper(queryClient),
    });

    await act(() =>
      result.current.mutateAsync({
        projectId: "project-1",
        content: "Start work on RX-1",
        ticketRef,
      }),
    );

    expect(ticketingApi.startWorkFromTicket).toHaveBeenCalledWith({
      projectId: "project-1",
      content: "Start work on RX-1",
      ticketRef,
    });
    expect(invalidateSpy).toHaveBeenCalledWith({
      queryKey: ticketingKeys.associations({
        provider: "jira",
        ticketRef,
        projectId: "project-1",
      }),
    });
    expect(invalidateSpy).toHaveBeenCalledWith({
      queryKey: ticketingKeys.detail({
        provider: "jira",
        ticketRef,
      }),
    });
  });
});
