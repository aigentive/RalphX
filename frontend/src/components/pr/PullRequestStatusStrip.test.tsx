import type { ReactNode } from "react";

import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it } from "vitest";

import type { PullRequestCheck, PullRequestReviewSummary } from "@/api/github";
import { TooltipProvider } from "@/components/ui/tooltip";

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

function renderStrip(ui: ReactNode) {
  return render(<TooltipProvider delayDuration={0}>{ui}</TooltipProvider>);
}

describe("PullRequestStatusStrip", () => {
  it("preserves the default worded review and CI counts in order", () => {
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
    expect(
      Array.from(screen.getByTestId("pr-status-strip").children).map(
        (child) => child.textContent,
      ),
    ).toEqual(["Changes requested", "2 passed", "1 failed", "1 pending"]);
    expect(screen.getByTestId("pr-status-strip")).not.toHaveAttribute("tabindex");
  });

  it("renders compact accessible chips with a focusable worded tooltip", async () => {
    const user = userEvent.setup();
    renderStrip(
      <PullRequestStatusStrip
        variant="compact"
        reviewSummary={reviewSummary({ reviewDecision: "CHANGES_REQUESTED" })}
        checks={[
          check({ conclusion: "success" }),
          check({ conclusion: "success" }),
          check({ conclusion: "failure" }),
          check({ status: "in_progress", conclusion: null }),
        ]}
      />,
    );

    const strip = screen.getByRole("group", { name: "Pull request status" });
    expect(strip).toHaveAttribute("tabindex", "0");
    expect(screen.getByLabelText("Changes requested")).toHaveTextContent("");
    expect(screen.getByLabelText("2 passed")).toHaveTextContent("2");
    expect(screen.getByLabelText("1 failed")).toHaveTextContent("1");
    expect(screen.getByLabelText("1 pending")).toHaveTextContent("1");
    expect(screen.queryByText("2 passed")).not.toBeInTheDocument();

    strip.focus();
    const tooltip = await screen.findByRole("tooltip");
    expect(tooltip).toHaveTextContent(
      "Changes requested2 passed1 failed1 pending",
    );
    await user.keyboard("{Escape}");
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

  it("preserves the loading skeleton in compact mode", () => {
    render(
      <PullRequestStatusStrip
        variant="compact"
        reviewSummary={null}
        checks={[]}
        loading
      />,
    );

    expect(
      screen
        .getByTestId("pr-status-strip-skeleton")
        .querySelectorAll("[data-testid='pr-status-skeleton-chip']"),
    ).toHaveLength(2);
    expect(screen.queryByTestId("pr-status-strip")).not.toBeInTheDocument();
  });

  it("renders nothing when there is no review decision and no checks", () => {
    const { container } = render(
      <PullRequestStatusStrip reviewSummary={reviewSummary()} checks={[]} />,
    );

    expect(container).toBeEmptyDOMElement();
  });

  it("renders nothing in compact mode when there is no status data", () => {
    const { container } = renderStrip(
      <PullRequestStatusStrip
        variant="compact"
        reviewSummary={reviewSummary()}
        checks={[]}
      />,
    );

    expect(container).toBeEmptyDOMElement();
  });

  it("surfaces a CI-unavailable chip when checks could not be fetched", () => {
    render(<PullRequestStatusStrip reviewSummary={null} checks={[]} checksUnavailable />);

    expect(screen.getByTestId("pr-status-strip")).toBeInTheDocument();
    expect(screen.getByText("CI unavailable")).toBeInTheDocument();
  });

  it("keeps CI unavailable accessible but visually compact", () => {
    renderStrip(
      <PullRequestStatusStrip
        variant="compact"
        reviewSummary={null}
        checks={[]}
        checksUnavailable
      />,
    );

    expect(screen.getByLabelText("CI unavailable")).toHaveTextContent("");
    expect(screen.queryByText("CI unavailable")).not.toBeInTheDocument();
  });
});
