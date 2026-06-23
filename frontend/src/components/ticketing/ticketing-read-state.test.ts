import { describe, expect, it } from "vitest";

import type { TicketComment, TicketSummary } from "@/api/ticketing";

import {
  countNewComments,
  distinctAssigneeNames,
  filterTicketsByAssignee,
  filterTicketsByProject,
  groupTicketsByStatus,
  hasActiveTicketFilters,
  isCommentNewSince,
  isOptimisticCommentId,
  isTicketUpdatedSince,
  sortCommentsByCreatedAt,
  ticketRefKey,
  UNASSIGNED_ASSIGNEE,
} from "./ticketing-read-state";

function ticketInState(id: string, stateId: string, stateName: string): TicketSummary {
  return {
    ref: { provider: "linear", id, key: id },
    title: `Ticket ${id}`,
    state: { id: stateId, name: stateName, category: "todo" },
    labels: [],
    updatedAt: "2026-06-20T12:00:00.000Z",
    associationCount: 0,
  };
}

function comment(overrides: Partial<TicketComment>): TicketComment {
  return { bodyMarkdown: "", bodyText: "", ...overrides };
}

function ticket(id: string, assigneeName: string | null): TicketSummary {
  return {
    ref: { provider: "linear", id, key: id },
    title: `Ticket ${id}`,
    state: { id: "todo", name: "To Do", category: "todo" },
    ...(assigneeName ? { assignee: { name: assigneeName } } : {}),
    labels: [],
    updatedAt: "2026-06-20T12:00:00.000Z",
    associationCount: 0,
  };
}

const BASELINE = "2026-06-20T12:00:00.000Z";
const BEFORE = "2026-06-20T11:00:00.000Z";
const AFTER = "2026-06-20T13:00:00.000Z";

describe("ticketRefKey", () => {
  it("combines provider and id", () => {
    expect(ticketRefKey({ provider: "linear", id: "ABC-1", key: "ABC-1" })).toBe("linear:ABC-1");
  });
});

describe("isOptimisticCommentId", () => {
  it("detects local and optimistic prefixes", () => {
    expect(isOptimisticCommentId("local:123")).toBe(true);
    expect(isOptimisticCommentId("optimistic:abc")).toBe(true);
    expect(isOptimisticCommentId("real-id")).toBe(false);
    expect(isOptimisticCommentId(null)).toBe(false);
  });
});

describe("sortCommentsByCreatedAt", () => {
  it("orders oldest first and puts undated comments last", () => {
    const sorted = sortCommentsByCreatedAt([
      comment({ id: "b", createdAt: AFTER }),
      comment({ id: "x" }),
      comment({ id: "a", createdAt: BEFORE }),
    ]);
    expect(sorted.map((c) => c.id)).toEqual(["a", "b", "x"]);
  });

  it("does not mutate the input array", () => {
    const input = [comment({ id: "b", createdAt: AFTER }), comment({ id: "a", createdAt: BEFORE })];
    sortCommentsByCreatedAt(input);
    expect(input.map((c) => c.id)).toEqual(["b", "a"]);
  });
});

describe("isCommentNewSince", () => {
  it("flags comments created after the baseline", () => {
    expect(isCommentNewSince(comment({ id: "1", createdAt: AFTER }), BASELINE)).toBe(true);
  });

  it("ignores comments created before the baseline", () => {
    expect(isCommentNewSince(comment({ id: "1", createdAt: BEFORE }), BASELINE)).toBe(false);
  });

  it("never flags anything when the ticket was never opened", () => {
    expect(isCommentNewSince(comment({ id: "1", createdAt: AFTER }), null)).toBe(false);
  });

  it("never flags the viewer's own optimistic comment", () => {
    expect(isCommentNewSince(comment({ id: "local:1", createdAt: AFTER }), BASELINE)).toBe(false);
  });

  it("ignores comments without a timestamp", () => {
    expect(isCommentNewSince(comment({ id: "1" }), BASELINE)).toBe(false);
  });
});

describe("countNewComments", () => {
  it("counts only comments newer than the baseline", () => {
    const comments = [
      comment({ id: "old", createdAt: BEFORE }),
      comment({ id: "new-1", createdAt: AFTER }),
      comment({ id: "new-2", createdAt: AFTER }),
      comment({ id: "optimistic:mine", createdAt: AFTER }),
    ];
    // Two provider comments are new; the viewer's own optimistic comment is excluded.
    expect(countNewComments(comments, BASELINE)).toBe(2);
    expect(countNewComments(comments, null)).toBe(0);
  });
});

describe("isTicketUpdatedSince", () => {
  it("is true only when updated after a prior open", () => {
    expect(isTicketUpdatedSince(AFTER, BASELINE)).toBe(true);
    expect(isTicketUpdatedSince(BEFORE, BASELINE)).toBe(false);
    expect(isTicketUpdatedSince(AFTER, null)).toBe(false);
    expect(isTicketUpdatedSince(undefined, BASELINE)).toBe(false);
  });
});

describe("distinctAssigneeNames", () => {
  it("returns sorted unique assignee names and ignores unassigned tickets", () => {
    const tickets = [ticket("1", "Ada"), ticket("2", null), ticket("3", "Ada"), ticket("4", "Grace")];
    expect(distinctAssigneeNames(tickets)).toEqual(["Ada", "Grace"]);
  });
});

describe("filterTicketsByAssignee", () => {
  const tickets = [ticket("1", "Ada"), ticket("2", null), ticket("3", "Grace")];

  it("returns everyone when no assignee is selected", () => {
    expect(filterTicketsByAssignee(tickets, null)).toHaveLength(3);
  });

  it("returns only unassigned tickets for the sentinel", () => {
    const result = filterTicketsByAssignee(tickets, UNASSIGNED_ASSIGNEE);
    expect(result.map((t) => t.ref.id)).toEqual(["2"]);
  });

  it("returns only tickets matching the named assignee", () => {
    const result = filterTicketsByAssignee(tickets, "Grace");
    expect(result.map((t) => t.ref.id)).toEqual(["3"]);
  });
});

describe("filterTicketsByProject", () => {
  const tickets: TicketSummary[] = [
    { ...ticket("1", null), project: "FLUX PT" },
    { ...ticket("2", null), project: "Other" },
    ticket("3", null),
  ];

  it("returns all tickets when no project is selected", () => {
    expect(filterTicketsByProject(tickets, null)).toHaveLength(3);
  });

  it("returns only tickets in the named project", () => {
    expect(filterTicketsByProject(tickets, "FLUX PT").map((t) => t.ref.id)).toEqual(["1"]);
  });
});

describe("groupTicketsByStatus", () => {
  it("groups by state and orders groups by the provided columns", () => {
    const tickets = [
      ticketInState("1", "todo", "To Do"),
      ticketInState("2", "started", "In Progress"),
      ticketInState("3", "todo", "To Do"),
    ];
    const groups = groupTicketsByStatus(tickets, [
      { id: "started", name: "In Progress", category: "in_progress", order: 0 },
      { id: "todo", name: "To Do", category: "todo", order: 1 },
    ]);

    expect(groups.map((group) => group.id)).toEqual(["started", "todo"]);
    expect(groups[1]?.tickets.map((t) => t.ref.id)).toEqual(["1", "3"]);
  });

  it("sorts states absent from the columns last, alphabetically", () => {
    const groups = groupTicketsByStatus(
      [ticketInState("1", "z", "Zeta"), ticketInState("2", "a", "Alpha")],
      [],
    );
    expect(groups.map((group) => group.name)).toEqual(["Alpha", "Zeta"]);
  });
});

describe("hasActiveTicketFilters", () => {
  const empty = { text: "", stateIds: [], labels: [], assignee: null };

  it("is false when no filter is set (whitespace-only text counts as empty)", () => {
    expect(hasActiveTicketFilters(empty)).toBe(false);
    expect(hasActiveTicketFilters({ ...empty, text: "   " })).toBe(false);
  });

  it("is true when any of text, status, labels, or assignee is set", () => {
    expect(hasActiveTicketFilters({ ...empty, text: "bug" })).toBe(true);
    expect(hasActiveTicketFilters({ ...empty, stateIds: ["todo"] })).toBe(true);
    expect(hasActiveTicketFilters({ ...empty, labels: ["backend"] })).toBe(true);
    expect(hasActiveTicketFilters({ ...empty, assignee: "Me" })).toBe(true);
  });
});
