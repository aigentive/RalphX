import { describe, it, expect } from "vitest";
import { formatThinkingSummary } from "./thinking-summary";

describe("formatThinkingSummary", () => {
  it("returns active label when not settled", () => {
    expect(formatThinkingSummary(false)).toBe("Agent thinking…");
  });

  it("includes locale-formatted estimated tokens while active", () => {
    expect(formatThinkingSummary(false, undefined, 12_345)).toBe("Agent thinking… · ~12,345 tokens");
  });

  it("returns settled label with duration when provided", () => {
    expect(formatThinkingSummary(true, 5000)).toBe("Agent thought for 5s");
  });

  it("returns settled label without duration when omitted", () => {
    expect(formatThinkingSummary(true)).toBe("Agent thought");
  });

  it("rounds sub-second durations down", () => {
    expect(formatThinkingSummary(true, 1400)).toBe("Agent thought for 1s");
  });
});
