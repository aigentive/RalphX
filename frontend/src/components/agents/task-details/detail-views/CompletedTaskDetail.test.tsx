import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { Task } from "@/types/task";
import { CompletedTaskDetail } from "./CompletedTaskDetail";

const { historyState, transitionsState, gitDiffState } = vi.hoisted(() => ({
  historyState: {
    data: [
      {
        id: "review-1",
        outcome: "approved",
        notes: "Reviewer signed off",
        feedback: "All checks pass",
        reviewer: "ai_reviewer",
        created_at: "2026-07-07T12:00:00Z",
      },
    ] as unknown[],
    isLoading: false,
  },
  transitionsState: {
    data: [] as unknown[],
  },
  gitDiffState: {
    commits: [
      {
        shortSha: "abc1234",
        message: "feat: complete task",
        sha: "abc1234",
      },
    ],
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

vi.mock("@/components/reviews/ReviewDetailModal", () => ({
  ReviewDetailModal: () => <div data-testid="review-detail-modal" />,
}));

vi.mock("../TaskRerunDialog", () => ({
  TaskRerunDialog: () => null,
}));

vi.mock("@/lib/tauri", () => ({
  api: {
    tasks: {
      move: vi.fn(),
    },
  },
}));

vi.mock("@/lib/task-actions/resume-execution-if-stopped", () => ({
  resumeExecutionIfStopped: vi.fn(),
}));

function task(): Task {
  return {
    id: "task-1",
    projectId: "project-1",
    title: "Completed task",
    description: "Readable completed task summary",
    internalStatus: "approved",
    category: "feature",
    startedAt: "2026-07-07T11:00:00Z",
    completedAt: "2026-07-07T12:00:00Z",
  } as unknown as Task;
}

function TestWrapper({ children }: { children: React.ReactNode }) {
  const queryClient = new QueryClient({
    defaultOptions: { queries: { retry: false } },
  });
  return (
    <QueryClientProvider client={queryClient}>
      {children}
    </QueryClientProvider>
  );
}

describe("Agents CompletedTaskDetail", () => {
  beforeEach(() => {
    historyState.isLoading = false;
  });

  it("renders completed stage content, review evidence, and current actions in the one-column shell", () => {
    render(<CompletedTaskDetail task={task()} />, { wrapper: TestWrapper });

    expect(screen.getByTestId("completed-task-detail")).toBeInTheDocument();
    expect(screen.getByTestId("task-detail-stage-body")).toHaveTextContent("Task Completed");
    expect(screen.getByTestId("task-detail-evidence")).toHaveTextContent("Review History");
    expect(screen.getByTestId("task-detail-actions")).toBeInTheDocument();
    expect(screen.getByTestId("review-code-button")).toBeInTheDocument();
  });

  it("keeps completed stage evidence but hides mutation actions in historical mode", () => {
    render(<CompletedTaskDetail task={task()} isHistorical />, {
      wrapper: TestWrapper,
    });

    expect(screen.getByTestId("task-detail-stage-body")).toHaveTextContent("Task Completed");
    expect(screen.getByTestId("task-detail-evidence")).toHaveTextContent("Review History");
    expect(screen.queryByTestId("task-detail-actions")).not.toBeInTheDocument();
    expect(screen.queryByTestId("review-code-button")).not.toBeInTheDocument();
  });
});
