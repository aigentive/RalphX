import { describe, expect, it } from "vitest";

import { getModelLabel } from "./model-utils";

describe("getModelLabel", () => {
  it("labels aliases and exact pinned Claude Opus ids distinctly", () => {
    expect(getModelLabel("opus")).toBe("Opus");
    expect(getModelLabel("claude-opus-4-7")).toBe("Opus 4.7");
    expect(getModelLabel("claude-opus-4-8")).toBe("Opus 4.8");
    expect(getModelLabel("claude-opus-5")).toBe("Opus 5");
  });

  it("falls back to the raw id for unknown models", () => {
    expect(getModelLabel("claude-opus-preview")).toBe("claude-opus-preview");
  });
});
