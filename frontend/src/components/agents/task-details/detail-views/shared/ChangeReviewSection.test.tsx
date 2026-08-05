import { render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { CommitSummaryCard } from "./ChangeReviewSection";

const { useGitDiffMock } = vi.hoisted(() => ({ useGitDiffMock: vi.fn() }));

vi.mock("@/hooks/useGitDiff", () => ({ useGitDiff: () => useGitDiffMock() }));

beforeEach(() => useGitDiffMock.mockReset());

describe("CommitSummaryCard", () => {
  it("reports a failed history read instead of claiming there are no commits", () => {
    useGitDiffMock.mockReturnValue({
      commits: [],
      historyError: new Error("REMOTE_COMMAND_UNAVAILABLE"),
      isLoadingHistory: false,
    });
    render(<CommitSummaryCard taskId="t1" />);
    expect(screen.getByText(/could not be loaded/i)).toBeInTheDocument();
    expect(screen.queryByText("No commit history available")).not.toBeInTheDocument();
  });

  it("keeps the genuine empty-history copy", () => {
    useGitDiffMock.mockReturnValue({
      commits: [],
      historyError: null,
      isLoadingHistory: false,
    });
    render(<CommitSummaryCard taskId="t1" />);
    expect(screen.getByText("No commit history available")).toBeInTheDocument();
  });
});
