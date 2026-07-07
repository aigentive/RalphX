import { describe, expect, it } from "vitest";

import {
  AUTOMATION_PHASE_STATUS_LABELS,
  AUTOMATION_PHASES_LABEL,
  AUTOMATION_STATUS_LABELS,
  normalizeAutomationPhaseStatus,
  parseAutomationGoalItems,
  summarizeAutomationPhases,
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

  it("exposes canonical phase status labels", () => {
    expect(AUTOMATION_PHASE_STATUS_LABELS).toEqual({
      pending: "Pending",
      in_progress: "In progress",
      done: "Done",
      skipped: "Skipped",
    });
  });

  it("normalizes known statuses and falls back to pending", () => {
    expect(normalizeAutomationPhaseStatus("done")).toBe("done");
    expect(normalizeAutomationPhaseStatus("in_progress")).toBe("in_progress");
    expect(normalizeAutomationPhaseStatus("skipped")).toBe("skipped");
    expect(normalizeAutomationPhaseStatus("pending")).toBe("pending");
    expect(normalizeAutomationPhaseStatus("")).toBe("pending");
    expect(normalizeAutomationPhaseStatus("weird")).toBe("pending");
  });

  it("summarizes phase progress with counts, current index, and ratio", () => {
    const items = parseAutomationGoalItems(
      JSON.stringify([
        { title: "A", status: "done" },
        { title: "B", status: "done" },
        { title: "C", status: "in_progress" },
        { title: "D", status: "skipped" },
        { title: "E", status: "pending" },
        { title: "F", status: "mystery" },
      ]),
    );

    const summary = summarizeAutomationPhases(items);

    expect(summary).toEqual({
      total: 6,
      done: 2,
      inProgress: 1,
      pending: 2,
      skipped: 1,
      currentIndex: 2,
      progressRatio: 2 / 6,
    });
  });

  it("returns a zeroed summary for an empty phase list", () => {
    expect(summarizeAutomationPhases([])).toEqual({
      total: 0,
      done: 0,
      inProgress: 0,
      pending: 0,
      skipped: 0,
      currentIndex: -1,
      progressRatio: 0,
    });
  });
});
