import { describe, expect, it } from "vitest";

import {
  effortTone,
  optionIndexFromPointer,
  selectedOptionIndex,
} from "./runtimeSelectorModel";

const options = [
  { id: "quick", label: "Quick" },
  { id: "balanced", label: "Balanced" },
  { id: "deep", label: "Deep" },
];

describe("runtimeSelectorModel", () => {
  it("derives effort depth from catalog order instead of effort ids", () => {
    expect(selectedOptionIndex(options, "balanced")).toBe(1);
    expect(selectedOptionIndex(options, "unknown")).toBe(0);
  });

  it("preserves the established mini-indicator tones for current effort ids", () => {
    expect(effortTone(options, "low")).toBe("var(--status-error)");
    expect(effortTone(options, "medium")).toBe("var(--accent-primary)");
    expect(effortTone(options, "high")).toBe("var(--status-warning)");
    expect(effortTone(options, "xhigh")).toBe("var(--status-success)");
    expect(effortTone(options, "max")).toBe("var(--status-success)");
  });

  it("maps future ids to deterministic position-derived tones and stops", () => {
    expect(effortTone(options, "quick")).toBe("var(--status-error)");
    expect(effortTone(options, "deep")).toBe("var(--status-success)");
    expect(optionIndexFromPointer(74, 0, 100, options.length)).toBe(1);
    expect(optionIndexFromPointer(100, 0, 100, options.length)).toBe(2);
  });
});
