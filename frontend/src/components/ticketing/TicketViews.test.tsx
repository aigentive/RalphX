import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import type { TicketSummary } from "@/api/ticketing";

import { resolveTicketKanbanMove } from "./ticketing-kanban-utils";
import { TicketListView } from "./TicketViews";

const tickets: TicketSummary[] = [
  {
    ref: { provider: "jira", id: "10001", key: "RX-1" },
    title: "First ticket",
    state: { id: "todo", name: "To Do", category: "todo" },
    labels: [],
    updatedAt: "2026-06-19T22:00:00.000Z",
    url: null,
    associationCount: 0,
  },
  {
    ref: { provider: "jira", id: "10002", key: "RX-2" },
    title: "Second ticket",
    state: { id: "started", name: "In Progress", category: "in_progress" },
    labels: [],
    updatedAt: "2026-06-19T22:01:00.000Z",
    url: null,
    associationCount: 1,
  },
];

describe("TicketListView", () => {
  it("moves focus between ticket rows with arrow keys", () => {
    render(
      <TicketListView
        tickets={tickets}
        hasNextPage={false}
        isFetchingNextPage={false}
        onLoadMore={vi.fn()}
        onSelectTicket={vi.fn()}
      />,
    );

    const first = screen.getByRole("button", { name: /RX-1/ });
    const second = screen.getByRole("button", { name: /RX-2/ });

    first.focus();
    fireEvent.keyDown(first, { key: "ArrowDown" });
    expect(second).toHaveFocus();

    fireEvent.keyDown(second, { key: "ArrowUp" });
    expect(first).toHaveFocus();
  });
});

describe("resolveTicketKanbanMove", () => {
  it("returns the moved ticket and destination column when the drop changes state", () => {
    const move = resolveTicketKanbanMove(
      "jira:10001",
      "done",
      tickets,
      [
        { id: "todo", name: "To Do", category: "todo", order: 0 },
        { id: "done", name: "Done", category: "done", order: 1 },
      ],
    );

    expect(move?.ticket.ref.id).toBe("10001");
    expect(move?.column.id).toBe("done");
  });

  it("ignores drops on the current column", () => {
    const move = resolveTicketKanbanMove(
      "jira:10001",
      "todo",
      tickets,
      [{ id: "todo", name: "To Do", category: "todo", order: 0 }],
    );

    expect(move).toBeNull();
  });
});
