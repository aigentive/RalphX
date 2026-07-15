import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import type {
  PullRequestReviewSummary,
  PullRequestReviewThreadComment,
} from "@/api/github";

import { PullRequestReviewSection } from "./PullRequestReviewSection";

function reviewSummary(
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

function threadComment(
  overrides: Partial<PullRequestReviewThreadComment> = {},
): PullRequestReviewThreadComment {
  return {
    id: "t1",
    author: "reviewer",
    body: "Inline note",
    path: "src/app.ts",
    side: "RIGHT",
    line: 12,
    url: null,
    createdAt: null,
    inReplyToId: null,
    isOutdated: false,
    ...overrides,
  };
}

describe("PullRequestReviewSection", () => {
  it("renders the decision, changes-requested feedback, and inline comments", () => {
    render(
      <PullRequestReviewSection
        reviewSummary={reviewSummary({
          reviewDecision: "CHANGES_REQUESTED",
          latestChangesRequestedAuthor: "alice",
          latestChangesRequestedBody: "Please fix the guard.",
          latestChangesRequestedComments: [
            { id: "f1", author: "alice", path: "src/guard.ts", line: 8, body: "null check missing" },
          ],
        })}
        reviewThread={[]}
        loading={false}
      />,
    );

    expect(screen.getByText("Changes requested")).toBeInTheDocument();
    expect(screen.getByText("Please fix the guard.")).toBeInTheDocument();
    expect(screen.getByText("null check missing")).toBeInTheDocument();
    expect(screen.getByText(/alice requested changes/)).toBeInTheDocument();
  });

  it("still renders inline review-thread comments", () => {
    render(
      <PullRequestReviewSection
        reviewSummary={reviewSummary()}
        reviewThread={[threadComment()]}
        loading={false}
      />,
    );

    expect(screen.getByText("Inline note")).toBeInTheDocument();
  });

  it("shows an empty state when there is no review data", () => {
    render(
      <PullRequestReviewSection
        reviewSummary={reviewSummary()}
        reviewThread={[]}
        loading={false}
      />,
    );

    expect(screen.getByText("No review yet.")).toBeInTheDocument();
  });
});
