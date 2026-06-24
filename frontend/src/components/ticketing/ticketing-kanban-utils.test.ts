import { describe, expect, it } from "vitest";

import type { TicketingColumn, TicketSummary } from "@/api/ticketing";

import { resolveTicketKanbanMove, ticketDragId } from "./ticketing-kanban-utils";

function makeTicket(overrides: Partial<TicketSummary> = {}): TicketSummary {
  return {
    ref: { provider: "linear", id: "issue-1", key: "LIN-1" },
    title: "Wire dashboard sync",
    state: { id: "todo", name: "Todo", category: "todo" },
    labels: [],
    updatedAt: "2026-06-19T22:00:00.000Z",
    associationCount: 0,
    ...overrides,
  };
}

function makeColumn(overrides: Partial<TicketingColumn> = {}): TicketingColumn {
  return { id: "done", name: "Done", category: "done", order: 2, ...overrides };
}

describe("ticketDragId", () => {
  it("encodes provider and id into a stable drag id", () => {
    expect(ticketDragId(makeTicket())).toBe("linear:issue-1");
    expect(ticketDragId(makeTicket({ ref: { provider: "jira", id: "10001" } }))).toBe(
      "jira:10001",
    );
  });
});

describe("resolveTicketKanbanMove", () => {
  const ticket = makeTicket();
  const tickets = [ticket];
  const columns = [makeColumn({ id: "todo", category: "todo", order: 0 }), makeColumn()];

  it("returns null when there is no drop target", () => {
    expect(resolveTicketKanbanMove("linear:issue-1", null, tickets, columns)).toBeNull();
    expect(resolveTicketKanbanMove("linear:issue-1", undefined, tickets, columns)).toBeNull();
  });

  it("returns null when the active ticket cannot be found", () => {
    expect(resolveTicketKanbanMove("linear:missing", "done", tickets, columns)).toBeNull();
  });

  it("returns null when the drop column cannot be found", () => {
    expect(resolveTicketKanbanMove("linear:issue-1", "archived", tickets, columns)).toBeNull();
  });

  it("returns null when the ticket is already in the target column", () => {
    // The ticket's state.id is "todo"; dropping onto the "todo" column is a no-op.
    expect(resolveTicketKanbanMove("linear:issue-1", "todo", tickets, columns)).toBeNull();
  });

  it("returns the ticket and column for a valid cross-column move", () => {
    const move = resolveTicketKanbanMove("linear:issue-1", "done", tickets, columns);
    expect(move).not.toBeNull();
    expect(move?.ticket).toBe(ticket);
    expect(move?.column.id).toBe("done");
  });
});
