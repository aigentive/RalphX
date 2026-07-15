import { describe, expect, it } from "vitest";

import type { PullRequestCheck } from "@/api/github";

import { bucketCheck, summarizeChecks } from "./pullRequestChecksSummary";

function check(overrides: Partial<PullRequestCheck>): PullRequestCheck {
  return {
    name: "check",
    status: "completed",
    conclusion: "success",
    detailsUrl: null,
    ...overrides,
  };
}

describe("bucketCheck", () => {
  it("classifies successful / neutral / skipped completed checks as passed", () => {
    expect(bucketCheck(check({ conclusion: "success" }))).toBe("passed");
    expect(bucketCheck(check({ conclusion: "neutral" }))).toBe("passed");
    expect(bucketCheck(check({ conclusion: "skipped" }))).toBe("passed");
  });

  it("classifies the failing conclusions as failed", () => {
    for (const conclusion of [
      "failure",
      "timed_out",
      "cancelled",
      "action_required",
      "startup_failure",
      "stale",
    ]) {
      expect(bucketCheck(check({ conclusion }))).toBe("failed");
    }
  });

  it("classifies not-yet-completed or conclusion-less checks as pending", () => {
    expect(bucketCheck(check({ status: "in_progress", conclusion: null }))).toBe("pending");
    expect(bucketCheck(check({ status: "queued", conclusion: null }))).toBe("pending");
    expect(bucketCheck(check({ status: "completed", conclusion: null }))).toBe("pending");
  });

  it("is case-insensitive and treats unknown conclusions as pending", () => {
    expect(bucketCheck(check({ conclusion: "FAILURE" }))).toBe("failed");
    expect(bucketCheck(check({ status: "COMPLETED", conclusion: "SUCCESS" }))).toBe("passed");
    expect(bucketCheck(check({ status: "weird", conclusion: "mystery" }))).toBe("pending");
  });
});

describe("summarizeChecks", () => {
  it("counts each bucket and collects the failing checks", () => {
    const summary = summarizeChecks([
      check({ name: "build", conclusion: "success" }),
      check({ name: "lint", conclusion: "failure" }),
      check({ name: "e2e", status: "in_progress", conclusion: null }),
      check({ name: "unit", conclusion: "timed_out" }),
      check({ name: "docs", conclusion: "skipped" }),
    ]);

    expect(summary).toMatchObject({ total: 5, passed: 2, failed: 2, pending: 1 });
    expect(summary.failing.map((c) => c.name)).toEqual(["lint", "unit"]);
  });

  it("returns an all-zero summary for no checks", () => {
    expect(summarizeChecks([])).toEqual({
      total: 0,
      passed: 0,
      failed: 0,
      pending: 0,
      failing: [],
    });
  });
});
