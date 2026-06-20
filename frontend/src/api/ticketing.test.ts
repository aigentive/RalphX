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

<<<<<<< HEAD
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
=======
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
>>>>>>> ralphx/ralphx/agent-8e4ac713
  });
});
