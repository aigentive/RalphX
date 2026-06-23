import { QueryClient, QueryClientProvider, type InfiniteData } from "@tanstack/react-query";
import { act, renderHook, waitFor } from "@testing-library/react";
import { createElement, type ReactNode } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { ticketingApi } from "@/api/ticketing";
import type {
  TicketDetail,
  TicketPage,
  TicketRef,
  TicketSummary,
  TicketTransitionOption,
} from "@/api/ticketing";

import {
  createTicketClientOperationId,
  fetchTicketTransitionsForMove,
  findTicketTransitionForColumn,
  flattenTicketPages,
  ticketingKeys,
  useConversationTicket,
  useRefreshTickets,
  useTicketAssociations,
  useTicketDetail,
  useTicketingColumns,
  useTicketingContainers,
  useTicketingMutations,
  useTicketLabelOptions,
  useTicketTransitions,
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
      setTicketLabels: vi.fn(),
      listTicketLabels: vi.fn(),
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

  it("sets labels with the full new array, a clientOperationId, and optimistic patch", async () => {
    const ticketRef: TicketRef = { provider: "jira", id: "10001", key: "RX-1" };
    const detail: TicketDetail = {
      ref: ticketRef,
      title: "Fix merge race",
      state: { id: "todo", name: "To Do", category: "todo" },
      labels: ["bug"],
      updatedAt: "2026-06-19T22:00:00.000Z",
      url: null,
      associationCount: 0,
      descriptionMarkdown: "Investigate transition race.",
      comments: [],
      attachments: [],
      transitions: [],
    };
    vi.mocked(ticketingApi.setTicketLabels).mockResolvedValueOnce({
      ticketRef,
      operation: {
        id: "operation-1",
        operation: "set_labels",
        clientOperationId: "generated",
        status: "succeeded",
        providerOperationId: null,
        linked: true,
        createdAt: "2026-06-19T22:00:00.000Z",
        updatedAt: "2026-06-19T22:00:01.000Z",
      },
      idempotent: false,
      labels: { labels: ["bug", "frontend"] },
      refreshedAt: "2026-06-19T22:00:01.000Z",
    });

    const harness = createWrapper();
    harness.queryClient.setQueryData(ticketingKeys.detail({ provider: "jira", ticketRef }), detail);
    const { result } = renderHook(() => useTicketingMutations("project-1"), {
      wrapper: harness.wrapper,
    });

    await act(async () => {
      await result.current.setLabels({
        provider: "jira",
        ticketRef,
        labels: ["bug", "frontend"],
      });
    });

    expect(ticketingApi.setTicketLabels).toHaveBeenCalledWith(
      expect.objectContaining({
        provider: "jira",
        ticketRef,
        labels: ["bug", "frontend"],
        projectId: "project-1",
        clientOperationId: expect.stringMatching(/^ticketing:set-labels:/),
      }),
    );
    expect(
      harness.queryClient.getQueryData<TicketDetail>(
        ticketingKeys.detail({ provider: "jira", ticketRef }),
      )?.labels,
    ).toEqual(["bug", "frontend"]);
  });

  it("rolls back optimistic label updates when the label mutation fails", async () => {
    const ticketRef: TicketRef = { provider: "jira", id: "10001", key: "RX-1" };
    const detail: TicketDetail = {
      ref: ticketRef,
      title: "Fix merge race",
      state: { id: "todo", name: "To Do", category: "todo" },
      labels: ["bug"],
      updatedAt: "2026-06-19T22:00:00.000Z",
      url: null,
      associationCount: 0,
      descriptionMarkdown: "Investigate transition race.",
      comments: [],
      attachments: [],
      transitions: [],
    };
    vi.mocked(ticketingApi.setTicketLabels).mockRejectedValueOnce(new Error("Label write failed"));

    const harness = createWrapper();
    harness.queryClient.setQueryData(ticketingKeys.detail({ provider: "jira", ticketRef }), detail);
    const { result } = renderHook(() => useTicketingMutations("project-1"), {
      wrapper: harness.wrapper,
    });

    await expect(
      act(() =>
        result.current.setLabels({
          provider: "jira",
          ticketRef,
          labels: ["bug", "frontend"],
        }),
      ),
    ).rejects.toThrow("Label write failed");

    expect(
      harness.queryClient.getQueryData<TicketDetail>(
        ticketingKeys.detail({ provider: "jira", ticketRef }),
      )?.labels,
    ).toEqual(["bug"]);
  });

  it("invalidates ticket caches after a label mutation settles", async () => {
    const ticketRef: TicketRef = { provider: "jira", id: "10001", key: "RX-1" };
    vi.mocked(ticketingApi.setTicketLabels).mockResolvedValueOnce({
      ticketRef,
      operation: {
        id: "operation-1",
        operation: "set_labels",
        clientOperationId: "generated",
        status: "succeeded",
        providerOperationId: null,
        linked: true,
        createdAt: "2026-06-19T22:00:00.000Z",
        updatedAt: "2026-06-19T22:00:01.000Z",
      },
      idempotent: false,
      labels: { labels: ["bug"] },
      refreshedAt: "2026-06-19T22:00:01.000Z",
    });

    const harness = createWrapper();
    const invalidateSpy = vi.spyOn(harness.queryClient, "invalidateQueries");
    const { result } = renderHook(() => useTicketingMutations("project-1"), {
      wrapper: harness.wrapper,
    });

    await act(async () => {
      await result.current.setLabels({ provider: "jira", ticketRef, labels: ["bug"] });
    });

    expect(invalidateSpy).toHaveBeenCalledWith({
      queryKey: ticketingKeys.detail({ provider: "jira", ticketRef }),
    });
  });

  it("builds query keys with null fallbacks for optional segments", () => {
    expect(ticketingKeys.providers()).toEqual(["ticketing", "providers", null]);
    expect(ticketingKeys.containers({ provider: "jira" })).toEqual([
      "ticketing",
      "containers",
      "jira",
      null,
    ]);
    expect(ticketingKeys.columns({ provider: "linear" })).toEqual([
      "ticketing",
      "columns",
      "linear",
      null,
    ]);
    // detail.key falls back to null when absent
    expect(
      ticketingKeys.detail({ provider: "jira", ticketRef: { provider: "jira", id: "10001" } }),
    ).toEqual(["ticketing", "detail", "jira", "10001", null]);
    // tickets key threads filters/sort/limit nulls
    expect(ticketingKeys.tickets({ provider: "jira" })).toEqual([
      "ticketing",
      "tickets",
      "jira",
      null,
      null,
      null,
      null,
      null,
    ]);
    expect(ticketingKeys.conversationTicket("conv-1")).toEqual([
      "ticketing",
      "conversation-ticket",
      "conv-1",
    ]);
  });

  it("creates a deterministic-prefixed client operation id with a crypto uuid", () => {
    const id = createTicketClientOperationId("transition", {
      provider: "jira",
      id: "10001",
      key: "RX-1",
    });
    expect(id).toMatch(/^ticketing:transition:jira:RX-1:/);

    // Falls back to id when key is absent.
    const idNoKey = createTicketClientOperationId("comment", {
      provider: "linear",
      id: "LIN-9",
    });
    expect(idNoKey).toMatch(/^ticketing:comment:linear:LIN-9:/);
  });

  it("generates a non-crypto client operation id when randomUUID is unavailable", () => {
    const originalCrypto = globalThis.crypto;
    Object.defineProperty(globalThis, "crypto", {
      value: { ...originalCrypto, randomUUID: undefined },
      configurable: true,
    });
    try {
      const id = createTicketClientOperationId("assign", {
        provider: "jira",
        id: "10001",
        key: "RX-1",
      });
      expect(id).toMatch(/^ticketing:assign:jira:RX-1:\d+-/);
    } finally {
      Object.defineProperty(globalThis, "crypto", {
        value: originalCrypto,
        configurable: true,
      });
    }
  });

  it("loads ticketing containers and threads the provider through the read command", async () => {
    vi.mocked(ticketingApi.listContainers).mockResolvedValueOnce([
      { provider: "jira", id: "board-1", name: "Board 1", kind: "board" },
    ]);

    const { result } = renderHook(
      () => useTicketingContainers({ provider: "jira", projectId: "project-1" }),
      { wrapper: createWrapper().wrapper },
    );

    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    expect(ticketingApi.listContainers).toHaveBeenCalledWith({
      provider: "jira",
      projectId: "project-1",
    });
    expect(result.current.data?.[0]?.id).toBe("board-1");
  });

  it("keeps the containers query disabled when input is null", () => {
    const { result } = renderHook(() => useTicketingContainers(null), {
      wrapper: createWrapper().wrapper,
    });
    expect(result.current.fetchStatus).toBe("idle");
    expect(ticketingApi.listContainers).not.toHaveBeenCalled();
  });

  it("loads ticketing columns for a container", async () => {
    vi.mocked(ticketingApi.listColumns).mockResolvedValueOnce([
      { id: "todo", name: "To Do", category: "todo", order: 0 },
    ]);

    const { result } = renderHook(
      () => useTicketingColumns({ provider: "jira", containerId: "board-1" }),
      { wrapper: createWrapper().wrapper },
    );

    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    expect(ticketingApi.listColumns).toHaveBeenCalledWith({
      provider: "jira",
      containerId: "board-1",
    });
    expect(result.current.data?.[0]?.category).toBe("todo");
  });

  it("keeps the columns query disabled when input is null", () => {
    const { result } = renderHook(() => useTicketingColumns(null), {
      wrapper: createWrapper().wrapper,
    });
    expect(result.current.fetchStatus).toBe("idle");
    expect(ticketingApi.listColumns).not.toHaveBeenCalled();
  });

  it("loads label options for a ticket", async () => {
    vi.mocked(ticketingApi.listTicketLabels).mockResolvedValueOnce([
      { id: "label-1", name: "bug" },
      { name: "frontend" },
    ]);

    const ticketRef: TicketRef = { provider: "jira", id: "10001", key: "RX-1" };
    const { result } = renderHook(
      () => useTicketLabelOptions({ provider: "jira", ticketRef }),
      { wrapper: createWrapper().wrapper },
    );

    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    expect(ticketingApi.listTicketLabels).toHaveBeenCalledWith({
      provider: "jira",
      ticketRef,
    });
    expect(result.current.data?.map((label) => label.name)).toEqual(["bug", "frontend"]);
  });

  it("keeps the label-options query disabled when input is null", () => {
    const { result } = renderHook(() => useTicketLabelOptions(null), {
      wrapper: createWrapper().wrapper,
    });
    expect(result.current.fetchStatus).toBe("idle");
    expect(ticketingApi.listTicketLabels).not.toHaveBeenCalled();
  });

  it("loads ticket detail when a ticket id is present", async () => {
    const ticketRef: TicketRef = { provider: "jira", id: "10001", key: "RX-1" };
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
      transitions: [],
    };
    vi.mocked(ticketingApi.getTicketDetail).mockResolvedValueOnce(detail);

    const { result } = renderHook(
      () => useTicketDetail({ provider: "jira", ticketRef }),
      { wrapper: createWrapper().wrapper },
    );

    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    expect(ticketingApi.getTicketDetail).toHaveBeenCalledWith({ provider: "jira", ticketRef });
    expect(result.current.data?.title).toBe("Fix merge race");
  });

  it("keeps the ticket-detail query disabled when input is null", () => {
    const { result } = renderHook(() => useTicketDetail(null), {
      wrapper: createWrapper().wrapper,
    });
    expect(result.current.fetchStatus).toBe("idle");
    expect(ticketingApi.getTicketDetail).not.toHaveBeenCalled();
  });

  it("loads ticket transitions", async () => {
    const ticketRef: TicketRef = { provider: "jira", id: "10001", key: "RX-1" };
    vi.mocked(ticketingApi.listTicketTransitions).mockResolvedValueOnce([
      { toStateId: "done", name: "Done", category: "done" },
    ]);

    const { result } = renderHook(
      () => useTicketTransitions({ provider: "jira", ticketRef }),
      { wrapper: createWrapper().wrapper },
    );

    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    expect(ticketingApi.listTicketTransitions).toHaveBeenCalledWith({ provider: "jira", ticketRef });
    expect(result.current.data?.[0]?.toStateId).toBe("done");
  });

  it("keeps the transitions query disabled when input is null", () => {
    const { result } = renderHook(() => useTicketTransitions(null), {
      wrapper: createWrapper().wrapper,
    });
    expect(result.current.fetchStatus).toBe("idle");
    expect(ticketingApi.listTicketTransitions).not.toHaveBeenCalled();
  });

  it("loads ticket associations when projectId and ticket id are present", async () => {
    const ticketRef: TicketRef = { provider: "jira", id: "10001", key: "RX-1" };
    vi.mocked(ticketingApi.getTicketAssociations).mockResolvedValueOnce({
      tasks: [],
      proposals: [],
      sessions: [],
      conversations: [],
      pullRequests: [],
      checks: [],
      qa: [],
      specs: [],
    });

    const { result } = renderHook(
      () => useTicketAssociations({ provider: "jira", ticketRef, projectId: "project-1" }),
      { wrapper: createWrapper().wrapper },
    );

    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    expect(ticketingApi.getTicketAssociations).toHaveBeenCalledWith({
      provider: "jira",
      ticketRef,
      projectId: "project-1",
    });
    expect(result.current.data?.tasks).toEqual([]);
  });

  it("keeps the associations query disabled when input is null", () => {
    const { result } = renderHook(() => useTicketAssociations(null), {
      wrapper: createWrapper().wrapper,
    });
    expect(result.current.fetchStatus).toBe("idle");
    expect(ticketingApi.getTicketAssociations).not.toHaveBeenCalled();
  });

  it("loads the conversation ticket binding when a conversation id is present", async () => {
    const ticketRef: TicketRef = { provider: "jira", id: "10001", key: "RX-1" };
    vi.mocked(ticketingApi.getConversationTicket).mockResolvedValueOnce({
      ticketRef,
      projectId: "project-1",
      title: "RX-1",
      url: null,
    });

    const { result } = renderHook(() => useConversationTicket("conversation-1"), {
      wrapper: createWrapper().wrapper,
    });

    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    expect(ticketingApi.getConversationTicket).toHaveBeenCalledWith("conversation-1");
    expect(result.current.data?.projectId).toBe("project-1");
  });

  it("keeps the conversation-ticket query disabled when id is null", () => {
    const { result } = renderHook(() => useConversationTicket(null), {
      wrapper: createWrapper().wrapper,
    });
    expect(result.current.fetchStatus).toBe("idle");
    expect(ticketingApi.getConversationTicket).not.toHaveBeenCalled();
  });

  it("refreshes tickets and invalidates the whole ticketing namespace", async () => {
    const queryClient = createQueryClient();
    const invalidateSpy = vi.spyOn(queryClient, "invalidateQueries");
    vi.mocked(ticketingApi.refreshTickets).mockResolvedValueOnce({
      refreshedAt: "2026-06-19T22:05:00.000Z",
    });

    const { result } = renderHook(() => useRefreshTickets(), {
      wrapper: createWrapper(queryClient).wrapper,
    });

    await act(() => result.current.mutateAsync({ provider: "jira", containerId: "board-1" }));

    expect(ticketingApi.refreshTickets).toHaveBeenCalledWith({
      provider: "jira",
      containerId: "board-1",
    });
    expect(invalidateSpy).toHaveBeenCalledWith({ queryKey: ticketingKeys.all });
  });

  it("assigns the ticket to me optimistically and patches the detail cache", async () => {
    const ticketRef: TicketRef = { provider: "jira", id: "10001", key: "RX-1" };
    const detail: TicketDetail = {
      ref: ticketRef,
      title: "Fix merge race",
      state: { id: "todo", name: "To Do", category: "todo" },
      assignee: null,
      labels: [],
      updatedAt: "2026-06-19T22:00:00.000Z",
      url: null,
      associationCount: 0,
      descriptionMarkdown: "Investigate transition race.",
      comments: [],
      attachments: [],
      transitions: [],
    };
    vi.mocked(ticketingApi.assignTicket).mockResolvedValueOnce({
      ticketRef,
      operation: {
        id: "operation-assign",
        operation: "assign",
        clientOperationId: "generated",
        status: "succeeded",
        providerOperationId: null,
        linked: true,
        createdAt: "2026-06-19T22:00:00.000Z",
        updatedAt: "2026-06-19T22:00:01.000Z",
      },
      idempotent: false,
      assignee: { id: "me", name: "Me" },
      comment: null,
      refreshedAt: "2026-06-19T22:00:01.000Z",
    });

    const harness = createWrapper();
    harness.queryClient.setQueryData(ticketingKeys.detail({ provider: "jira", ticketRef }), detail);
    const { result } = renderHook(() => useTicketingMutations("project-1"), {
      wrapper: harness.wrapper,
    });

    await act(async () => {
      await result.current.assignToMe({ provider: "jira", ticketRef });
    });

    expect(ticketingApi.assignTicket).toHaveBeenCalledWith(
      expect.objectContaining({
        provider: "jira",
        ticketRef,
        projectId: "project-1",
        clientOperationId: expect.stringMatching(/^ticketing:assign:/),
      }),
    );
    expect(
      harness.queryClient.getQueryData<TicketDetail>(
        ticketingKeys.detail({ provider: "jira", ticketRef }),
      )?.assignee?.name,
    ).toBe("Me");
  });

  it("rolls back optimistic assign-to-me when the assign mutation fails", async () => {
    const ticketRef: TicketRef = { provider: "jira", id: "10001", key: "RX-1" };
    const detail: TicketDetail = {
      ref: ticketRef,
      title: "Fix merge race",
      state: { id: "todo", name: "To Do", category: "todo" },
      assignee: { id: "orig", name: "Original" },
      labels: [],
      updatedAt: "2026-06-19T22:00:00.000Z",
      url: null,
      associationCount: 0,
      descriptionMarkdown: "Investigate transition race.",
      comments: [],
      attachments: [],
      transitions: [],
    };
    vi.mocked(ticketingApi.assignTicket).mockRejectedValueOnce(new Error("Assign failed"));

    const harness = createWrapper();
    harness.queryClient.setQueryData(ticketingKeys.detail({ provider: "jira", ticketRef }), detail);
    const { result } = renderHook(() => useTicketingMutations("project-1"), {
      wrapper: harness.wrapper,
    });

    await expect(
      act(() => result.current.assignToMe({ provider: "jira", ticketRef })),
    ).rejects.toThrow("Assign failed");

    expect(
      harness.queryClient.getQueryData<TicketDetail>(
        ticketingKeys.detail({ provider: "jira", ticketRef }),
      )?.assignee?.name,
    ).toBe("Original");
  });

  it("appends an optimistic comment then replaces it with the server comment", async () => {
    const ticketRef: TicketRef = { provider: "jira", id: "10001", key: "RX-1" };
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
      transitions: [],
    };
    vi.mocked(ticketingApi.addTicketComment).mockResolvedValueOnce({
      ticketRef,
      operation: {
        id: "operation-comment",
        operation: "comment",
        clientOperationId: "generated",
        status: "succeeded",
        providerOperationId: "comment-1",
        linked: true,
        createdAt: "2026-06-19T22:00:02.000Z",
        updatedAt: "2026-06-19T22:00:03.000Z",
      },
      idempotent: false,
      comment: {
        id: "server-comment-1",
        author: { name: "RalphX" },
        bodyMarkdown: "Pushed a fix.",
        bodyText: "Pushed a fix.",
        createdAt: "2026-06-19T22:00:03.000Z",
        updatedAt: "2026-06-19T22:00:03.000Z",
      },
      refreshedAt: "2026-06-19T22:00:03.000Z",
    });

    const harness = createWrapper();
    harness.queryClient.setQueryData(ticketingKeys.detail({ provider: "jira", ticketRef }), detail);
    const { result } = renderHook(() => useTicketingMutations("project-1"), {
      wrapper: harness.wrapper,
    });

    await act(async () => {
      await result.current.addComment({
        provider: "jira",
        ticketRef,
        bodyMarkdown: "Pushed a fix.",
      });
    });

    expect(ticketingApi.addTicketComment).toHaveBeenCalledWith(
      expect.objectContaining({
        provider: "jira",
        ticketRef,
        bodyMarkdown: "Pushed a fix.",
        projectId: "project-1",
        clientOperationId: expect.stringMatching(/^ticketing:comment:/),
      }),
    );
    const comments = harness.queryClient.getQueryData<TicketDetail>(
      ticketingKeys.detail({ provider: "jira", ticketRef }),
    )?.comments;
    // Optimistic placeholder replaced by the confirmed server comment id.
    expect(comments).toHaveLength(1);
    expect(comments?.[0]?.id).toBe("server-comment-1");
  });

  it("rolls back the optimistic comment when the comment mutation fails", async () => {
    const ticketRef: TicketRef = { provider: "jira", id: "10001", key: "RX-1" };
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
      transitions: [],
    };
    vi.mocked(ticketingApi.addTicketComment).mockRejectedValueOnce(new Error("Comment failed"));

    const harness = createWrapper();
    harness.queryClient.setQueryData(ticketingKeys.detail({ provider: "jira", ticketRef }), detail);
    const { result } = renderHook(() => useTicketingMutations("project-1"), {
      wrapper: harness.wrapper,
    });

    await expect(
      act(() =>
        result.current.addComment({
          provider: "jira",
          ticketRef,
          bodyMarkdown: "Pushed a fix.",
        }),
      ),
    ).rejects.toThrow("Comment failed");

    expect(
      harness.queryClient.getQueryData<TicketDetail>(
        ticketingKeys.detail({ provider: "jira", ticketRef }),
      )?.comments,
    ).toEqual([]);
  });

  it("patches the assignee from the transition response when the backend returns one", async () => {
    const ticketRef: TicketRef = { provider: "jira", id: "10001", key: "RX-1" };
    const transition: TicketTransitionOption = {
      toStateId: "in_progress",
      providerTransitionId: "transition-21",
      name: "In Progress",
      category: "in_progress",
    };
    const detail: TicketDetail = {
      ref: ticketRef,
      title: "Fix merge race",
      state: { id: "todo", name: "To Do", category: "todo" },
      assignee: null,
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
        providerOperationId: "transition-21",
        linked: true,
        createdAt: "2026-06-19T22:00:00.000Z",
        updatedAt: "2026-06-19T22:00:01.000Z",
      },
      idempotent: false,
      transition,
      assignee: { id: "auto", name: "Auto Assignee" },
      comment: null,
      refreshedAt: "2026-06-19T22:00:01.000Z",
    });

    const harness = createWrapper();
    harness.queryClient.setQueryData(ticketingKeys.detail({ provider: "jira", ticketRef }), detail);
    const { result } = renderHook(() => useTicketingMutations("project-1"), {
      wrapper: harness.wrapper,
    });

    await act(async () => {
      await result.current.transitionStatus({ provider: "jira", ticketRef, transition });
    });

    const cached = harness.queryClient.getQueryData<TicketDetail>(
      ticketingKeys.detail({ provider: "jira", ticketRef }),
    );
    expect(cached?.state.name).toBe("In Progress");
    expect(cached?.assignee?.name).toBe("Auto Assignee");
  });

  it("honors an explicit projectId override and an explicit clientOperationId", async () => {
    const ticketRef: TicketRef = { provider: "jira", id: "10001", key: "RX-1" };
    vi.mocked(ticketingApi.assignTicket).mockResolvedValueOnce({
      ticketRef,
      operation: {
        id: "operation-assign",
        operation: "assign",
        clientOperationId: "explicit-op",
        status: "succeeded",
        linked: true,
        createdAt: "2026-06-19T22:00:00.000Z",
        updatedAt: "2026-06-19T22:00:01.000Z",
      },
      idempotent: false,
      assignee: { id: "me", name: "Me" },
      refreshedAt: "2026-06-19T22:00:01.000Z",
    });

    // No hook-level projectId — input-level projectId must win.
    const { result } = renderHook(() => useTicketingMutations(), {
      wrapper: createWrapper().wrapper,
    });

    await act(async () => {
      await result.current.assignToMe({
        provider: "jira",
        ticketRef,
        projectId: "project-override",
        clientOperationId: "explicit-op",
      });
    });

    expect(ticketingApi.assignTicket).toHaveBeenCalledWith(
      expect.objectContaining({
        projectId: "project-override",
        clientOperationId: "explicit-op",
      }),
    );
  });

  it("invalidates the project-scoped associations cache after start-work", async () => {
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
        wasQueued: false,
        queuedAsPending: false,
        queuedMessageId: null,
      },
    });
    const { result } = renderHook(() => useStartWorkFromTicket(), {
      wrapper: createWrapper(queryClient).wrapper,
    });

    await act(() =>
      result.current.mutateAsync({
        projectId: "project-1",
        content: "Start work",
        ticketRef,
      }),
    );

    // The ticket-list invalidation key is project + provider scoped.
    expect(invalidateSpy).toHaveBeenCalledWith({
      queryKey: ["ticketing", "tickets", "jira", "project-1"],
    });
  });

  it("optimistically patches the matching ticket in cached ticket-list pages", async () => {
    const ticketRef: TicketRef = { provider: "jira", id: "10001", key: "RX-1" };
    const otherRef: TicketRef = { provider: "jira", id: "20002", key: "RX-2" };
    const summary = (ref: TicketRef, name: string): TicketSummary => ({
      ref,
      title: `Ticket ${ref.id}`,
      state: { id: "todo", name, category: "todo" },
      labels: [],
      updatedAt: "2026-06-19T22:00:00.000Z",
      url: null,
      associationCount: 0,
    });
    const listKey = ticketingKeys.tickets({ provider: "jira", projectId: "project-1" });
    const pages: InfiniteData<TicketPage> = {
      pages: [
        {
          items: [summary(ticketRef, "To Do"), summary(otherRef, "To Do")],
          nextCursor: null,
          total: 2,
        },
      ],
      pageParams: [null],
    };

    const transition: TicketTransitionOption = {
      toStateId: "done",
      providerTransitionId: "transition-31",
      name: "Done",
      category: "done",
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
    harness.queryClient.setQueryData(listKey, pages);
    const { result } = renderHook(() => useTicketingMutations("project-1"), {
      wrapper: harness.wrapper,
    });

    await act(async () => {
      await result.current.transitionStatus({ provider: "jira", ticketRef, transition });
    });

    const cached = harness.queryClient.getQueryData<InfiniteData<TicketPage>>(listKey);
    const items = cached?.pages[0]?.items ?? [];
    // Only the matching ref is patched; the sibling row keeps its original state.
    expect(items.find((t) => t.ref.id === "10001")?.state.name).toBe("Done");
    expect(items.find((t) => t.ref.id === "20002")?.state.name).toBe("To Do");
  });

  it("restores cached ticket-list pages when the mutation fails", async () => {
    const ticketRef: TicketRef = { provider: "jira", id: "10001", key: "RX-1" };
    const summary: TicketSummary = {
      ref: ticketRef,
      title: "Fix merge race",
      state: { id: "todo", name: "To Do", category: "todo" },
      labels: ["bug"],
      updatedAt: "2026-06-19T22:00:00.000Z",
      url: null,
      associationCount: 0,
    };
    const listKey = ticketingKeys.tickets({ provider: "jira", projectId: "project-1" });
    const pages: InfiniteData<TicketPage> = {
      pages: [{ items: [summary], nextCursor: null, total: 1 }],
      pageParams: [null],
    };
    vi.mocked(ticketingApi.setTicketLabels).mockRejectedValueOnce(new Error("Label write failed"));

    const harness = createWrapper();
    harness.queryClient.setQueryData(listKey, pages);
    const { result } = renderHook(() => useTicketingMutations("project-1"), {
      wrapper: harness.wrapper,
    });

    await expect(
      act(() => result.current.setLabels({ provider: "jira", ticketRef, labels: ["bug", "ui"] })),
    ).rejects.toThrow("Label write failed");

    const restored = harness.queryClient.getQueryData<InfiniteData<TicketPage>>(listKey);
    expect(restored?.pages[0]?.items[0]?.labels).toEqual(["bug"]);
  });

  it("rolls back the optimistic clear-assignee when the clear mutation fails", async () => {
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
    vi.mocked(ticketingApi.clearTicketAssignee).mockRejectedValueOnce(new Error("Clear failed"));

    const harness = createWrapper();
    harness.queryClient.setQueryData(ticketingKeys.detail({ provider: "linear", ticketRef }), detail);
    const { result } = renderHook(() => useTicketingMutations("project-1"), {
      wrapper: harness.wrapper,
    });

    await expect(
      act(() => result.current.clearAssignee({ provider: "linear", ticketRef })),
    ).rejects.toThrow("Clear failed");

    expect(
      harness.queryClient.getQueryData<TicketDetail>(
        ticketingKeys.detail({ provider: "linear", ticketRef }),
      )?.assignee?.name,
    ).toBe("A. User");
  });

  it("leaves a ticket-list summary unchanged when a comment mutation patches it", async () => {
    // Comment patches only apply to detail entries (which carry a comments array);
    // a plain summary row in the ticket-list cache must be left untouched.
    const ticketRef: TicketRef = { provider: "jira", id: "10001", key: "RX-1" };
    const summary: TicketSummary = {
      ref: ticketRef,
      title: "Fix merge race",
      state: { id: "todo", name: "To Do", category: "todo" },
      labels: [],
      updatedAt: "2026-06-19T22:00:00.000Z",
      url: null,
      associationCount: 0,
    };
    const listKey = ticketingKeys.tickets({ provider: "jira", projectId: "project-1" });

    const harness = createWrapper();
    harness.queryClient.setQueryData<InfiniteData<TicketPage>>(listKey, {
      pages: [{ items: [summary], nextCursor: null, total: 1 }],
      pageParams: [null],
    });
    vi.mocked(ticketingApi.addTicketComment).mockResolvedValueOnce({
      ticketRef,
      operation: {
        id: "operation-comment",
        operation: "comment",
        clientOperationId: "generated",
        status: "succeeded",
        providerOperationId: "comment-1",
        linked: true,
        createdAt: "2026-06-19T22:00:02.000Z",
        updatedAt: "2026-06-19T22:00:03.000Z",
      },
      idempotent: false,
      comment: null,
      refreshedAt: "2026-06-19T22:00:03.000Z",
    });
    const { result } = renderHook(() => useTicketingMutations("project-1"), {
      wrapper: harness.wrapper,
    });

    await act(async () => {
      await result.current.addComment({ provider: "jira", ticketRef, bodyMarkdown: "note" });
    });

    const cached = harness.queryClient.getQueryData<InfiniteData<TicketPage>>(listKey);
    const row = cached?.pages[0]?.items[0];
    expect(row).toBeDefined();
    // No comments key leaks onto the summary row.
    expect("comments" in (row as object)).toBe(false);
  });

  it("keeps the optimistic labels when the set-labels response omits labels", async () => {
    const ticketRef: TicketRef = { provider: "jira", id: "10001", key: "RX-1" };
    const detail: TicketDetail = {
      ref: ticketRef,
      title: "Fix merge race",
      state: { id: "todo", name: "To Do", category: "todo" },
      labels: ["bug"],
      updatedAt: "2026-06-19T22:00:00.000Z",
      url: null,
      associationCount: 0,
      descriptionMarkdown: "Investigate transition race.",
      comments: [],
      attachments: [],
      transitions: [],
    };
    vi.mocked(ticketingApi.setTicketLabels).mockResolvedValueOnce({
      ticketRef,
      operation: {
        id: "operation-labels",
        operation: "transition",
        clientOperationId: "generated",
        status: "succeeded",
        linked: true,
        createdAt: "2026-06-19T22:00:00.000Z",
        updatedAt: "2026-06-19T22:00:01.000Z",
      },
      idempotent: false,
      // labels omitted → onSuccess takes the early-return path and keeps the optimistic patch.
      refreshedAt: "2026-06-19T22:00:01.000Z",
    });

    const harness = createWrapper();
    harness.queryClient.setQueryData(ticketingKeys.detail({ provider: "jira", ticketRef }), detail);
    const { result } = renderHook(() => useTicketingMutations("project-1"), {
      wrapper: harness.wrapper,
    });

    await act(async () => {
      await result.current.setLabels({ provider: "jira", ticketRef, labels: ["bug", "ui"] });
    });

    expect(
      harness.queryClient.getQueryData<TicketDetail>(
        ticketingKeys.detail({ provider: "jira", ticketRef }),
      )?.labels,
    ).toEqual(["bug", "ui"]);
  });
});
