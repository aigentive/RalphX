import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen } from "@testing-library/react";

import { CommitSummaryCard } from "./ChangeReviewSection";

const { useGitDiffMock } = vi.hoisted(() => ({
  useGitDiffMock: vi.fn(),
}));

vi.mock("@/hooks/useGitDiff", () => ({
  useGitDiff: () => useGitDiffMock(),
}));

beforeEach(() => {
  useGitDiffMock.mockReset();
});

describe("CommitSummaryCard", () => {
  it("renders the loading spinner while git diff history is loading", () => {
    useGitDiffMock.mockReturnValue({
      commits: [],
      isLoadingHistory: true,
    });
    const { container } = render(<CommitSummaryCard taskId="t1" />);
    expect(container.querySelector(".animate-spin")).toBeInTheDocument();
  });

  it("renders the empty fallback message when there are no commits", () => {
    useGitDiffMock.mockReturnValue({
      commits: [],
      isLoadingHistory: false,
    });
    render(<CommitSummaryCard taskId="t1" />);
    expect(screen.getByText("No commit history available")).toBeInTheDocument();
  });

  it("renders commit shortSha and message when commits exist", () => {
    useGitDiffMock.mockReturnValue({
      commits: [
        { shortSha: "abc1234", message: "Add baseline test coverage" },
        { shortSha: "def5678", message: "Fix flaky merge timing" },
      ],
      isLoadingHistory: false,
    });
    render(<CommitSummaryCard taskId="t1" />);
    expect(screen.getByText("abc1234")).toBeInTheDocument();
    expect(screen.getByText("Add baseline test coverage")).toBeInTheDocument();
    expect(screen.getByText("def5678")).toBeInTheDocument();
  });
});
