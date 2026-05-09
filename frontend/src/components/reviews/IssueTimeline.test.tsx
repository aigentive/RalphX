import { describe, it, expect } from "vitest";
import { render, screen } from "@testing-library/react";

import { IssueTimeline, IssueTimelineCompact } from "./IssueTimeline";
import type { ReviewIssue } from "@/types/review-issue";

function makeIssue(overrides: Partial<ReviewIssue> = {}): ReviewIssue {
  return {
    id: "issue-1",
    reviewNoteId: "note-1",
    taskId: "t1",
    stepId: null,
    noStepReason: null,
    title: "Missing test coverage",
    description: "Add tests for the new branch.",
    severity: "major",
    category: "missing",
    filePath: null,
    lineNumber: null,
    codeSnippet: null,
    status: "open",
    resolutionNotes: null,
    addressedInAttempt: null,
    verifiedByReviewId: null,
    createdAt: new Date(Date.now() - 30_000).toISOString(),
    updatedAt: new Date(Date.now() - 30_000).toISOString(),
    ...overrides,
  };
}

describe("IssueTimeline", () => {
  it("renders title, description, and a Created event for an open issue", () => {
    render(<IssueTimeline issue={makeIssue()} />);
    expect(screen.getByText("Missing test coverage")).toBeInTheDocument();
    expect(screen.getByText("Add tests for the new branch.")).toBeInTheDocument();
    expect(screen.getByText("Issue created")).toBeInTheDocument();
    expect(screen.getByText("Severity: major")).toBeInTheDocument();
    // Only the created event for an open issue.
    expect(screen.queryByText("Work started")).toBeNull();
    expect(screen.queryByText("Addressed")).toBeNull();
    expect(screen.queryByText("Verified")).toBeNull();
  });

  it("hides description when not provided", () => {
    render(<IssueTimeline issue={makeIssue({ description: "" })} />);
    expect(screen.queryByText("Add tests for the new branch.")).toBeNull();
  });

  it("renders file link when filePath and lineNumber are set and showFileLink is true", () => {
    render(
      <IssueTimeline
        issue={makeIssue({ filePath: "src/foo.ts", lineNumber: 42 })}
      />,
    );
    expect(screen.getByText("src/foo.ts:42")).toBeInTheDocument();
  });

  it("hides file link when showFileLink is false", () => {
    render(
      <IssueTimeline
        issue={makeIssue({ filePath: "src/foo.ts", lineNumber: 42 })}
        showFileLink={false}
      />,
    );
    expect(screen.queryByText("src/foo.ts:42")).toBeNull();
  });

  it("hides file link when filePath or lineNumber is missing", () => {
    render(<IssueTimeline issue={makeIssue({ filePath: "src/foo.ts" })} />);
    expect(screen.queryByText(/src\/foo\.ts/)).toBeNull();
  });

  it("renders in_progress event when status is in_progress", () => {
    render(<IssueTimeline issue={makeIssue({ status: "in_progress" })} />);
    expect(screen.getByText("Issue created")).toBeInTheDocument();
    expect(screen.getByText("Work started")).toBeInTheDocument();
    expect(screen.queryByText("Addressed")).toBeNull();
  });

  it("renders addressed event with attempt number when addressedInAttempt is set", () => {
    render(
      <IssueTimeline
        issue={makeIssue({ status: "addressed", addressedInAttempt: 3 })}
      />,
    );
    expect(screen.getByText("Issue created")).toBeInTheDocument();
    expect(screen.getByText("Work started")).toBeInTheDocument();
    expect(screen.getByText("Addressed")).toBeInTheDocument();
    expect(screen.getByText("In attempt #3")).toBeInTheDocument();
  });

  it("renders the full chain ending in Verified for verified issues", () => {
    render(<IssueTimeline issue={makeIssue({ status: "verified" })} />);
    expect(screen.getByText("Issue created")).toBeInTheDocument();
    expect(screen.getByText("Work started")).toBeInTheDocument();
    expect(screen.getByText("Addressed")).toBeInTheDocument();
    expect(screen.getByText("Verified")).toBeInTheDocument();
  });

  it("renders wontfix event with resolution details when status is wontfix", () => {
    render(
      <IssueTimeline
        issue={makeIssue({
          status: "wontfix",
          resolutionNotes: "Out of scope for this release",
        })}
      />,
    );
    expect(screen.getByText("Won't fix")).toBeInTheDocument();
    expect(screen.getByText("Out of scope for this release")).toBeInTheDocument();
    // Resolution notes block should be hidden for wontfix (rendered inside the event itself).
    expect(screen.queryByText("Resolution Notes")).toBeNull();
  });

  it("renders Resolution Notes section when resolutionNotes is set and not wontfix", () => {
    render(
      <IssueTimeline
        issue={makeIssue({
          status: "verified",
          resolutionNotes: "Fixed by adding the missing test",
        })}
      />,
    );
    expect(screen.getByText("Resolution Notes")).toBeInTheDocument();
    expect(
      screen.getByText("Fixed by adding the missing test"),
    ).toBeInTheDocument();
  });

  it("renders 'Just now' when timestamp is fresh", () => {
    render(
      <IssueTimeline
        issue={makeIssue({ createdAt: new Date().toISOString() })}
      />,
    );
    expect(screen.getAllByText("Just now").length).toBeGreaterThan(0);
  });

  it("renders relative-time strings for older timestamps (m/h/d ago)", () => {
    const minutesAgo = new Date(Date.now() - 5 * 60_000).toISOString();
    const { rerender } = render(
      <IssueTimeline issue={makeIssue({ createdAt: minutesAgo })} />,
    );
    expect(screen.getByText(/\d+m ago/)).toBeInTheDocument();

    const hoursAgo = new Date(Date.now() - 3 * 60 * 60_000).toISOString();
    rerender(<IssueTimeline issue={makeIssue({ createdAt: hoursAgo })} />);
    expect(screen.getByText(/\d+h ago/)).toBeInTheDocument();

    const daysAgo = new Date(Date.now() - 5 * 24 * 60 * 60_000).toISOString();
    rerender(<IssueTimeline issue={makeIssue({ createdAt: daysAgo })} />);
    expect(screen.getByText(/\d+d ago/)).toBeInTheDocument();
  });

  it("renders 'Unknown' for missing timestamps", () => {
    render(
      <IssueTimeline
        issue={makeIssue({ createdAt: "" as unknown as string })}
      />,
    );
    expect(screen.getAllByText("Unknown").length).toBeGreaterThan(0);
  });

  it("propagates different severity values into the created event details", () => {
    render(<IssueTimeline issue={makeIssue({ severity: "critical" })} />);
    expect(screen.getByText("Severity: critical")).toBeInTheDocument();
  });
});

describe("IssueTimelineCompact", () => {
  it("renders one dot per timeline event for an open issue", () => {
    const { container } = render(
      <IssueTimelineCompact issue={makeIssue()} />,
    );
    const dots = container.querySelectorAll("div.rounded-full");
    expect(dots.length).toBe(1);
  });

  it("renders multiple dots and connectors for verified issues", () => {
    const { container } = render(
      <IssueTimelineCompact issue={makeIssue({ status: "verified" })} />,
    );
    const dots = container.querySelectorAll("div.rounded-full");
    // created + in_progress + addressed + verified = 4 events
    expect(dots.length).toBe(4);
  });

  it("includes a tooltip-like title attribute on each dot", () => {
    const { container } = render(
      <IssueTimelineCompact issue={makeIssue({ status: "addressed" })} />,
    );
    const dots = Array.from(container.querySelectorAll("div.rounded-full"));
    expect(dots.length).toBeGreaterThan(0);
    for (const d of dots) {
      expect(d.getAttribute("title")).toMatch(
        /(Issue created|Work started|Addressed)/,
      );
    }
  });

  it("renders wontfix dots", () => {
    const { container } = render(
      <IssueTimelineCompact issue={makeIssue({ status: "wontfix" })} />,
    );
    const dots = container.querySelectorAll("div.rounded-full");
    // created + wontfix = 2
    expect(dots.length).toBe(2);
  });
});
