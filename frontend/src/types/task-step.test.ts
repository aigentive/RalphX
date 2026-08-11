import { describe, expect, it } from "vitest";
import {
  getCompletableStepProgressCounts,
  getStepProgressDisplay,
} from "./task-step";

describe("task step progress display helpers", () => {
  it("excludes skipped steps from display counts", () => {
    expect(
      getCompletableStepProgressCounts({
        total: 5,
        completed: 1,
        skipped: 2,
      })
    ).toEqual({ completed: 1, total: 3 });
  });

  it("separates completed progress from the active in-progress layer", () => {
    const display = getStepProgressDisplay({
      total: 5,
      completed: 1,
      inProgress: 1,
      skipped: 2,
    });

    expect(display.completed).toBe(1);
    expect(display.total).toBe(3);
    expect(display.completedPercent).toBeCloseTo(33.333, 2);
    expect(display.activePercent).toBeCloseTo(66.667, 2);
  });

  it("does not create progress when all steps are skipped", () => {
    expect(
      getStepProgressDisplay({
        total: 2,
        completed: 0,
        inProgress: 0,
        skipped: 2,
      })
    ).toEqual({
      completed: 0,
      total: 0,
      completedPercent: 0,
      activePercent: 0,
    });
  });
});
