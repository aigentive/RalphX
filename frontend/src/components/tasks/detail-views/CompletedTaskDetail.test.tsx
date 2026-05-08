import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor, act } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";

import { CompletedTaskDetail } from "./CompletedTaskDetail";
import type { Task } from "@/types/task";

const { historyState, transitionsState, gitDiffState } = vi.hoisted(() => ({
  historyState: {
    data: [
      {
        id: "h1",
        outcome: "approved",
        notes: "Reviewer signed off",
        feedback: "All checks pass",
        reviewer: "ai_reviewer",
        created_at: "2026-04-22T12:00:00Z",
      },
    ] as unknown[] | undefined,
    isLoading: false,
  },
  transitionsState: {
    data: [] as unknown[],
  },
  gitDiffState: {
    commits: [
      { shortSha: "abc1234", message: "feat: add thing", sha: "abc1234" },
      { shortSha: "def5678", message: "chore: refactor", sha: "def5678" },
    ] as Array<{ shortSha: string; message: string; sha: string }>,
  },
}));

vi.mock("@/hooks/useReviews", () => ({
  useTaskStateHistory: () => historyState,
  reviewKeys: { all: ["review"] },
}));

vi.mock("@/hooks/useTaskStateTransitions", () => ({
  useTaskStateTransitions: () => transitionsState,
}));

vi.mock("@/hooks/useGitDiff", () => ({
  useGitDiff: () => gitDiffState,
}));

const moveMock = vi.fn().mockResolvedValue(undefined);
vi.mock("@/lib/tauri", () => ({
  api: {
    tasks: {
      move: (...args: unknown[]) => moveMock(...args),
    },
  },
  taskKeys: { all: ["tasks"] },
}));

const resumeMock = vi.fn().mockResolvedValue(undefined);
vi.mock("@/lib/task-actions/resume-execution-if-stopped", () => ({
  resumeExecutionIfStopped: (...args: unknown[]) => resumeMock(...args),
}));

vi.mock("@/components/tasks/TaskRerunDialog", () => ({
  TaskRerunDialog: ({
    isOpen,
    onClose,
    onConfirm,
    isProcessing,
    error,
  }: {
    isOpen: boolean;
    onClose: () => void;
    onConfirm: (r: { option: "keep_changes" | "revert_commit" | "create_new"; note?: string }) => void;
    isProcessing: boolean;
    error: string | null;
  }) =>
    isOpen ? (
      <div data-testid="rerun-dialog">
        {isProcessing && <span data-testid="rerun-processing">processing</span>}
        {error && <span data-testid="rerun-error">{error}</span>}
        <button data-testid="rerun-close" onClick={onClose}>
          close
        </button>
        <button
          data-testid="rerun-confirm"
          onClick={() => onConfirm({ option: "keep_changes", note: "go" })}
        >
          confirm
        </button>
      </div>
    ) : null,
}));

vi.mock("@/components/reviews/ReviewDetailModal", () => ({
  ReviewDetailModal: ({ onClose }: { onClose: () => void }) => (
    <div data-testid="review-detail-modal">
      <button data-testid="modal-close" onClick={onClose}>
        x
      </button>
    </div>
  ),
}));

function task(overrides?: Partial<Task>): Task {
  return {
    id: "task-c-1",
    projectId: "proj-1",
    category: "feature",
    title: "Done thing",
    description: "Task that is approved",
    priority: 2,
    internalStatus: "approved",
    needsReviewPoint: false,
    createdAt: "2026-04-22T10:00:00Z",
    updatedAt: "2026-04-22T11:00:00Z",
    startedAt: "2026-04-22T10:30:00Z",
    completedAt: "2026-04-22T11:00:00Z",
    archivedAt: null,
    blockedReason: null,
    taskBranch: null,
    worktreePath: null,
    mergeCommitSha: null,
    metadata: null,
    ...overrides,
  };
}

function TestWrapper({ children }: { children: React.ReactNode }) {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return <QueryClientProvider client={qc}>{children}</QueryClientProvider>;
}

beforeEach(() => {
  vi.clearAllMocks();
  historyState.isLoading = false;
  historyState.data = [
    {
      id: "h1",
      outcome: "approved",
      notes: "Reviewer signed off",
      feedback: "All checks pass",
      reviewer: "ai_reviewer",
      created_at: "2026-04-22T12:00:00Z",
    },
  ];
  gitDiffState.commits = [
    { shortSha: "abc1234", message: "feat: add thing", sha: "abc1234" },
    { shortSha: "def5678", message: "chore: refactor", sha: "def5678" },
  ];
});

describe("CompletedTaskDetail", () => {
  it("renders the loader while history is loading", () => {
    historyState.isLoading = true;
    historyState.data = undefined;
    const { container } = render(<CompletedTaskDetail task={task()} />, {
      wrapper: TestWrapper,
    });
    expect(container.querySelector(".animate-spin")).toBeInTheDocument();
  });

  it("renders the completed-task layout with banner, duration, and review history", () => {
    render(<CompletedTaskDetail task={task()} />, { wrapper: TestWrapper });
    expect(screen.getByTestId("completed-task-detail")).toBeInTheDocument();
    expect(screen.getByText("Task Completed")).toBeInTheDocument();
    expect(screen.getByTestId("completed-task-duration")).toBeInTheDocument();
    expect(screen.getByTestId("review-history-section")).toBeInTheDocument();
    expect(screen.getByText("Review History")).toBeInTheDocument();
  });

  it("hides duration when startedAt or completedAt is missing", () => {
    render(
      <CompletedTaskDetail task={task({ startedAt: null, completedAt: null })} />,
      { wrapper: TestWrapper }
    );
    expect(screen.queryByTestId("completed-task-duration")).not.toBeInTheDocument();
  });

  it("hides actions when isHistorical is true", () => {
    render(<CompletedTaskDetail task={task()} isHistorical />, {
      wrapper: TestWrapper,
    });
    expect(screen.queryByTestId("action-buttons")).not.toBeInTheDocument();
    expect(screen.queryByTestId("review-code-button")).not.toBeInTheDocument();
  });

  it("opens the review detail modal when Review Code is clicked", () => {
    render(<CompletedTaskDetail task={task()} />, { wrapper: TestWrapper });
    fireEvent.click(screen.getByTestId("review-code-button"));
    expect(screen.getByTestId("review-detail-modal")).toBeInTheDocument();
    fireEvent.click(screen.getByTestId("modal-close"));
    expect(screen.queryByTestId("review-detail-modal")).not.toBeInTheDocument();
  });

  it("View Final Diff button is rendered and clickable", () => {
    render(<CompletedTaskDetail task={task()} />, { wrapper: TestWrapper });
    const btn = screen.getByTestId("view-diff-button");
    expect(btn).toBeInTheDocument();
    fireEvent.click(btn);
  });

  it("opens the rerun dialog when Reopen Task is clicked, and closes it", () => {
    render(<CompletedTaskDetail task={task()} />, { wrapper: TestWrapper });
    expect(screen.queryByTestId("rerun-dialog")).not.toBeInTheDocument();
    fireEvent.click(screen.getByTestId("reopen-task-button"));
    expect(screen.getByTestId("rerun-dialog")).toBeInTheDocument();
    fireEvent.click(screen.getByTestId("rerun-close"));
    expect(screen.queryByTestId("rerun-dialog")).not.toBeInTheDocument();
  });

  it("confirms rerun: calls api.tasks.move and resumeExecutionIfStopped", async () => {
    render(<CompletedTaskDetail task={task()} />, { wrapper: TestWrapper });
    fireEvent.click(screen.getByTestId("reopen-task-button"));
    await act(async () => {
      fireEvent.click(screen.getByTestId("rerun-confirm"));
    });
    await waitFor(() => {
      expect(moveMock).toHaveBeenCalledWith("task-c-1", "ready", undefined, "go");
      expect(resumeMock).toHaveBeenCalledWith("proj-1");
    });
    await waitFor(() => {
      expect(screen.queryByTestId("rerun-dialog")).not.toBeInTheDocument();
    });
  });

  it("shows error in rerun dialog when api.tasks.move throws", async () => {
    moveMock.mockRejectedValueOnce(new Error("network down"));
    render(<CompletedTaskDetail task={task()} />, { wrapper: TestWrapper });
    fireEvent.click(screen.getByTestId("reopen-task-button"));
    await act(async () => {
      fireEvent.click(screen.getByTestId("rerun-confirm"));
    });
    await waitFor(() => {
      expect(screen.getByTestId("rerun-error")).toHaveTextContent("network down");
    });
  });

  it("shows fallback error message when api.tasks.move rejects with non-Error", async () => {
    moveMock.mockRejectedValueOnce("boom");
    render(<CompletedTaskDetail task={task()} />, { wrapper: TestWrapper });
    fireEvent.click(screen.getByTestId("reopen-task-button"));
    await act(async () => {
      fireEvent.click(screen.getByTestId("rerun-confirm"));
    });
    await waitFor(() => {
      expect(screen.getByTestId("rerun-error")).toHaveTextContent("Failed to reopen task");
    });
  });

  it("renders even when commits list is empty (uses 'unknown' fallback)", () => {
    gitDiffState.commits = [];
    render(<CompletedTaskDetail task={task()} />, { wrapper: TestWrapper });
    expect(screen.getByTestId("completed-task-detail")).toBeInTheDocument();
  });
});
