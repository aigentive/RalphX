import { describe, expect, it } from "vitest";

import type { TicketRef, TicketStateCategory, TicketSummary } from "@/api/ticketing";

import {
  categoryToken,
  formatTicketDate,
  providerLabel,
  ticketButtonLabel,
  ticketKey,
} from "./ticketing-utils";

function makeRef(overrides: Partial<TicketRef> = {}): TicketRef {
  return { provider: "jira", id: "10001", ...overrides };
}

function makeSummary(overrides: Partial<TicketSummary> = {}): TicketSummary {
  return {
    ref: makeRef(),
    title: "Fix flaky merge",
    state: { id: "todo", name: "To Do", category: "todo" },
    labels: [],
    updatedAt: "2026-06-19T22:00:00.000Z",
    associationCount: 0,
    ...overrides,
  };
}

describe("ticketKey", () => {
  it("prefers the human-readable key when present", () => {
    expect(ticketKey(makeRef({ key: "RX-42" }))).toBe("RX-42");
  });

  it("falls back to the provider id when key is null or undefined", () => {
    expect(ticketKey(makeRef({ key: null }))).toBe("10001");
    expect(ticketKey(makeRef({ key: undefined }))).toBe("10001");
    expect(ticketKey(makeRef())).toBe("10001");
  });
});

describe("ticketButtonLabel", () => {
  it("combines the resolved key and the ticket title", () => {
    const ticket = makeSummary({ ref: makeRef({ key: "RX-7" }), title: "Polish cards" });
    expect(ticketButtonLabel(ticket)).toBe("RX-7 Polish cards");
  });

  it("uses the id when no key is available", () => {
    const ticket = makeSummary({ ref: makeRef({ id: "issue-9", key: null }), title: "No key" });
    expect(ticketButtonLabel(ticket)).toBe("issue-9 No key");
  });
});

describe("formatTicketDate", () => {
  it("returns Unknown for missing values", () => {
    expect(formatTicketDate(undefined)).toBe("Unknown");
    expect(formatTicketDate(null)).toBe("Unknown");
    expect(formatTicketDate("")).toBe("Unknown");
  });

  it("returns Unknown for unparseable date strings", () => {
    expect(formatTicketDate("not-a-date")).toBe("Unknown");
  });

  it("formats a valid ISO timestamp into a non-empty, non-Unknown label", () => {
    const formatted = formatTicketDate("2026-06-19T22:00:00.000Z");
    expect(formatted).not.toBe("Unknown");
    expect(formatted.length).toBeGreaterThan(0);
    // The label includes a short month abbreviation for the date.
    expect(formatted).toMatch(/[A-Za-z]{3}/);
    // Minimal: month + day only, with no time-of-day.
    expect(formatted).not.toMatch(/\d{1,2}:\d{2}/);
    expect(formatted).not.toMatch(/\b[AP]M\b/i);
  });
});

describe("categoryToken", () => {
  it("maps each known category to its design token", () => {
    expect(categoryToken("done")).toBe("var(--status-success)");
    expect(categoryToken("in_progress")).toBe("var(--accent-primary)");
    expect(categoryToken("other")).toBe("var(--status-warning)");
    expect(categoryToken("todo")).toBe("var(--text-muted)");
  });

  it("falls back to the muted token for unexpected categories", () => {
    expect(categoryToken("unmapped" as TicketStateCategory)).toBe("var(--text-muted)");
  });
});

describe("providerLabel", () => {
  it("maps each known provider to its display label", () => {
    expect(providerLabel("jira")).toBe("Jira");
    expect(providerLabel("linear")).toBe("Linear");
    expect(providerLabel("clickup")).toBe("ClickUp");
  });

  it("falls back to a neutral label for unknown provider values", () => {
    // ClickUp must never be silently rendered as "Linear" (the old binary
    // ternary's bug); unknown providers get a neutral, non-misleading label.
    expect(providerLabel("github")).toBe("Provider");
    expect(providerLabel("")).toBe("Provider");
  });
});
