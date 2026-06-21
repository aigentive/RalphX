import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, renderHook, waitFor } from "@testing-library/react";
import { createElement, type ReactNode } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { ticketingApi } from "@/api/ticketing";
import type { TicketDetail, TicketRef, TicketTransitionOption } from "@/api/ticketing";

import {
  fetchTicketTransitionsForMove,
  findTicketTransitionForColumn,
  flattenTicketPages,
  ticketingKeys,
  useTicketingMutations,
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
      getConversationTicket: vi.fn(),
      startWorkFromTicket: vi.fn(),
      refreshTickets: vi.fn(),
      transitionTicketStatus: vi.fn(),
      assignTicket: vi.fn(),
      clearTicketAssignee: vi.fn(),
      addTicketComment: vi.fn(),
    },
  };
});

function createQueryClient() {
  return new QueryClient({
    defaultOptions: {
      queries: { retry: false, gcTime: Infinity },
      mutations: { retry: false },
    },
  });
}

function createWrapper(queryClient = createQueryClient()) {
  function Wrapper({ children }: { children: ReactNode }) {
    return createElement(QueryClientProvider, { client: queryClient }, children);
  }

  return { queryClient, wrapper: Wrapper };
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
      { wrapper: createWrapper().wrapper },
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
      wrapper: createWrapper().wrapper,
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

  it("generates clientOperationId and applies optimistic status transitions", async () => {
    const ticketRef: TicketRef = { provider: "jira", id: "10001", key: "RX-1" };
    const transition: TicketTransitionOption = {
      toStateId: "done",
      providerTransitionId: "transition-31",
      name: "Done",
      category: "done",
    };
    const detail: TicketDetail = {
      ref: ticketRef,
      title: "Fix merge race",
      state: { id: "todo", name: "To Do", category: "todo" },
      labels: [],
      updatedAt: "2026-06-19T22:00:00.000Z",
      url: null,
      associationCount: 0,
      descriptionMarkdown: "Investigate transition race.",
      comments: [],
      attachments: [],
      transitions: [transition],
    };
    vi.mocked(ticketingApi.transitionTicketStatus).mockResolvedValueOnce({
      ticketRef,
      operation: {
        id: "operation-1",
        operation: "transition",
        clientOperationId: "generated",
        status: "succeeded",
        providerOperationId: "transition-31",
        linked: true,
        createdAt: "2026-06-19T22:00:00.000Z",
        updatedAt: "2026-06-19T22:00:01.000Z",
      },
      idempotent: false,
      transition,
      comment: null,
      refreshedAt: "2026-06-19T22:00:01.000Z",
    });

    const harness = createWrapper();
    harness.queryClient.setQueryData(ticketingKeys.detail({ provider: "jira", ticketRef }), detail);
    const { result } = renderHook(() => useTicketingMutations("project-1"), {
      wrapper: harness.wrapper,
    });

    await act(async () => {
      await result.current.transitionStatus({
        provider: "jira",
        ticketRef,
        transition,
      });
    });

    expect(ticketingApi.transitionTicketStatus).toHaveBeenCalledWith(
      expect.objectContaining({
        provider: "jira",
        ticketRef,
        toStateId: "done",
        providerTransitionId: "transition-31",
        projectId: "project-1",
        clientOperationId: expect.stringMatching(/^ticketing:transition:/),
      }),
    );
    expect(
      harness.queryClient.getQueryData<TicketDetail>(
        ticketingKeys.detail({ provider: "jira", ticketRef }),
      )?.state.name,
    ).toBe("Done");
  });

  it("prefetches enabled ticket-specific transitions before kanban moves", async () => {
    const ticketRef: TicketRef = { provider: "jira", id: "10001", key: "RX-1" };
    const transition: TicketTransitionOption = {
      toStateId: "done",
      providerTransitionId: "transition-31",
      name: "Done",
      category: "done",
    };
    vi.mocked(ticketingApi.listTicketTransitions).mockResolvedValueOnce([
      {
        toStateId: "blocked",
        providerTransitionId: "transition-99",
        name: "Blocked",
        category: "other",
        disabledReason: "Workflow blocks this transition.",
      },
      transition,
    ]);

    const harness = createWrapper();
    const transitions = await fetchTicketTransitionsForMove(harness.queryClient, {
      provider: "jira",
      ticketRef,
    });

    expect(ticketingApi.listTicketTransitions).toHaveBeenCalledWith({
      provider: "jira",
      ticketRef,
    });
    expect(
      findTicketTransitionForColumn(transitions, {
        id: "blocked",
        name: "Blocked",
        category: "other",
        order: 1,
      }),
    ).toBeNull();
    expect(
      findTicketTransitionForColumn(transitions, {
        id: "done",
        name: "Done",
        category: "done",
        order: 2,
      }),
    ).toEqual(transition);
  });

  it("rolls back optimistic ticket detail updates when status transition fails", async () => {
    const ticketRef: TicketRef = { provider: "jira", id: "10001", key: "RX-1" };
    const transition: TicketTransitionOption = {
      toStateId: "done",
      providerTransitionId: "transition-31",
      name: "Done",
      category: "done",
    };
    const detail: TicketDetail = {
      ref: ticketRef,
      title: "Fix merge race",
      state: { id: "todo", name: "To Do", category: "todo" },
      labels: [],
      updatedAt: "2026-06-19T22:00:00.000Z",
      url: null,
      associationCount: 0,
      descriptionMarkdown: "Investigate transition race.",
      comments: [],
      attachments: [],
      transitions: [transition],
    };
    vi.mocked(ticketingApi.transitionTicketStatus).mockRejectedValueOnce(new Error("Workflow blocked"));

    const harness = createWrapper();
    harness.queryClient.setQueryData(ticketingKeys.detail({ provider: "jira", ticketRef }), detail);
    const { result } = renderHook(() => useTicketingMutations("project-1"), {
      wrapper: harness.wrapper,
    });

    await expect(
      act(() =>
        result.current.transitionStatus({
          provider: "jira",
          ticketRef,
          transition,
        }),
      ),
    ).rejects.toThrow("Workflow blocked");

    expect(
      harness.queryClient.getQueryData<TicketDetail>(
        ticketingKeys.detail({ provider: "jira", ticketRef }),
      )?.state.name,
    ).toBe("To Do");
  });

  it("clears assignee optimistically from ticket detail", async () => {
    const ticketRef: TicketRef = { provider: "linear", id: "LIN-1", key: "LIN-1" };
    const detail: TicketDetail = {
      ref: ticketRef,
      title: "Fix dashboard loading",
      state: { id: "todo", name: "Todo", category: "todo" },
      assignee: { id: "user-1", name: "A. User" },
      labels: [],
      updatedAt: "2026-06-19T22:00:00.000Z",
      url: null,
      associationCount: 0,
      descriptionMarkdown: "Investigate Linear tickets.",
      comments: [],
      attachments: [],
      transitions: [],
    };
    vi.mocked(ticketingApi.clearTicketAssignee).mockResolvedValueOnce({
      ticketRef,
      operation: {
        id: "operation-2",
        operation: "assign",
        clientOperationId: "generated",
        status: "succeeded",
        providerOperationId: null,
        linked: true,
        createdAt: "2026-06-19T22:00:00.000Z",
        updatedAt: "2026-06-19T22:00:01.000Z",
      },
      idempotent: false,
      assignee: null,
      comment: null,
      refreshedAt: "2026-06-19T22:00:01.000Z",
    });

    const harness = createWrapper();
    harness.queryClient.setQueryData(ticketingKeys.detail({ provider: "linear", ticketRef }), detail);
    const { result } = renderHook(() => useTicketingMutations("project-1"), {
      wrapper: harness.wrapper,
    });

    await act(async () => {
      await result.current.clearAssignee({
        provider: "linear",
        ticketRef,
      });
    });

    expect(ticketingApi.clearTicketAssignee).toHaveBeenCalledWith(
      expect.objectContaining({
        provider: "linear",
        ticketRef,
        projectId: "project-1",
        clientOperationId: expect.stringMatching(/^ticketing:clear-assignee:/),
      }),
    );
    expect(
      harness.queryClient.getQueryData<TicketDetail>(
        ticketingKeys.detail({ provider: "linear", ticketRef }),
      )?.assignee,
    ).toBeNull();
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
      wrapper: createWrapper(queryClient).wrapper,
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
