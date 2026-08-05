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
