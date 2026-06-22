import { invoke } from "@tauri-apps/api/core";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { ticketingApi } from "./ticketing";

const capabilities = {
  supportsBoards: true,
  supportsKanban: true,
  kanbanWrite: false,
  statusWrite: false,
  assignmentWrite: false,
  commentWrite: false,
  freshness: "manual",
};

describe("ticketingApi", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("lists providers through the normalized read command", async () => {
    vi.mocked(invoke).mockResolvedValueOnce([
      {
        provider: "jira",
        label: "Jira",
        enabled: true,
        connectionStatus: "connected",
        capabilities,
        fetchedAt: "2026-06-19T22:00:00.000Z",
      },
    ]);

    const providers = await ticketingApi.listProviders({ projectId: "project-1" });

    expect(invoke).toHaveBeenCalledWith("list_ticketing_providers", {
      projectId: "project-1",
    });
    expect(providers).toHaveLength(1);
    expect(providers[0]?.capabilities.supportsKanban).toBe(true);
  });

  it("passes ticket filters and pagination with camelCase invoke args", async () => {
    vi.mocked(invoke).mockResolvedValueOnce({
      items: [],
      nextCursor: "cursor-2",
      total: 24,
      fetchedAt: "2026-06-19T22:00:00.000Z",
    });

    const page = await ticketingApi.listTickets({
      provider: "linear",
      projectId: "project-1",
      containerId: "team-1",
      cursor: "cursor-1",
      limit: 30,
      filters: {
        text: "merge",
        stateIds: ["started"],
        labels: ["backend"],
      },
      sort: "updated_desc",
    });

    expect(invoke).toHaveBeenCalledWith("list_tickets", {
      query: {
        provider: "linear",
        projectId: "project-1",
        containerId: "team-1",
        cursor: "cursor-1",
        limit: 30,
        filters: {
          text: "merge",
          stateIds: ["started"],
          labels: ["backend"],
        },
        sort: "updated_desc",
      },
    });
    expect(page.nextCursor).toBe("cursor-2");
  });

  it("sends status transitions with stable client operation ids", async () => {
    vi.mocked(invoke).mockResolvedValueOnce({
      ticketRef: { provider: "jira", id: "10001", key: "RX-1" },
      operation: {
        id: "operation-1",
        operation: "transition",
        clientOperationId: "client-op-1",
        status: "succeeded",
        providerOperationId: "transition-31",
        linked: true,
        createdAt: "2026-06-19T22:00:00.000Z",
        updatedAt: "2026-06-19T22:00:01.000Z",
      },
      idempotent: false,
      transition: {
        toStateId: "done",
        providerTransitionId: "transition-31",
        name: "Done",
        category: "done",
      },
      comment: null,
      refreshedAt: "2026-06-19T22:00:01.000Z",
    });

    const result = await ticketingApi.transitionTicketStatus({
      provider: "jira",
      ticketRef: { provider: "jira", id: "10001", key: "RX-1" },
      toStateId: "done",
      providerTransitionId: "transition-31",
      clientOperationId: "client-op-1",
      projectId: "project-1",
    });

    expect(invoke).toHaveBeenCalledWith("transition_ticket_status", {
      input: {
        provider: "jira",
        ticketRef: { provider: "jira", id: "10001", key: "RX-1" },
        toStateId: "done",
        providerTransitionId: "transition-31",
        clientOperationId: "client-op-1",
        projectId: "project-1",
      },
    });
    expect(result.operation.clientOperationId).toBe("client-op-1");
  });

  it("sends assign-to-me and comment writes through mutation commands", async () => {
    vi.mocked(invoke)
      .mockResolvedValueOnce({
        ticketRef: { provider: "linear", id: "LIN-1", key: "ENG-1" },
        operation: {
          id: "operation-assign",
          operation: "assign",
          clientOperationId: "assign-op",
          status: "succeeded",
          linked: false,
          createdAt: "2026-06-19T22:00:00.000Z",
          updatedAt: "2026-06-19T22:00:01.000Z",
        },
        idempotent: false,
        transition: null,
        comment: null,
        refreshedAt: "2026-06-19T22:00:01.000Z",
      })
      .mockResolvedValueOnce({
        ticketRef: { provider: "linear", id: "LIN-1", key: "ENG-1" },
        operation: {
          id: "operation-comment",
          operation: "comment",
          clientOperationId: "comment-op",
          status: "succeeded",
          providerOperationId: "comment-1",
          linked: false,
          createdAt: "2026-06-19T22:00:02.000Z",
          updatedAt: "2026-06-19T22:00:03.000Z",
        },
        idempotent: false,
        transition: null,
        comment: {
          id: "comment-1",
          author: { name: "RalphX" },
          bodyMarkdown: "Pushed a fix.",
          bodyText: "Pushed a fix.",
          createdAt: "2026-06-19T22:00:03.000Z",
          updatedAt: "2026-06-19T22:00:03.000Z",
        },
        refreshedAt: "2026-06-19T22:00:03.000Z",
      });

    await ticketingApi.assignTicket({
      provider: "linear",
      ticketRef: { provider: "linear", id: "LIN-1", key: "ENG-1" },
      clientOperationId: "assign-op",
    });
    await ticketingApi.addTicketComment({
      provider: "linear",
      ticketRef: { provider: "linear", id: "LIN-1", key: "ENG-1" },
      bodyMarkdown: "Pushed a fix.",
      clientOperationId: "comment-op",
    });

    expect(invoke).toHaveBeenNthCalledWith(1, "assign_ticket", {
      input: {
        provider: "linear",
        ticketRef: { provider: "linear", id: "LIN-1", key: "ENG-1" },
        clientOperationId: "assign-op",
      },
    });
    expect(invoke).toHaveBeenNthCalledWith(2, "add_ticket_comment", {
      input: {
        provider: "linear",
        ticketRef: { provider: "linear", id: "LIN-1", key: "ENG-1" },
        bodyMarkdown: "Pushed a fix.",
        clientOperationId: "comment-op",
      },
    });
  });

  it("starts RalphX work from a ticket with the shared start payload shape", async () => {
    vi.mocked(invoke).mockResolvedValueOnce({
      conversation: {
        id: "conversation-ticket",
        context_type: "project",
        context_id: "project-1",
        claude_session_id: null,
        provider_session_id: null,
        provider_harness: null,
        agent_mode: "edit",
        title: "RX-1",
        message_count: 1,
        last_message_at: null,
        created_at: "2026-06-19T22:00:00Z",
        updated_at: "2026-06-19T22:00:00Z",
        archived_at: null,
      },
      workspace: null,
      send_result: {
        conversation_id: "conversation-ticket",
        agent_run_id: "",
        is_new_conversation: true,
        was_queued: true,
        queued_as_pending: false,
        queued_message_id: "queued-1",
      },
    });

    const result = await ticketingApi.startWorkFromTicket({
      projectId: "project-1",
      content: "Start work on RX-1",
      ticketRef: { provider: "jira", id: "10001", key: "RX-1" },
      providerHarness: "codex",
      modelId: "gpt-5.5",
      logicalEffort: "high",
      composerIntegrationReferences: [
        {
          provider: "atlassian",
          kind: "jira",
          id: "10001",
          key: "RX-1",
        },
      ],
    });

    expect(invoke).toHaveBeenCalledWith("start_ralphx_work_from_ticket", {
      input: {
        projectId: "project-1",
        content: "Start work on RX-1",
        ticketRef: { provider: "jira", id: "10001", key: "RX-1" },
        providerHarness: "codex",
        modelOverride: "gpt-5.5",
        logicalEffort: "high",
        composerIntegrationReferences: [
          {
            provider: "atlassian",
            kind: "jira",
            id: "10001",
            key: "RX-1",
          },
        ],
      },
    });
    expect(result.conversation.id).toBe("conversation-ticket");
    expect(result.sendResult.wasQueued).toBe(true);
  });

  it("lists containers and omits projectId when undefined", async () => {
    vi.mocked(invoke).mockResolvedValueOnce([
      { provider: "jira", id: "board-1", name: "Board 1", kind: "board" },
    ]);

    const containers = await ticketingApi.listContainers({ provider: "jira" });

    expect(invoke).toHaveBeenCalledWith("list_ticketing_containers", {
      provider: "jira",
    });
    expect(containers[0]?.id).toBe("board-1");
    // kind defaults are preserved and ticketCount stays optional
    expect(containers[0]?.ticketCount ?? null).toBeNull();
  });

  it("lists containers with projectId when provided", async () => {
    vi.mocked(invoke).mockResolvedValueOnce([]);

    await ticketingApi.listContainers({ provider: "linear", projectId: "project-9" });

    expect(invoke).toHaveBeenCalledWith("list_ticketing_containers", {
      provider: "linear",
      projectId: "project-9",
    });
  });

  it("lists columns and threads containerId only when present", async () => {
    vi.mocked(invoke).mockResolvedValueOnce([
      { id: "todo", name: "To Do", category: "todo", order: 0 },
    ]);

    const columns = await ticketingApi.listColumns({
      provider: "jira",
      containerId: "board-1",
    });

    expect(invoke).toHaveBeenCalledWith("list_ticketing_columns", {
      provider: "jira",
      containerId: "board-1",
    });
    expect(columns[0]?.category).toBe("todo");
  });

  it("lists columns without containerId when omitted", async () => {
    vi.mocked(invoke).mockResolvedValueOnce([]);

    await ticketingApi.listColumns({ provider: "jira" });

    expect(invoke).toHaveBeenCalledWith("list_ticketing_columns", {
      provider: "jira",
    });
  });

  it("reads ticket detail including the new openPrStatus and labels fields", async () => {
    vi.mocked(invoke).mockResolvedValueOnce({
      ref: { provider: "jira", id: "10001", key: "RX-1" },
      title: "Fix merge race",
      state: { id: "todo", name: "To Do", category: "todo" },
      labels: ["bug", "backend"],
      updatedAt: "2026-06-19T22:00:00.000Z",
      url: "https://jira/RX-1",
      associationCount: 2,
      openPrCount: 1,
      openPrNumber: 42,
      openPrUrl: "https://github.com/x/y/pull/42",
      openPrStatus: "open",
      descriptionMarkdown: "Investigate transition race.",
      comments: [],
      attachments: [],
      transitions: [],
    });

    const detail = await ticketingApi.getTicketDetail({
      provider: "jira",
      ticketRef: { provider: "jira", id: "10001", key: "RX-1" },
    });

    expect(invoke).toHaveBeenCalledWith("get_ticket_detail", {
      provider: "jira",
      ticketRef: { provider: "jira", id: "10001", key: "RX-1" },
    });
    expect(detail.openPrStatus).toBe("open");
    expect(detail.openPrNumber).toBe(42);
    expect(detail.labels).toEqual(["bug", "backend"]);
  });

  it("defaults summary array/number fields when the backend omits them", async () => {
    vi.mocked(invoke).mockResolvedValueOnce({
      ref: { provider: "linear", id: "LIN-1" },
      title: "No defaults",
      state: { id: "todo", name: "Todo", category: "todo" },
      updatedAt: "2026-06-19T22:00:00.000Z",
      // labels, associationCount, openPrCount intentionally omitted
      comments: [],
      attachments: [],
      transitions: [],
    });

    const detail = await ticketingApi.getTicketDetail({
      provider: "linear",
      ticketRef: { provider: "linear", id: "LIN-1" },
    });

    expect(detail.labels).toEqual([]);
    expect(detail.associationCount).toBe(0);
    expect(detail.openPrCount).toBe(0);
    expect(detail.openPrStatus ?? null).toBeNull();
  });

  it("lists ticket transitions including disabledReason passthrough", async () => {
    vi.mocked(invoke).mockResolvedValueOnce([
      {
        toStateId: "blocked",
        providerTransitionId: "transition-99",
        name: "Blocked",
        category: "other",
        disabledReason: "Workflow blocks this transition.",
      },
    ]);

    const transitions = await ticketingApi.listTicketTransitions({
      provider: "jira",
      ticketRef: { provider: "jira", id: "10001", key: "RX-1" },
    });

    expect(invoke).toHaveBeenCalledWith("list_ticket_transitions", {
      provider: "jira",
      ticketRef: { provider: "jira", id: "10001", key: "RX-1" },
    });
    expect(transitions[0]?.disabledReason).toBe("Workflow blocks this transition.");
  });

  it("reads ticket associations and parses deepLink projectId", async () => {
    vi.mocked(invoke).mockResolvedValueOnce({
      tasks: [
        {
          id: "task-1",
          title: "Linked task",
          active: true,
          deepLink: { view: "kanban", id: "task-1", projectId: "project-1" },
        },
      ],
      pullRequests: [
        {
          id: "pr-42",
          title: "Fix race",
          status: "open",
          deepLink: { view: "kanban", id: "pr-42" },
        },
      ],
      fetchedAt: "2026-06-19T22:00:00.000Z",
    });

    const associations = await ticketingApi.getTicketAssociations({
      provider: "jira",
      ticketRef: { provider: "jira", id: "10001", key: "RX-1" },
      projectId: "project-1",
    });

    expect(invoke).toHaveBeenCalledWith("get_ticket_associations", {
      provider: "jira",
      ticketRef: { provider: "jira", id: "10001", key: "RX-1" },
      projectId: "project-1",
    });
    expect(associations.tasks[0]?.deepLink.projectId).toBe("project-1");
    // Defaulted empty arrays for omitted association buckets.
    expect(associations.proposals).toEqual([]);
    expect(associations.specs).toEqual([]);
    // projectId defaults to undefined when the backend omits it.
    expect(associations.pullRequests[0]?.deepLink.projectId ?? null).toBeNull();
  });

  it("reads the conversation ticket and tolerates a null response", async () => {
    vi.mocked(invoke).mockResolvedValueOnce({
      ticketRef: { provider: "jira", id: "10001", key: "RX-1" },
      projectId: "project-1",
      title: "RX-1",
      url: "https://jira/RX-1",
    });

    const ticket = await ticketingApi.getConversationTicket("conversation-1");

    expect(invoke).toHaveBeenCalledWith("get_conversation_ticket", {
      conversationId: "conversation-1",
    });
    expect(ticket?.projectId).toBe("project-1");

    vi.mocked(invoke).mockResolvedValueOnce(null);
    const missing = await ticketingApi.getConversationTicket("conversation-2");
    expect(missing).toBeNull();
  });

  it("refreshes tickets and parses the explicit refreshedAt response", async () => {
    vi.mocked(invoke).mockResolvedValueOnce({
      refreshedAt: "2026-06-19T22:05:00.000Z",
    });

    const result = await ticketingApi.refreshTickets({
      provider: "jira",
      containerId: "board-1",
    });

    expect(invoke).toHaveBeenCalledWith("refresh_tickets", {
      provider: "jira",
      containerId: "board-1",
    });
    expect(result.refreshedAt).toBe("2026-06-19T22:05:00.000Z");
  });

  it("synthesizes refreshedAt when refresh returns void/null", async () => {
    vi.mocked(invoke).mockResolvedValueOnce(null);

    const result = await ticketingApi.refreshTickets({ provider: "linear" });

    expect(invoke).toHaveBeenCalledWith("refresh_tickets", {
      provider: "linear",
    });
    // Falls back to the TauriVoidSchema transform that fills a fresh timestamp.
    expect(typeof result.refreshedAt).toBe("string");
    expect(Number.isNaN(Date.parse(result.refreshedAt))).toBe(false);
  });

  it("clears the assignee through the clear command", async () => {
    vi.mocked(invoke).mockResolvedValueOnce({
      ticketRef: { provider: "linear", id: "LIN-1", key: "ENG-1" },
      operation: {
        id: "operation-clear",
        operation: "assign",
        clientOperationId: "clear-op",
        status: "succeeded",
        linked: false,
        createdAt: "2026-06-19T22:00:00.000Z",
        updatedAt: "2026-06-19T22:00:01.000Z",
      },
      idempotent: false,
      assignee: null,
      refreshedAt: "2026-06-19T22:00:01.000Z",
    });

    await ticketingApi.clearTicketAssignee({
      provider: "linear",
      ticketRef: { provider: "linear", id: "LIN-1", key: "ENG-1" },
      clientOperationId: "clear-op",
      projectId: "project-1",
    });

    expect(invoke).toHaveBeenCalledWith("clear_ticket_assignee", {
      input: {
        provider: "linear",
        ticketRef: { provider: "linear", id: "LIN-1", key: "ENG-1" },
        clientOperationId: "clear-op",
        projectId: "project-1",
      },
    });
  });

  it("sets ticket labels and parses the normalized labels response", async () => {
    vi.mocked(invoke).mockResolvedValueOnce({
      ticketRef: { provider: "jira", id: "10001", key: "RX-1" },
      operation: {
        id: "operation-labels",
        operation: "transition",
        clientOperationId: "labels-op",
        status: "succeeded",
        linked: false,
        createdAt: "2026-06-19T22:00:00.000Z",
        updatedAt: "2026-06-19T22:00:01.000Z",
      },
      idempotent: false,
      labels: { labels: ["bug", "frontend"] },
      refreshedAt: "2026-06-19T22:00:01.000Z",
    });

    const result = await ticketingApi.setTicketLabels({
      provider: "jira",
      ticketRef: { provider: "jira", id: "10001", key: "RX-1" },
      labels: ["bug", "frontend"],
      clientOperationId: "labels-op",
    });

    expect(invoke).toHaveBeenCalledWith("set_ticket_labels", {
      input: {
        provider: "jira",
        ticketRef: { provider: "jira", id: "10001", key: "RX-1" },
        labels: ["bug", "frontend"],
        clientOperationId: "labels-op",
      },
    });
    expect(result.labels?.labels).toEqual(["bug", "frontend"]);
  });

  it("lists available label options for a ticket", async () => {
    vi.mocked(invoke).mockResolvedValueOnce([
      { id: "label-1", name: "bug" },
      { name: "frontend" },
    ]);

    const labels = await ticketingApi.listTicketLabels({
      provider: "jira",
      ticketRef: { provider: "jira", id: "10001", key: "RX-1" },
    });

    expect(invoke).toHaveBeenCalledWith("list_ticket_labels", {
      provider: "jira",
      ticketRef: { provider: "jira", id: "10001", key: "RX-1" },
    });
    expect(labels.map((l) => l.name)).toEqual(["bug", "frontend"]);
    expect(labels[1]?.id ?? null).toBeNull();
  });

  it("omits optional clientOperationId/projectId from mutation payloads", async () => {
    const mutationResponse = {
      ticketRef: { provider: "jira", id: "10001", key: "RX-1" },
      operation: {
        id: "operation-x",
        operation: "assign",
        clientOperationId: "",
        status: "succeeded",
        linked: false,
        createdAt: "2026-06-19T22:00:00.000Z",
        updatedAt: "2026-06-19T22:00:01.000Z",
      },
      idempotent: false,
      refreshedAt: "2026-06-19T22:00:01.000Z",
    };
    vi.mocked(invoke)
      .mockResolvedValueOnce(mutationResponse)
      .mockResolvedValueOnce(mutationResponse)
      .mockResolvedValueOnce(mutationResponse);

    const ref = { provider: "jira" as const, id: "10001", key: "RX-1" };
    await ticketingApi.assignTicket({ provider: "jira", ticketRef: ref });
    await ticketingApi.addTicketComment({
      provider: "jira",
      ticketRef: ref,
      bodyMarkdown: "note",
    });
    await ticketingApi.setTicketLabels({
      provider: "jira",
      ticketRef: ref,
      labels: ["bug"],
    });

    expect(invoke).toHaveBeenNthCalledWith(1, "assign_ticket", {
      input: { provider: "jira", ticketRef: ref },
    });
    expect(invoke).toHaveBeenNthCalledWith(2, "add_ticket_comment", {
      input: { provider: "jira", ticketRef: ref, bodyMarkdown: "note" },
    });
    expect(invoke).toHaveBeenNthCalledWith(3, "set_ticket_labels", {
      input: { provider: "jira", ticketRef: ref, labels: ["bug"] },
    });
  });
});
