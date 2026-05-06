import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

import { IssueList } from "./IssueList";
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
    createdAt: "2026-04-22T12:00:00Z",
    updatedAt: "2026-04-22T12:00:00Z",
    ...overrides,
  };
}

describe("IssueList", () => {
  it("renders the empty state when issues is empty", () => {
    render(<IssueList issues={[]} emptyMessage="Nothing to review" />);
    expect(screen.getByText("Nothing to review")).toBeInTheDocument();
  });

  it("renders issue cards with title, description, severity and status badges", () => {
    render(<IssueList issues={[makeIssue()]} />);
    expect(screen.getByText("Missing test coverage")).toBeInTheDocument();
    expect(screen.getByText("Add tests for the new branch.")).toBeInTheDocument();
    // Severity label appears in the group header AND the card badge.
    expect(screen.getAllByText(/Major/i).length).toBeGreaterThan(0);
  });

  it("renders the category pill when category is set", () => {
    render(<IssueList issues={[makeIssue({ category: "bug" })]} />);
    expect(screen.getByText("Bug")).toBeInTheDocument();
  });

  it("compact mode hides the description text", () => {
    render(<IssueList issues={[makeIssue()]} compact />);
    expect(screen.getByText("Missing test coverage")).toBeInTheDocument();
    // Description is hidden in compact mode.
    expect(screen.queryByText("Add tests for the new branch.")).toBeNull();
  });

  it("invokes onIssueClick when a card is clicked", async () => {
    const user = userEvent.setup();
    const onIssueClick = vi.fn();
    render(<IssueList issues={[makeIssue()]} onIssueClick={onIssueClick} />);
    await user.click(screen.getByText("Missing test coverage"));
    expect(onIssueClick).toHaveBeenCalledTimes(1);
  });

  it("groups issues by severity by default", () => {
    render(
      <IssueList
        issues={[
          makeIssue({ id: "a", title: "Critical thing", severity: "critical" }),
          makeIssue({ id: "b", title: "Minor thing", severity: "minor" }),
        ]}
      />,
    );
    // Both items render — group headers exist.
    expect(screen.getByText("Critical thing")).toBeInTheDocument();
    expect(screen.getByText("Minor thing")).toBeInTheDocument();
  });

  it("collapses a group when its header is clicked", async () => {
    const user = userEvent.setup();
    render(<IssueList issues={[makeIssue({ title: "Inside group" })]} />);
    expect(screen.getByText("Inside group")).toBeInTheDocument();

    // Click the "Major" group header to collapse it.
    const headerButton = screen
      .getAllByRole("button")
      .find((b) => /Major/i.test(b.textContent ?? ""));
    expect(headerButton).toBeDefined();
    await user.click(headerButton!);

    // Items inside the collapsed group should be hidden.
    expect(screen.queryByText("Inside group")).toBeNull();
  });

  it("groups issues by status when groupBy='status'", () => {
    render(
      <IssueList
        groupBy="status"
        issues={[
          makeIssue({ id: "o", title: "Open one", status: "open" }),
          makeIssue({ id: "v", title: "Verified one", status: "verified" }),
          makeIssue({ id: "w", title: "Wont fix one", status: "wontfix" }),
          makeIssue({ id: "ip", title: "In progress one", status: "in_progress" }),
        ]}
      />,
    );
    expect(screen.getByText("Open one")).toBeInTheDocument();
    expect(screen.getByText("Verified one")).toBeInTheDocument();
    expect(screen.getByText("Wont fix one")).toBeInTheDocument();
    expect(screen.getByText("In progress one")).toBeInTheDocument();
    // Status group header text (uppercased via CSS but source text preserved).
    expect(screen.getAllByText(/In Progress/).length).toBeGreaterThan(0);
  });

  it("groups issues by step when groupBy='step', placing nullable stepId in General Issues", () => {
    render(
      <IssueList
        groupBy="step"
        issues={[
          makeIssue({ id: "s1", title: "Step issue", stepId: "abcdef1234567890" }),
          makeIssue({ id: "n1", title: "No step issue", stepId: null }),
        ]}
      />,
    );
    expect(screen.getByText("Step issue")).toBeInTheDocument();
    expect(screen.getByText("No step issue")).toBeInTheDocument();
    expect(screen.getByText("General Issues")).toBeInTheDocument();
    // Step group title is "Step: <first 8 chars>..."
    expect(screen.getByText(/Step: abcdef12\.\.\./)).toBeInTheDocument();
  });

  it("renders the progress bar with severity breakdown when showProgress is set", () => {
    const progress = {
      taskId: "t1",
      total: 4,
      open: 1,
      inProgress: 1,
      addressed: 1,
      verified: 1,
      wontfix: 0,
      percentResolved: 50,
      bySeverity: {
        critical: { total: 1, open: 1, resolved: 0 },
        major: { total: 1, open: 0, resolved: 1 },
        minor: { total: 1, open: 0, resolved: 1 },
        suggestion: { total: 1, open: 0, resolved: 1 },
      },
    };
    render(
      <IssueList
        issues={[makeIssue()]}
        showProgress
        progress={progress}
      />,
    );
    // Percent label and status counts render.
    expect(screen.getByText("50%")).toBeInTheDocument();
    expect(screen.getByText("1 verified")).toBeInTheDocument();
    expect(screen.getByText("1 addressed")).toBeInTheDocument();
    expect(screen.getByText("1 in progress")).toBeInTheDocument();
    expect(screen.getByText("1 open")).toBeInTheDocument();
    // Severity breakdown lines render.
    expect(screen.getByText(/1\/1 critical/)).toBeInTheDocument();
    expect(screen.getByText(/0\/1 major/)).toBeInTheDocument();
  });
});
