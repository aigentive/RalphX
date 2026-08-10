import { beforeEach, describe, expect, it, vi } from "vitest";

vi.unmock("@tauri-apps/api/core");

const { primitiveInvoke } = vi.hoisted(() => ({
  primitiveInvoke: vi.fn<(cmd: string, args?: unknown) => Promise<unknown>>(),
}));

vi.mock("#tauri-core-primitive", async (importOriginal) => {
  const actual = await importOriginal<Record<string, unknown>>();
  return { ...actual, invoke: primitiveInvoke };
});

import { ticketingApi } from "@/api/ticketing";
import { LOCAL_ENVIRONMENT_ID, useEnvironmentStore } from "@/stores/environmentStore";

function remoteCall(index: number): Record<string, unknown> {
  const input = (primitiveInvoke.mock.calls[index]?.[1] as { input?: unknown } | undefined)
    ?.input;
  expect(input).toBeTypeOf("object");
  return input as Record<string, unknown>;
}

beforeEach(() => {
  primitiveInvoke.mockReset();
  useEnvironmentStore.setState({
    activeEnvironmentId: "remote-1",
    environments: [
      { id: LOCAL_ENVIRONMENT_ID, name: "This Mac", kind: "local" },
      { id: "remote-1", name: "Host Mac", kind: "remote" },
    ],
  });
});

describe("remote ticketing reads", () => {
  it("routes all five audited reads through the real remote_invoke input envelope", async () => {
    primitiveInvoke.mockImplementation(async (_transport, raw) => {
      const command = (raw as { input: { cmd: string } }).input.cmd;
      if (command === "get_ticket_associations") {
        // The REAL serialization: TicketAssociationsResponse is #[serde(rename_all =
        // "camelCase")] with eight arrays plus fetchedAt — not a two-field snake_case object.
        return {
          outcome: "ok",
          result: {
            tasks: [],
            proposals: [],
            sessions: [],
            conversations: [],
            pullRequests: [],
            checks: [],
            qa: [],
            specs: [],
            fetchedAt: "2026-08-05T12:00:00Z",
          },
        };
      }
      if (command === "get_conversation_ticket") {
        return { outcome: "ok", result: null };
      }
      if (command === "refresh_tickets") {
        // camelCase on the wire, same reason.
        return { outcome: "ok", result: { refreshedAt: "2026-08-05T12:00:00Z" } };
      }
      return { outcome: "ok", result: [] };
    });

    await ticketingApi.listProviders({ projectId: "project-1" });
    await ticketingApi.listStatusCatalog({ provider: "linear", scopeKind: "team", scopeId: "team-1" });
    await ticketingApi.getTicketAssociations({
      provider: "linear",
      ticketRef: { provider: "linear", id: "issue-1", key: "ENG-1" },
      projectId: "project-1",
    });
    await ticketingApi.getConversationTicket("conversation-1");
    await ticketingApi.refreshTickets({ provider: "linear" });

    const expected = [
      ["list_ticketing_providers", { projectId: "project-1" }],
      ["list_ticketing_status_catalog", { provider: "linear", scopeKind: "team", scopeId: "team-1" }],
      ["get_ticket_associations", { provider: "linear", ticketRef: { provider: "linear", id: "issue-1", key: "ENG-1" }, projectId: "project-1" }],
      ["get_conversation_ticket", { conversationId: "conversation-1" }],
      ["refresh_tickets", { provider: "linear" }],
    ] as const;
    expected.forEach(([command, payload], index) => {
      expect(primitiveInvoke.mock.calls[index]?.[0]).toBe("remote_invoke");
      expect(remoteCall(index).cmd).toBe(command);
      expect(remoteCall(index).args).toEqual(payload);
      expect(remoteCall(index).id).toBe("remote-1");
      expect(remoteCall(index).requestId).toEqual(expect.any(String));
    });
  });

  it("routes all six credential-spending reads through literal remote commands", async () => {
    primitiveInvoke.mockImplementation(async (_transport, raw) => {
      const command = (raw as { input: { cmd: string } }).input.cmd;
      if (command === "list_tickets") {
        return { outcome: "ok", result: { items: [], nextCursor: null, total: 0 } };
      }
      if (command === "list_ticket_filter_options") {
        return { outcome: "ok", result: { assignees: [], labels: [], states: [], truncated: false } };
      }
      if (command === "get_ticket_detail") {
        return {
          outcome: "ok",
          result: {
            ref: { provider: "linear", id: "issue-1", key: "ENG-1" },
            title: "Ticket",
            state: { id: "todo", name: "Todo", category: "todo" },
            labels: [],
            comments: [],
            transitions: [],
            updatedAt: "2026-08-05T12:00:00Z",
            associationCount: 0,
            openPrCount: 0,
          },
        };
      }
      return { outcome: "ok", result: [] };
    });

    const ticketRef = { provider: "linear" as const, id: "issue-1", key: "ENG-1" };
    await ticketingApi.listContainers({ provider: "linear", projectId: "project-1" });
    await ticketingApi.listTickets({ provider: "linear", projectId: "project-1" });
    await ticketingApi.listTicketFilterOptions({ provider: "linear", projectId: "project-1" });
    await ticketingApi.getTicketDetail({ provider: "linear", ticketRef });
    await ticketingApi.listTicketTransitions({ provider: "linear", ticketRef });
    await ticketingApi.listTicketLabels({ provider: "linear", ticketRef });

    const commands = [
      "list_ticketing_containers",
      "list_tickets",
      "list_ticket_filter_options",
      "get_ticket_detail",
      "list_ticket_transitions",
      "list_ticket_labels",
    ];
    commands.forEach((command, index) => {
      expect(primitiveInvoke.mock.calls[index]?.[0]).toBe("remote_invoke");
      expect(remoteCall(index).cmd).toBe(command);
      expect(remoteCall(index).id).toBe("remote-1");
      expect(remoteCall(index).requestId).toEqual(expect.any(String));
    });
    expect(remoteCall(0).args).toEqual({ provider: "linear", projectId: "project-1" });
    expect(remoteCall(1).args).toEqual({ query: { provider: "linear", projectId: "project-1" } });
    expect(remoteCall(2).args).toEqual({ query: { provider: "linear", projectId: "project-1" } });
    expect(remoteCall(3).args).toEqual({ provider: "linear", ticketRef });
    expect(remoteCall(4).args).toEqual({ provider: "linear", ticketRef });
    expect(remoteCall(5).args).toEqual({ provider: "linear", ticketRef });
  });

  it("surfaces a credential-spending read commandError without wrapping it", async () => {
    primitiveInvoke.mockResolvedValue({ outcome: "commandError", error: "provider denied" });
    await expect(
      ticketingApi.listContainers({ provider: "linear", projectId: "project-1" }),
    ).rejects.toBe("provider denied");
  });

  it("surfaces the command's own unwrapped commandError value", async () => {
    primitiveInvoke.mockResolvedValue({
      outcome: "commandError",
      error: "status catalog unavailable",
    });

    await expect(
      ticketingApi.listStatusCatalog({ provider: "linear", scopeKind: "team", scopeId: "team-1" }),
    ).rejects.toBe("status catalog unavailable");
  });
});

describe("remote ticketing writes", () => {
  it("routes all eight writes through literal remote commands and real input envelopes", async () => {
    primitiveInvoke.mockResolvedValue({ outcome: "ok", result: [] });
    const scope = { provider: "linear" as const, scopeKind: "team", scopeId: "team-1" };
    const ticketRef = { provider: "linear" as const, id: "issue-1", key: "ENG-1" };

    await ticketingApi.listColumns({ provider: "linear" });
    await ticketingApi.refreshStatusCatalog(scope);
    await ticketingApi.updateStatusPresentation({ ...scope, patches: [] });
    primitiveInvoke.mockResolvedValue({
      outcome: "ok",
      result: {
        ticketRef,
        operation: {
          id: "operation-1",
          operation: "assign",
          clientOperationId: "client-operation-1",
          status: "succeeded",
          linked: true,
          createdAt: "2026-08-05T12:00:00Z",
          updatedAt: "2026-08-05T12:00:00Z",
        },
        idempotent: false,
        refreshedAt: "2026-08-05T12:00:00Z",
      },
    });
    await ticketingApi.transitionTicketStatus({ provider: "linear", ticketRef, toStateId: "done" });
    await ticketingApi.assignTicket({ provider: "linear", ticketRef });
    await ticketingApi.clearTicketAssignee({ provider: "linear", ticketRef });
    await ticketingApi.addTicketComment({ provider: "linear", ticketRef, bodyMarkdown: "hello" });
    await ticketingApi.setTicketLabels({ provider: "linear", ticketRef, labels: ["bug"] });

    const commands = [
      "list_ticketing_columns",
      "refresh_ticketing_status_catalog",
      "update_ticketing_status_presentation",
      "transition_ticket_status",
      "assign_ticket",
      "clear_ticket_assignee",
      "add_ticket_comment",
      "set_ticket_labels",
    ];
    commands.forEach((command, index) => {
      expect(primitiveInvoke.mock.calls[index]?.[0]).toBe("remote_invoke");
      expect(remoteCall(index).cmd).toBe(command);
      expect(remoteCall(index).id).toBe("remote-1");
      expect(remoteCall(index).requestId).toEqual(expect.any(String));
    });
    expect(remoteCall(0).args).toEqual({ provider: "linear" });
    expect(remoteCall(1).args).toEqual(scope);
    expect(remoteCall(2).args).toEqual({ input: { ...scope, patches: [] } });
    expect(remoteCall(3).args).toEqual({ input: { provider: "linear", ticketRef, toStateId: "done" } });
    expect(remoteCall(4).args).toEqual({ input: { provider: "linear", ticketRef } });
    expect(remoteCall(5).args).toEqual({ input: { provider: "linear", ticketRef } });
    expect(remoteCall(6).args).toEqual({ input: { provider: "linear", ticketRef, bodyMarkdown: "hello" } });
    expect(remoteCall(7).args).toEqual({ input: { provider: "linear", ticketRef, labels: ["bug"] } });
  });

  it("surfaces a mutation commandError instead of silently succeeding", async () => {
    primitiveInvoke.mockResolvedValue({ outcome: "commandError", error: "write rejected" });
    await expect(ticketingApi.assignTicket({
      provider: "linear",
      ticketRef: { provider: "linear", id: "issue-1", key: "ENG-1" },
    })).rejects.toBe("write rejected");
  });
});
