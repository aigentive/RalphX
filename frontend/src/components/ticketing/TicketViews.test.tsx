import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import type { TicketSummary } from "@/api/ticketing";

import { resolveTicketKanbanMove } from "./ticketing-kanban-utils";
import { TicketKanbanView, TicketListView } from "./TicketViews";

const tickets: TicketSummary[] = [
  {
    ref: { provider: "jira", id: "10001", key: "RX-1" },
    title: "First ticket",
    state: { id: "todo", name: "To Do", category: "todo" },
    assignee: { name: "Adrian Demian" },
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
  it("renders the assignee name or an unassigned placeholder per row", () => {
    render(
      <TicketListView
        tickets={tickets}
        hasNextPage={false}
        isFetchingNextPage={false}
        onLoadMore={vi.fn()}
        onSelectTicket={vi.fn()}
      />,
    );

    expect(screen.getByText("Adrian Demian")).toBeInTheDocument();
    expect(screen.getByText("Unassigned")).toBeInTheDocument();
  });

  it("shows up to three labels with a +N overflow chip", () => {
    render(
      <TicketListView
        tickets={[
          {
            ref: { provider: "linear", id: "L-9", key: "L-9" },
            title: "Labelled ticket",
            state: { id: "todo", name: "To Do", category: "todo" },
            labels: ["a", "b", "c", "d"],
            updatedAt: "2026-06-19T22:00:00.000Z",
            url: null,
            associationCount: 0,
          },
        ]}
        hasNextPage={false}
        isFetchingNextPage={false}
        onLoadMore={vi.fn()}
        onSelectTicket={vi.fn()}
      />,
    );

    expect(screen.getByText("a")).toBeInTheDocument();
    expect(screen.getByText("c")).toBeInTheDocument();
    expect(screen.queryByText("d")).not.toBeInTheDocument();
    expect(screen.getByText("+1")).toBeInTheDocument();
  });

  it("shows the unread indicator only for rows flagged by isUnread", () => {
    render(
      <TicketListView
        tickets={tickets}
        hasNextPage={false}
        isFetchingNextPage={false}
        onLoadMore={vi.fn()}
        onSelectTicket={vi.fn()}
        isUnread={(ticket) => ticket.ref.id === "10001"}
      />,
    );

    expect(
      screen.getAllByRole("img", { name: /updated since you last opened/i }),
    ).toHaveLength(1);
  });

  it("offers an inline Assign-to-me action for unassigned rows when enabled", () => {
    const onQuickAssign = vi.fn();
    render(
      <TicketListView
        tickets={tickets}
        hasNextPage={false}
        isFetchingNextPage={false}
        onLoadMore={vi.fn()}
        onSelectTicket={vi.fn()}
        canQuickAssign
        onQuickAssign={onQuickAssign}
      />,
    );

    // tickets[0] is assigned (no action); tickets[1] is unassigned (one action).
    const assignButton = screen.getByRole("button", { name: "Assign to me" });
    fireEvent.click(assignButton);
    expect(onQuickAssign).toHaveBeenCalledTimes(1);
  });

  it("hides the inline Assign-to-me action when quick assign is disabled", () => {
    render(
      <TicketListView
        tickets={tickets}
        hasNextPage={false}
        isFetchingNextPage={false}
        onLoadMore={vi.fn()}
        onSelectTicket={vi.fn()}
        canQuickAssign={false}
        onQuickAssign={vi.fn()}
      />,
    );

    expect(screen.queryByRole("button", { name: "Assign to me" })).not.toBeInTheDocument();
  });

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

describe("TicketKanbanView", () => {
  it("renders labels and the assignee on kanban cards", () => {
    render(
      <TicketKanbanView
        columns={[{ id: "todo", name: "To Do", category: "todo", order: 0 }]}
        tickets={[
          {
            ref: { provider: "linear", id: "L-1", key: "L-1" },
            title: "Kanban card ticket",
            state: { id: "todo", name: "To Do", category: "todo" },
            assignee: { name: "Ada Lovelace" },
            labels: ["frontend", "ux"],
            updatedAt: "2026-06-20T10:00:00.000Z",
            url: null,
            associationCount: 0,
          },
        ]}
        onSelectTicket={vi.fn()}
      />,
    );

    expect(screen.getByText("frontend")).toBeInTheDocument();
    expect(screen.getByText("ux")).toBeInTheDocument();
    expect(screen.getByText("Ada Lovelace")).toBeInTheDocument();
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
