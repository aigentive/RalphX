import { describe, it, expect } from "vitest";

import {
  getBaseCardStyles,
  getCardStyles,
  isDraggableStatus,
} from "./TaskCard.utils";

describe("TaskCard.utils", () => {
  it("isDraggableStatus returns true for ready and false for non-draggable statuses", () => {
    expect(isDraggableStatus("ready")).toBe(true);
    expect(isDraggableStatus("merging")).toBe(false);
    expect(isDraggableStatus("merged")).toBe(false);
  });

  it("getBaseCardStyles applies grab cursor when draggable and default when not", () => {
    expect(getBaseCardStyles("ready", false, true).cursor).toBe("grab");
    expect(getBaseCardStyles("ready", false, false).cursor).toBe("default");
  });

  it("getBaseCardStyles archived flag still returns a styled surface", () => {
    const archived = getBaseCardStyles("ready", true, true);
    expect(archived.backgroundColor).toBeDefined();
    expect(archived.borderStyle).toBe("solid");
  });

  it("getBaseCardStyles routes warning statuses to the warning surface", () => {
    const blocked = getBaseCardStyles("blocked", false, false);
    const ready = getBaseCardStyles("ready", false, false);
    expect(blocked.backgroundColor).not.toEqual(ready.backgroundColor);
  });

  it("getBaseCardStyles routes success statuses to the success surface", () => {
    const merged = getBaseCardStyles("merged", false, false);
    const ready = getBaseCardStyles("ready", false, false);
    expect(merged.backgroundColor).not.toEqual(ready.backgroundColor);
  });

  it("getBaseCardStyles routes error statuses to the error surface", () => {
    const failed = getBaseCardStyles("failed", false, false);
    const ready = getBaseCardStyles("ready", false, false);
    expect(failed.backgroundColor).not.toEqual(ready.backgroundColor);
  });

  it("getCardStyles dragging branch swaps cursor + transform + zIndex", () => {
    const dragging = getCardStyles("ready", false, true, true, false);
    expect(dragging.cursor).toBe("grabbing");
    expect(dragging.transform).toBe("scale(1.015)");
    expect(dragging.zIndex).toBe(50);
  });

  it("getCardStyles selected branch adds inset selection box-shadow", () => {
    const selected = getCardStyles("ready", false, false, true, true);
    expect(typeof selected.boxShadow).toBe("string");
    expect(String(selected.boxShadow)).toContain("inset");
  });

  it("getCardStyles default branch returns the base styles unchanged", () => {
    const idle = getCardStyles("ready", false, false, true, false);
    expect(idle.cursor).toBe("grab");
    expect(idle.transform).toBeUndefined();
    expect(idle.zIndex).toBeUndefined();
  });
});
