import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";

import type {
  TicketingCapabilities,
  TicketSummary,
  TicketTransitionOption,
} from "@/api/ticketing";
import { TooltipProvider } from "@/components/ui/tooltip";

import { TicketDetailSheet } from "./TicketDetailSheet";

const baseCapabilities: TicketingCapabilities = {
  supportsBoards: true,
  supportsKanban: true,
  kanbanWrite: true,
  statusWrite: true,
  assignmentWrite: true,
  commentWrite: true,
  freshness: "manual",
};

const baseTicket: TicketSummary = {
  ref: { provider: "linear", id: "ABC-1", key: "ABC-1" },
  title: "Polish the ticket detail overlay",
  state: { id: "todo", name: "To Do", category: "todo" },
  labels: [],
  updatedAt: "2026-06-19T22:00:00.000Z",
  url: null,
  associationCount: 0,
};

const writableTransition: TicketTransitionOption = {
  toStateId: "done",
  name: "Done",
  category: "done",
};

function renderSheet(
  overrides: {
    ticket?: TicketSummary;
    capabilities?: TicketingCapabilities;
  } = {},
) {
  return render(
    <TooltipProvider>
      <TicketDetailSheet
        open
        ticket={overrides.ticket ?? baseTicket}
        capabilities={overrides.capabilities ?? baseCapabilities}
        transitions={[writableTransition]}
        associations={undefined}
        isDetailLoading={false}
        isAssociationsLoading={false}
        isTransitionPending={false}
        isAssignPending={false}
        isCommentPending={false}
        onClose={vi.fn()}
      />
    </TooltipProvider>,
  );
}

describe("TicketDetailSheet assignee control", () => {
  it("offers 'Assign to me' only when the ticket is unassigned", () => {
    renderSheet();

    expect(screen.getByRole("button", { name: /assign to me/i })).toBeInTheDocument();
    expect(screen.queryByText("Assignee")).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /clear assignee/i })).not.toBeInTheDocument();
  });

  it("shows the assignee and hides 'Assign to me' when assigned", () => {
    renderSheet({
      ticket: { ...baseTicket, assignee: { name: "Adrian Demian" } },
    });

    expect(screen.getByText("Assignee")).toBeInTheDocument();
    expect(screen.getByText("Adrian Demian")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /assign to me/i })).not.toBeInTheDocument();
    // Clear assignee stays available because assignment write-back is enabled.
    expect(screen.getByRole("button", { name: /clear assignee/i })).toBeInTheDocument();
  });

  it("shows the assignee read-only when assignment write-back is unavailable", () => {
    renderSheet({
      ticket: { ...baseTicket, assignee: { name: "Adrian Demian" } },
      capabilities: { ...baseCapabilities, assignmentWrite: false },
    });

    expect(screen.getByText("Adrian Demian")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /assign to me/i })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /clear assignee/i })).not.toBeInTheDocument();
  });
});
