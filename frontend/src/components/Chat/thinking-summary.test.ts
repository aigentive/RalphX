import { describe, it, expect } from "vitest";
import { formatThinkingGroupSummary, formatThinkingSummary } from "./thinking-summary";

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

  it("shows settled reasoning tokens only when duration is absent", () => {
    expect(formatThinkingSummary(true, undefined, undefined, 1_234))
      .toBe("Agent thought · ~1,234 reasoning tokens");
    expect(formatThinkingSummary(true, 5_000, undefined, 1_234))
      .toBe("Agent thought for 5s");
  });

  it("rounds sub-second durations down", () => {
    expect(formatThinkingSummary(true, 1400)).toBe("Agent thought for 1s");
  });

  it("keeps a single grouped block byte-identical to the existing formatter", () => {
    expect(formatThinkingGroupSummary({
      isSettled: true,
      segmentCount: 1,
      totalDurationMs: 5_000,
    })).toBe(formatThinkingSummary(true, 5_000));
  });

  it("adds a step count to settled multi-segment groups", () => {
    expect(formatThinkingGroupSummary({
      isSettled: true,
      segmentCount: 2,
      totalDurationMs: 34_000,
    })).toBe("Agent thought for 34s · 2 steps");
  });
});
