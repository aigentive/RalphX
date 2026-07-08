import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";

import type { PullRequestCheck, PullRequestReviewSummary } from "@/api/github";

import { PullRequestStatusStrip } from "./PullRequestStatusStrip";

function check(overrides: Partial<PullRequestCheck>): PullRequestCheck {
  return { name: "c", status: "completed", conclusion: "success", detailsUrl: null, ...overrides };
}

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

describe("PullRequestStatusStrip", () => {
  it("renders the review decision and CI counts", () => {
    render(
      <PullRequestStatusStrip
        reviewSummary={reviewSummary({ reviewDecision: "CHANGES_REQUESTED" })}
        checks={[
          check({ conclusion: "success" }),
          check({ conclusion: "success" }),
          check({ conclusion: "failure" }),
          check({ status: "in_progress", conclusion: null }),
        ]}
      />,
    );

    expect(screen.getByTestId("pr-status-strip")).toBeInTheDocument();
    expect(screen.getByText("Changes requested")).toBeInTheDocument();
    expect(screen.getByText("2 passed")).toBeInTheDocument();
    expect(screen.getByText("1 failed")).toBeInTheDocument();
    expect(screen.getByText("1 pending")).toBeInTheDocument();
  });

  it("shows a skeleton while loading", () => {
    render(<PullRequestStatusStrip reviewSummary={null} checks={[]} loading />);

    const skeleton = screen.getByTestId("pr-status-strip-skeleton");
    expect(skeleton).toBeInTheDocument();
    expect(
      skeleton.querySelector("[data-testid='pr-status-skeleton-chip']"),
    ).toHaveStyle("background-color: var(--bg-hover)");
    expect(screen.queryByTestId("pr-status-strip")).not.toBeInTheDocument();
  });

  it("renders nothing when there is no review decision and no checks", () => {
    const { container } = render(
      <PullRequestStatusStrip reviewSummary={reviewSummary()} checks={[]} />,
    );

    expect(container).toBeEmptyDOMElement();
  });

  it("surfaces a CI-unavailable chip when checks could not be fetched", () => {
    render(<PullRequestStatusStrip reviewSummary={null} checks={[]} checksUnavailable />);

    expect(screen.getByTestId("pr-status-strip")).toBeInTheDocument();
    expect(screen.getByText("CI unavailable")).toBeInTheDocument();
  });
});
