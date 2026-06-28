import { describe, expect, it } from "vitest";

import { formatPrDate, normalizePrStatus } from "./PullRequestDetailUtils";

describe("PullRequestDetailUtils", () => {
  it("normalizes known PR statuses without inventing an open status", () => {
    expect(normalizePrStatus("open")).toBe("Open");
    expect(normalizePrStatus("merged")).toBe("Merged");
    expect(normalizePrStatus("closed")).toBe("Closed");
    expect(normalizePrStatus("draft")).toBe("Draft");
    expect(normalizePrStatus(null)).toBeNull();
    expect(normalizePrStatus("unknown")).toBeNull();
  });

  it("draft flag takes precedence over raw state", () => {
    expect(normalizePrStatus("open", true)).toBe("Draft");
  });

  it("formats valid dates and leaves invalid values unchanged", () => {
    expect(formatPrDate(null)).toBeNull();
    expect(formatPrDate("not-a-date")).toBe("not-a-date");
    expect(formatPrDate("2026-06-24T08:00:00Z")).toEqual(expect.any(String));
  });
});
