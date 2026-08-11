import { describe, expect, it } from "vitest";
import {
  aggregateThinkingSegments,
  joinThinkingSegmentTexts,
} from "./thinking-group";

describe("thinking group helpers", () => {
  it("aggregates lifecycle using the caller's default for missing settlement", () => {
    expect(aggregateThinkingSegments([
      { durationMs: 1_000 },
      { isSettled: true, durationMs: 2_000 },
    ], false)).toEqual({
      isSettled: false,
      segmentCount: 2,
      totalDurationMs: 3_000,
    });

    expect(aggregateThinkingSegments([{ durationMs: 1_000 }], true)).toEqual({
      isSettled: true,
      segmentCount: 1,
      totalDurationMs: 1_000,
    });
  });

  it("keeps the latest token progress only while the group is running", () => {
    expect(aggregateThinkingSegments([
      { isSettled: false, estimatedTokens: 500 },
      { isSettled: false, estimatedTokens: 1_200 },
    ], false)).toMatchObject({ isSettled: false, estimatedTokens: 1_200 });
    expect(aggregateThinkingSegments([
      { isSettled: true, estimatedTokens: 1_200 },
    ], false)).not.toHaveProperty("estimatedTokens");
  });

  it("retains settled reasoning tokens while duration stays available for label precedence", () => {
    expect(aggregateThinkingSegments([
      { isSettled: true, durationMs: 1_000, reasoningTokens: 400 },
    ], true)).toMatchObject({
      isSettled: true,
      totalDurationMs: 1_000,
      reasoningTokens: 400,
    });
  });

  it("joins non-empty segments with a visible separator", () => {
    expect(joinThinkingSegmentTexts([" first ", undefined, "", "second "]))
      .toBe("first\n\n···\n\nsecond");
  });

});
