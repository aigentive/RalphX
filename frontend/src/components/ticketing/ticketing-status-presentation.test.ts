import { describe, expect, it } from "vitest";

import type { TicketingColumn } from "@/api/ticketing";

import { mergeProviderAndTicketColumns, statusColor } from "./ticketing-status-presentation";

describe("ticketing status presentation", () => {
  it("uses resolved status color before category fallback", () => {
    expect(statusColor({ category: "in_progress", color: "#123456" })).toBe("#123456");
    expect(statusColor({ category: "done", color: null })).toBe("var(--status-success)");
  });

  it("collapses ClickUp fallback columns that duplicate provider statuses by case or one-character typo", () => {
    const providerColumns: TicketingColumn[] = [
      {
        id: "in_progress",
        name: "in progress",
        category: "in_progress",
        order: 0,
        color: "#1090ff",
      },
      {
        id: "done",
        name: "Done",
        category: "done",
        order: 1,
      },
      {
        id: "archived",
        name: "Archived",
        category: "done",
        order: 2,
        isVisible: false,
      },
    ];
    const ticketColumns: TicketingColumn[] = [
      {
        id: "In Progress",
        name: "In Progress",
        category: "in_progress",
        order: 0,
      },
      {
        id: "in_progres",
        name: "In Progres",
        category: "in_progress",
        order: 1,
      },
      {
        id: "blocked",
        name: "Blocked",
        category: "other",
        order: 2,
      },
      {
        id: "archived",
        name: "Archived",
        category: "done",
        order: 3,
      },
    ];

    const merged = mergeProviderAndTicketColumns(providerColumns, ticketColumns);

    expect(merged.map((column) => column.name)).toEqual(["in progress", "Done", "Blocked"]);
    expect(merged[0]?.color).toBe("#1090ff");
  });
});
