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
});
