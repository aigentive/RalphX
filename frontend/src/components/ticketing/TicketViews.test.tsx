import { fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import type { TicketSummary } from "@/api/ticketing";

import { TooltipProvider } from "@/components/ui/tooltip";

import { resolveTicketKanbanMove } from "./ticketing-kanban-utils";
import { formatTicketDate } from "./ticketing-utils";
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
    openPrCount: 0,
  },
  {
    ref: { provider: "jira", id: "10002", key: "RX-2" },
    title: "Second ticket",
    state: { id: "started", name: "In Progress", category: "in_progress" },
    labels: [],
    updatedAt: "2026-06-19T22:01:00.000Z",
    url: null,
    associationCount: 1,
    openPrCount: 0,
  },
];

describe("TicketListView", () => {
  it("renders the assignee avatar label or an unassigned placeholder per row", () => {
    render(
      <TicketListView
        tickets={tickets}
        hasNextPage={false}
        isFetchingNextPage={false}
        onLoadMore={vi.fn()}
        onSelectTicket={vi.fn()}
      />,
    );

    expect(screen.getByLabelText("Adrian Demian")).toBeInTheDocument();
    expect(screen.queryByText("Adrian Demian")).not.toBeInTheDocument();
    expect(screen.getByText("Unassigned")).toBeInTheDocument();
  });

  it("renders every assignee avatar on a multi-assignee row", () => {
    render(
      <TicketListView
        tickets={[
          {
            ref: { provider: "clickup", id: "CU-1", key: "CU-1" },
            title: "Pair on ticketing",
            state: { id: "todo", name: "To Do", category: "todo" },
            assignee: { name: "Ada" },
            assignees: [
              { name: "Ada", avatarUrl: "https://example.test/ada.png" },
              { name: "Grace" },
            ],
            labels: [],
            updatedAt: "2026-06-19T22:00:00.000Z",
            url: null,
            associationCount: 0,
            openPrCount: 0,
          },
        ]}
        hasNextPage={false}
        isFetchingNextPage={false}
        onLoadMore={vi.fn()}
        onSelectTicket={vi.fn()}
      />,
    );

    expect(screen.getByLabelText("Ada")).toHaveAttribute("title", "Ada");
    expect(screen.getByLabelText("Grace")).toHaveAttribute("title", "Grace");
    expect(screen.queryByText("Ada")).not.toBeInTheDocument();
    expect(screen.queryByText("Grace")).not.toBeInTheDocument();
    expect(document.querySelector("img[src='https://example.test/ada.png']")).not.toBeNull();
  });

  it("renders the project (category) in its own column just before the timestamp", () => {
    render(
      <TicketListView
        tickets={[
          {
            ref: { provider: "linear", id: "L-3", key: "L-3" },
            title: "Has a project",
            state: { id: "todo", name: "To Do", category: "todo" },
            project: "Platform",
            labels: [],
            updatedAt: "2026-06-19T22:00:00.000Z",
            url: null,
            associationCount: 0,
            openPrCount: 0,
          },
        ]}
        hasNextPage={false}
        isFetchingNextPage={false}
        onLoadMore={vi.fn()}
        onSelectTicket={vi.fn()}
      />,
    );

    const project = screen.getByText("Platform");
    const updated = screen.getByText(formatTicketDate("2026-06-19T22:00:00.000Z"));
    // Project (category) sits to the left of the updated timestamp.
    expect(project.compareDocumentPosition(updated) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();
  });

  it("shows the conversation count for linked RalphX work without a PR icon in the badge", () => {
    render(
      <TicketListView
        tickets={[
          {
            ref: { provider: "linear", id: "L-7", key: "L-7" },
            title: "Has open PR",
            state: { id: "started", name: "In Progress", category: "in_progress" },
            labels: [],
            updatedAt: "2026-06-19T22:00:00.000Z",
            url: null,
            associationCount: 2,
            openPrCount: 1,
          },
        ]}
        hasNextPage={false}
        isFetchingNextPage={false}
        onLoadMore={vi.fn()}
        onSelectTicket={vi.fn()}
      />,
    );

    // The RX badge keeps the conversation (suitcase) count...
    expect(screen.getByRole("img", { name: /2 RalphX conversations/i })).toBeInTheDocument();
    // ...but no longer carries the open-PR icon (that moved to the dedicated PR column).
    expect(screen.queryByRole("img", { name: /open pull request/i })).not.toBeInTheDocument();
  });

  it("renders an interactive PR detail control when the ticket has a representative open PR", () => {
    const onOpenPullRequestDetail = vi.fn();
    render(
      <TooltipProvider>
        <TicketListView
          tickets={[
            {
              ref: { provider: "linear", id: "L-7", key: "L-7" },
              title: "Has open PR",
              state: { id: "started", name: "In Progress", category: "in_progress" },
              labels: [],
              updatedAt: "2026-06-19T22:00:00.000Z",
              url: null,
              associationCount: 2,
              openPrCount: 1,
              openPrNumber: 42,
              openPrUrl: "https://github.com/x/y/pull/42",
            },
          ]}
          hasNextPage={false}
          isFetchingNextPage={false}
          onLoadMore={vi.fn()}
          onSelectTicket={vi.fn()}
          onOpenPullRequestDetail={onOpenPullRequestDetail}
        />
      </TooltipProvider>,
    );

    const prButton = screen.getByRole("button", { name: /open pull request #42/i });
    expect(prButton).toHaveTextContent("#42");

    fireEvent.click(prButton);
    expect(onOpenPullRequestDetail).toHaveBeenCalledTimes(1);
    expect(onOpenPullRequestDetail).toHaveBeenCalledWith(
      expect.objectContaining({ openPrNumber: 42, openPrUrl: "https://github.com/x/y/pull/42" }),
    );
  });

  it("does not render the PR control when the ticket has no representative open PR", () => {
    render(
      <TooltipProvider>
        <TicketListView
          tickets={[
            {
              ref: { provider: "linear", id: "L-8", key: "L-8" },
              title: "No open PR",
              state: { id: "todo", name: "To Do", category: "todo" },
              labels: [],
              updatedAt: "2026-06-19T22:00:00.000Z",
              url: null,
              associationCount: 1,
              openPrCount: 0,
              openPrNumber: null,
              openPrUrl: null,
            },
          ]}
          hasNextPage={false}
          isFetchingNextPage={false}
          onLoadMore={vi.fn()}
          onSelectTicket={vi.fn()}
          onOpenPullRequestDetail={vi.fn()}
        />
      </TooltipProvider>,
    );

    expect(
      screen.queryByRole("button", { name: /open pull request/i }),
    ).not.toBeInTheDocument();
    // The RX badge conversation count still renders.
    expect(screen.getByRole("img", { name: /1 RalphX conversation/i })).toBeInTheDocument();
  });

  it("renders a muted PR control naming the status for a closed representative PR", () => {
    const onOpenPullRequestDetail = vi.fn();
    render(
      <TooltipProvider>
        <TicketListView
          tickets={[
            {
              ref: { provider: "linear", id: "L-381", key: "L-381" },
              title: "Closed PR ticket",
              state: { id: "started", name: "In Progress", category: "in_progress" },
              labels: [],
              updatedAt: "2026-06-19T22:00:00.000Z",
              url: null,
              associationCount: 1,
              // open_pr_count stays 0 for a closed-only ticket, but the
              // representative PR is still shown.
              openPrCount: 0,
              openPrNumber: 381,
              openPrUrl: "https://github.com/x/y/pull/381",
              openPrStatus: "closed",
            },
          ]}
          hasNextPage={false}
          isFetchingNextPage={false}
          onLoadMore={vi.fn()}
          onSelectTicket={vi.fn()}
          onOpenPullRequestDetail={onOpenPullRequestDetail}
        />
      </TooltipProvider>,
    );

    const prButton = screen.getByRole("button", { name: /pull request #381 \(closed\)/i });
    expect(prButton).toHaveTextContent("#381");
    // A closed/merged PR is muted, not green.
    expect(prButton).toHaveStyle({ color: "var(--text-muted)" });

    fireEvent.click(prButton);
    expect(onOpenPullRequestDetail).toHaveBeenCalledTimes(1);
    expect(onOpenPullRequestDetail).toHaveBeenCalledWith(
      expect.objectContaining({ openPrNumber: 381, openPrStatus: "closed" }),
    );
  });

  it("colors an open representative PR green", () => {
    const onOpenPullRequestDetail = vi.fn();
    render(
      <TooltipProvider>
        <TicketListView
          tickets={[
            {
              ref: { provider: "linear", id: "L-99", key: "L-99" },
              title: "Open PR ticket",
              state: { id: "started", name: "In Progress", category: "in_progress" },
              labels: [],
              updatedAt: "2026-06-19T22:00:00.000Z",
              url: null,
              associationCount: 1,
              openPrCount: 1,
              openPrNumber: 99,
              openPrUrl: "https://github.com/x/y/pull/99",
              openPrStatus: "open",
            },
          ]}
          hasNextPage={false}
          isFetchingNextPage={false}
          onLoadMore={vi.fn()}
          onSelectTicket={vi.fn()}
          onOpenPullRequestDetail={onOpenPullRequestDetail}
        />
      </TooltipProvider>,
    );

    const prButton = screen.getByRole("button", { name: /open pull request #99/i });
    expect(prButton).toHaveStyle({ color: "var(--status-success)" });
  });

  it("places the PR control after the RX/suitcase badge in the row", () => {
    const onOpenPullRequestDetail = vi.fn();
    render(
      <TooltipProvider>
        <TicketListView
          tickets={[
            {
              ref: { provider: "linear", id: "L-7", key: "L-7" },
              title: "Both badges",
              state: { id: "started", name: "In Progress", category: "in_progress" },
              labels: [],
              updatedAt: "2026-06-19T22:00:00.000Z",
              url: null,
              associationCount: 2,
              openPrCount: 1,
              openPrNumber: 42,
              openPrUrl: "https://github.com/x/y/pull/42",
              openPrStatus: "open",
            },
          ]}
          hasNextPage={false}
          isFetchingNextPage={false}
          onLoadMore={vi.fn()}
          onSelectTicket={vi.fn()}
          onOpenPullRequestDetail={onOpenPullRequestDetail}
        />
      </TooltipProvider>,
    );

    const rxBadge = screen.getByRole("img", { name: /2 RalphX conversations/i });
    const prButton = screen.getByRole("button", { name: /open pull request #42/i });
    // The PR column sits to the RIGHT of the RX (suitcase) column, so the PR
    // control appears after the RX badge in document order.
    expect(
      rxBadge.compareDocumentPosition(prButton) & Node.DOCUMENT_POSITION_FOLLOWING,
    ).toBeTruthy();
  });

  it("opens the status options and moves the ticket on select when writable", async () => {
    const onMoveTicket = vi.fn();
    const onSelectTicket = vi.fn();
    render(
      <TooltipProvider>
        <TicketListView
          tickets={[tickets[0]!]}
          columns={[
            { id: "todo", name: "To Do", category: "todo", order: 0 },
            { id: "done", name: "Done", category: "done", order: 1 },
          ]}
          hasNextPage={false}
          isFetchingNextPage={false}
          onLoadMore={vi.fn()}
          onSelectTicket={onSelectTicket}
          canMoveTickets
          onMoveTicket={onMoveTicket}
        />
      </TooltipProvider>,
    );

    // No-overlap regression: an editable row shows the interactive control, not a
    // second read-only status icon on top of it — only the group heading carries
    // the read-only "Status" glyph.
    expect(screen.getAllByRole("img", { name: "Status: To Do" })).toHaveLength(1);

    const statusTrigger = screen.getByRole("button", {
      name: /change status \(current: to do\)/i,
    });
    // Radix DropdownMenu opens on pointerDown, not click.
    fireEvent.pointerDown(statusTrigger, { button: 0, ctrlKey: false });
    // Opening the status control must NOT select the row.
    expect(onSelectTicket).not.toHaveBeenCalled();

    const doneOption = screen.getByRole("menuitem", { name: /done/i });
    fireEvent.click(doneOption);
    expect(onMoveTicket).toHaveBeenCalledTimes(1);
    expect(onMoveTicket.mock.calls[0]?.[0]?.ref.id).toBe("10001");
    expect(onMoveTicket.mock.calls[0]?.[1]?.id).toBe("done");
    // Selecting a status still must not open the ticket detail.
    expect(onSelectTicket).not.toHaveBeenCalled();
  });

  it("keeps the status icon read-only when ticket moves are not writable", () => {
    render(
      <TooltipProvider>
        <TicketListView
          tickets={[tickets[0]!]}
          columns={[
            { id: "todo", name: "To Do", category: "todo", order: 0 },
            { id: "done", name: "Done", category: "done", order: 1 },
          ]}
          hasNextPage={false}
          isFetchingNextPage={false}
          onLoadMore={vi.fn()}
          onSelectTicket={vi.fn()}
          canMoveTickets={false}
          onMoveTicket={vi.fn()}
        />
      </TooltipProvider>,
    );

    expect(
      screen.queryByRole("button", { name: /change status/i }),
    ).not.toBeInTheDocument();
    // Read-only status glyph still renders (group heading + row both carry one).
    expect(
      screen.getAllByRole("img", { name: /^Status: To Do/ }).length,
    ).toBeGreaterThanOrEqual(1);
  });

  it("opening the PR control does not also select the ticket row", () => {
    const onSelectTicket = vi.fn();
    const onOpenPullRequestDetail = vi.fn();
    render(
      <TooltipProvider>
        <TicketListView
          tickets={[
            {
              ref: { provider: "linear", id: "L-7", key: "L-7" },
              title: "Has open PR",
              state: { id: "started", name: "In Progress", category: "in_progress" },
              labels: [],
              updatedAt: "2026-06-19T22:00:00.000Z",
              url: null,
              associationCount: 2,
              openPrCount: 1,
              openPrNumber: 42,
              openPrUrl: "https://github.com/x/y/pull/42",
            },
          ]}
          hasNextPage={false}
          isFetchingNextPage={false}
          onLoadMore={vi.fn()}
          onSelectTicket={onSelectTicket}
          onOpenPullRequestDetail={onOpenPullRequestDetail}
        />
      </TooltipProvider>,
    );

    fireEvent.click(screen.getByRole("button", { name: /open pull request #42/i }));
    expect(onOpenPullRequestDetail).toHaveBeenCalledTimes(1);
    expect(onSelectTicket).not.toHaveBeenCalled();
  });

  it("shows up to two labels with a +N overflow chip on dense rows", () => {
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
            openPrCount: 0,
          },
        ]}
        hasNextPage={false}
        isFetchingNextPage={false}
        onLoadMore={vi.fn()}
        onSelectTicket={vi.fn()}
      />,
    );

    expect(screen.getByText("a")).toBeInTheDocument();
    expect(screen.getByText("b")).toBeInTheDocument();
    expect(screen.queryByText("c")).not.toBeInTheDocument();
    expect(screen.getByText("+2")).toBeInTheDocument();
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

  it("groups rows under status headings ordered by the provided columns", () => {
    render(
      <TicketListView
        tickets={tickets}
        columns={[
          { id: "todo", name: "To Do", category: "todo", order: 0 },
          { id: "started", name: "In Progress", category: "in_progress", order: 1 },
        ]}
        hasNextPage={false}
        isFetchingNextPage={false}
        onLoadMore={vi.fn()}
        onSelectTicket={vi.fn()}
      />,
    );

    // Group headings render with counts; per-row status icons carry the state name.
    expect(screen.getByText("To Do")).toBeInTheDocument();
    expect(screen.getByText("In Progress")).toBeInTheDocument();
    expect(screen.getAllByRole("img", { name: /^Status:/ }).length).toBeGreaterThanOrEqual(2);
  });

  it("collapses and expands a status group when its heading is clicked", () => {
    render(
      <TicketListView
        tickets={tickets}
        columns={[
          { id: "todo", name: "To Do", category: "todo", order: 0 },
          { id: "started", name: "In Progress", category: "in_progress", order: 1 },
        ]}
        hasNextPage={false}
        isFetchingNextPage={false}
        onLoadMore={vi.fn()}
        onSelectTicket={vi.fn()}
      />,
    );

    expect(screen.getByRole("button", { name: /RX-1/ })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: /To Do/ }));
    // RX-1 (To Do group) is hidden; RX-2 (In Progress group) stays.
    expect(screen.queryByRole("button", { name: /RX-1/ })).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: /RX-2/ })).toBeInTheDocument();
  });

  it("moves focus between ticket rows with arrow keys", () => {
    render(
      <TicketListView
        tickets={tickets}
        columns={[
          { id: "todo", name: "To Do", category: "todo", order: 0 },
          { id: "started", name: "In Progress", category: "in_progress", order: 1 },
        ]}
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

  it("keeps the row list as the vertical scroll container", () => {
    render(
      <TicketListView
        tickets={tickets}
        columns={[
          { id: "todo", name: "To Do", category: "todo", order: 0 },
          { id: "started", name: "In Progress", category: "in_progress", order: 1 },
        ]}
        hasNextPage={false}
        isFetchingNextPage={false}
        onLoadMore={vi.fn()}
        onSelectTicket={vi.fn()}
      />,
    );

    const scrollRegion = document.querySelector("[data-ticket-list]");
    expect(scrollRegion).toHaveClass("min-h-0", "flex-1", "overflow-auto", "overscroll-contain");
  });
});

describe("TicketKanbanView", () => {
  it("renders labels and the assignee avatar on kanban cards", () => {
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
            openPrCount: 0,
          },
        ]}
        onSelectTicket={vi.fn()}
      />,
    );

    expect(screen.getByText("frontend")).toBeInTheDocument();
    expect(screen.getByText("ux")).toBeInTheDocument();
    expect(screen.getByLabelText("Ada Lovelace")).toHaveAttribute("title", "Ada Lovelace");
    expect(screen.queryByText("Ada Lovelace")).not.toBeInTheDocument();
  });

  it("renders multiple assignee avatars on kanban cards", () => {
    render(
      <TicketKanbanView
        columns={[{ id: "todo", name: "To Do", category: "todo", order: 0 }]}
        tickets={[
          {
            ref: { provider: "clickup", id: "CU-2", key: "CU-2" },
            title: "Shared ClickUp ticket",
            state: { id: "todo", name: "To Do", category: "todo" },
            assignees: [{ name: "Ada" }, { name: "Grace" }],
            labels: [],
            updatedAt: "2026-06-19T22:00:00.000Z",
            url: null,
            associationCount: 0,
            openPrCount: 0,
          },
        ]}
        onSelectTicket={vi.fn()}
      />,
    );

    expect(screen.getByLabelText("Ada")).toHaveAttribute("title", "Ada");
    expect(screen.getByLabelText("Grace")).toHaveAttribute("title", "Grace");
    expect(screen.queryByText("Ada")).not.toBeInTheDocument();
    expect(screen.queryByText("Grace")).not.toBeInTheDocument();
  });

  it("bounds each kanban column body to its own vertical scroll region", () => {
    render(
      <TicketKanbanView
        columns={[{ id: "todo", name: "To Do", category: "todo", order: 0 }]}
        tickets={tickets.map((ticket) => ({
          ...ticket,
          state: { id: "todo", name: "To Do", category: "todo" },
        }))}
        onSelectTicket={vi.fn()}
      />,
    );

    const column = screen.getByTestId("ticket-column-todo");
    expect(column).toHaveClass("h-full", "min-h-0", "overflow-hidden");
    const scrollRegion = column.querySelector(".overflow-y-auto");
    expect(scrollRegion).toHaveClass("min-h-0", "flex-1", "overscroll-contain");
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
