import { describe, expect, it } from "vitest";
import {
  DIFF_ANNOTATION_LEVEL_LEGEND,
  annotationLevelColor,
  isBlockingAnnotationLevel,
} from "./diffRenderHelpers";

describe("isBlockingAnnotationLevel", () => {
  it("accepts exactly the levels the legend labels as blocking", () => {
    const blockingLegendEntry = DIFF_ANNOTATION_LEVEL_LEGEND.find(
      (item) => item.label === "Blocking",
    );
    expect(blockingLegendEntry).toBeDefined();

    for (const level of blockingLegendEntry!.levels.split(", ")) {
      expect(isBlockingAnnotationLevel(level)).toBe(true);
    }
  });

  it("rejects every level the legend does not classify as blocking", () => {
    const nonBlockingLevels = DIFF_ANNOTATION_LEVEL_LEGEND.filter(
      (item) => item.label !== "Blocking",
    ).flatMap((item) => item.levels.split(", "));

    for (const level of [...nonBlockingLevels, "unknown-level"]) {
      expect(isBlockingAnnotationLevel(level)).toBe(false);
    }
  });

  it("normalizes casing", () => {
    expect(isBlockingAnnotationLevel("CRITICAL")).toBe(true);
    expect(isBlockingAnnotationLevel("Warning")).toBe(false);
  });
});

describe("annotationLevelColor", () => {
  it("classifies every level the workspace reviewer is told to emit", () => {
    // agents/ralphx-workspace-reviewer/shared/prompt.md instructs reviewers to
    // use notice / warning / info — none of them may fall through to the
    // unclassified accent color.
    expect(annotationLevelColor("notice")).toBe("var(--status-info)");
    expect(annotationLevelColor("warning")).toBe("var(--status-warning)");
    expect(annotationLevelColor("info")).toBe("var(--status-info)");
  });

  it("lists every classified level in the legend", () => {
    const legendLevels = DIFF_ANNOTATION_LEVEL_LEGEND.filter(
      (item) => item.label !== "Other",
    ).flatMap((item) => item.levels.split(", "));

    expect(legendLevels).toContain("info");
    for (const level of legendLevels) {
      expect(annotationLevelColor(level)).not.toBe("var(--accent-primary)");
    }
  });

  it("still falls back to the accent color for genuinely unknown levels", () => {
    expect(annotationLevelColor("banana")).toBe("var(--accent-primary)");
  });
});
