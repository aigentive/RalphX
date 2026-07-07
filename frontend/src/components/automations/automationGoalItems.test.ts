import { describe, expect, it } from "vitest";

import {
  AUTOMATION_PHASES_LABEL,
  AUTOMATION_STATUS_LABELS,
  parseAutomationGoalItems,
} from "./automationGoalItems";

describe("automationGoalItems", () => {
  it("exposes the canonical status labels and phases heading", () => {
    expect(AUTOMATION_PHASES_LABEL).toBe("Phases");
    expect(AUTOMATION_STATUS_LABELS).toEqual({
      draft: "Draft",
      active: "Approved",
      paused: "Paused",
      completed: "Completed",
      stopped: "Stopped",
    });
  });

  it("returns an empty list for empty, blank, non-JSON, or non-array values", () => {
    expect(parseAutomationGoalItems(null)).toEqual([]);
    expect(parseAutomationGoalItems("")).toEqual([]);
    expect(parseAutomationGoalItems("   ")).toEqual([]);
    expect(parseAutomationGoalItems("not-json")).toEqual([]);
    expect(parseAutomationGoalItems('{"title":"nope"}')).toEqual([]);
  });

  it("prefers title, then text, then a Phase fallback for the item label", () => {
    const items = parseAutomationGoalItems(
      JSON.stringify([
        { title: "Explicit title", status: "done" },
        { text: "From text" },
        { id: "only-id" },
      ]),
    );

    expect(items).toEqual([
      { id: "phase-1", title: "Explicit title", status: "done" },
      { id: "phase-2", title: "From text", status: "pending" },
      { id: "only-id", title: "Phase 3", status: "pending" },
    ]);
  });

  it("drops non-object entries and defaults status to pending", () => {
    const items = parseAutomationGoalItems(
      JSON.stringify([{ title: "Keep me" }, "ignored", null, 42]),
    );

    expect(items).toEqual([{ id: "phase-1", title: "Keep me", status: "pending" }]);
  });

  it("applies the optional limit to the parsed items", () => {
    const value = JSON.stringify(
      Array.from({ length: 8 }, (_, index) => ({ title: `Phase ${index + 1}` })),
    );

    expect(parseAutomationGoalItems(value, { limit: 6 })).toHaveLength(6);
    expect(parseAutomationGoalItems(value)).toHaveLength(8);
  });
});
