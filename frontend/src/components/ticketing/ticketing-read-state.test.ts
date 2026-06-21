import { describe, expect, it } from "vitest";

import type { TicketComment } from "@/api/ticketing";

import {
  countNewComments,
  isCommentNewSince,
  isOptimisticCommentId,
  isTicketUpdatedSince,
  sortCommentsByCreatedAt,
  ticketRefKey,
} from "./ticketing-read-state";

function comment(overrides: Partial<TicketComment>): TicketComment {
  return { bodyMarkdown: "", bodyText: "", ...overrides };
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
