import { describe, expect, it } from "vitest";

import type { PullRequestReviewSummary } from "@/api/github";

import { reviewDecisionBadge } from "./pullRequestReviewDecision";

function summary(
  overrides: Partial<PullRequestReviewSummary> = {},
): PullRequestReviewSummary {
  return {
    reviewDecision: null,
    latestChangesRequestedAuthor: null,
    latestChangesRequestedBody: null,
    latestChangesRequestedSubmittedAt: null,
    latestChangesRequestedComments: [],
    ...overrides,
  };
}

describe("reviewDecisionBadge", () => {
  it("maps the known GitHub decisions", () => {
    expect(reviewDecisionBadge(summary({ reviewDecision: "APPROVED" }))).toEqual({
      label: "Approved",
      tone: "approved",
    });
    expect(
      reviewDecisionBadge(summary({ reviewDecision: "CHANGES_REQUESTED" })),
    ).toEqual({ label: "Changes requested", tone: "changesRequested" });
    expect(
      reviewDecisionBadge(summary({ reviewDecision: "REVIEW_REQUIRED" })),
    ).toEqual({ label: "Review required", tone: "pending" });
  });

  it("normalizes casing and whitespace", () => {
    expect(reviewDecisionBadge(summary({ reviewDecision: "  approved " }))).toEqual({
      label: "Approved",
      tone: "approved",
    });
  });

  it("returns null when there is no decision to show", () => {
    expect(reviewDecisionBadge(null)).toBeNull();
    expect(reviewDecisionBadge(undefined)).toBeNull();
    expect(reviewDecisionBadge(summary({ reviewDecision: null }))).toBeNull();
    expect(reviewDecisionBadge(summary({ reviewDecision: "" }))).toBeNull();
    expect(reviewDecisionBadge(summary({ reviewDecision: "SOMETHING_NEW" }))).toBeNull();
  });
});
