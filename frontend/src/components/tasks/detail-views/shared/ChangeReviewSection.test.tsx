import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";

import { ChangeReviewSection, CommitSummaryCard } from "./ChangeReviewSection";

const { useGitDiffMock } = vi.hoisted(() => ({
  useGitDiffMock: vi.fn(),
}));
const { listTasksMock, getHistoryMock, usePlanBranchMock } = vi.hoisted(() => ({
  listTasksMock: vi.fn(),
  getHistoryMock: vi.fn(),
  usePlanBranchMock: vi.fn(),
}));

vi.mock("@/hooks/useGitDiff", () => ({
  useGitDiff: () => useGitDiffMock(),
}));
vi.mock("@/hooks/usePlanBranchForTask", () => ({
  usePlanBranchForTask: usePlanBranchMock,
}));
vi.mock("@/lib/tauri", () => ({
  api: {
    tasks: { list: listTasksMock },
    reviews: { getTaskStateHistory: getHistoryMock },
  },
}));

beforeEach(() => {
  useGitDiffMock.mockReset();
  listTasksMock.mockReset();
  getHistoryMock.mockReset();
  usePlanBranchMock.mockReturnValue({
    data: { projectId: "project-1", sessionId: "session-1" },
    isLoading: false,
  });
});

function renderPlanReview() {
  const client = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(
    <QueryClientProvider client={client}>
      <ChangeReviewSection
        taskId="merge-task-1"
        history={[]}
        stateTransitions={[]}
        context="plan_merge"
      />
    </QueryClientProvider>,
  );
}

describe("CommitSummaryCard", () => {
  it("renders the loading spinner while git diff history is loading", () => {
    useGitDiffMock.mockReturnValue({
      commits: [],
      historyError: null,
      isLoadingHistory: true,
    });
    const { container } = render(<CommitSummaryCard taskId="t1" />);
    expect(container.querySelector(".animate-spin")).toBeInTheDocument();
  });

  it("renders the empty fallback message when there are no commits", () => {
    useGitDiffMock.mockReturnValue({
      commits: [],
      historyError: null,
      isLoadingHistory: false,
    });
    render(<CommitSummaryCard taskId="t1" />);
    expect(screen.getByText("No commit history available")).toBeInTheDocument();
  });

  it("reports a failed history read instead of claiming there are no commits", () => {
    useGitDiffMock.mockReturnValue({
      commits: [],
      historyError: new Error("read failed"),
      isLoadingHistory: false,
    });
    render(<CommitSummaryCard taskId="t1" />);
    expect(screen.getByText(/could not be loaded/i)).toBeInTheDocument();
    expect(screen.queryByText("No commit history available")).not.toBeInTheDocument();
  });

  it("renders commit shortSha and message when commits exist", () => {
    useGitDiffMock.mockReturnValue({
      commits: [
        { shortSha: "abc1234", message: "Add baseline test coverage" },
        { shortSha: "def5678", message: "Fix flaky merge timing" },
      ],
      historyError: null,
      isLoadingHistory: false,
    });
    render(<CommitSummaryCard taskId="t1" />);
    expect(screen.getByText("abc1234")).toBeInTheDocument();
    expect(screen.getByText("Add baseline test coverage")).toBeInTheDocument();
    expect(screen.getByText("def5678")).toBeInTheDocument();
  });
});

describe("plan review history", () => {
  it("reports a failed read instead of claiming no review records exist", async () => {
    useGitDiffMock.mockReturnValue({ commits: [], historyError: null, isLoadingHistory: false });
    listTasksMock.mockResolvedValue({
      tasks: [{ id: "task-1", title: "Implement remote review" }],
      total: 1,
      limit: 500,
      offset: 0,
    });
    getHistoryMock.mockRejectedValue({
      outcome: "commandError",
      error: "REMOTE_INTERNAL_ERROR: review history unavailable",
    });

    renderPlanReview();

    expect(await screen.findByText("Review history could not be loaded.")).toBeInTheDocument();
    expect(screen.queryByText("No internal plan review records available")).not.toBeInTheDocument();
    expect(getHistoryMock).toHaveBeenCalledWith("task-1");
  });

  it("keeps a successful empty review history as empty", async () => {
    useGitDiffMock.mockReturnValue({ commits: [], historyError: null, isLoadingHistory: false });
    listTasksMock.mockResolvedValue({ tasks: [], total: 0, limit: 500, offset: 0 });
    renderPlanReview();

    expect(await screen.findByText("No internal plan review records available")).toBeInTheDocument();
    expect(screen.queryByText("Review history could not be loaded.")).not.toBeInTheDocument();
  });
});
