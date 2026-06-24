import { describe, expect, it } from "vitest";

import { assigneeInitials, splitLabels } from "./ticketing-presentation";

describe("assigneeInitials", () => {
  it("uses first and last initial for multi-word names", () => {
    expect(assigneeInitials("Adrian Demian")).toBe("AD");
    expect(assigneeInitials("ada lovelace byron")).toBe("AB");
  });

  it("uses the first two letters for single-word names", () => {
    expect(assigneeInitials("octocat")).toBe("OC");
  });

  it("falls back to a placeholder for blank names", () => {
    expect(assigneeInitials("   ")).toBe("?");
  });
});

describe("splitLabels", () => {
  it("returns all labels when within the max", () => {
    expect(splitLabels(["a", "b"], 3)).toEqual({ visible: ["a", "b"], overflow: 0 });
  });

  it("truncates and reports the overflow count", () => {
    expect(splitLabels(["a", "b", "c", "d", "e"], 3)).toEqual({
      visible: ["a", "b", "c"],
      overflow: 2,
    });
  });

  it("treats a non-positive max as all-overflow", () => {
    expect(splitLabels(["a", "b"], 0)).toEqual({ visible: [], overflow: 2 });
  });
});
