import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";

import { HumanReviewTaskDetail } from "./HumanReviewTaskDetail";
import type { Task } from "@/types/task";

const { reviewIssuesGetByTaskId, reviewIssuesGetProgress } = vi.hoisted(() => ({
  reviewIssuesGetByTaskId: vi.fn().mockResolvedValue([]),
  reviewIssuesGetProgress: vi.fn().mockResolvedValue(null),
}));

vi.mock("@/api/review-issues", () => ({
  reviewIssuesApi: {
    getByTaskId: (id: string) => reviewIssuesGetByTaskId(id),
    getProgress: (id: string) => reviewIssuesGetProgress(id),
  },
}));

vi.mock("@/hooks/useReviews", () => ({
  useTaskStateHistory: () => ({
    data: [
      {
        id: "h1",
        outcome: "approved",
        notes: "AI Review summary content",
        feedback: "Looks ready",
        reviewer: "ai_reviewer",
        created_at: "2026-04-22T12:00:00Z",
      },
    ],
    isLoading: false,
  }),
  reviewKeys: { all: ["review"] },
}));

vi.mock("@/hooks/useTaskStateTransitions", () => ({
  useTaskStateTransitions: () => ({ data: [] }),
}));

vi.mock("@/lib/tauri", () => ({
  api: {
    reviews: {
      approveTask: vi.fn().mockResolvedValue(undefined),
      requestTaskChanges: vi.fn().mockResolvedValue(undefined),
    },
  },
  taskKeys: { all: ["tasks"] },
}));

vi.mock("@/lib/navigation", () => ({
  navigateToIdeationSession: vi.fn(),
}));

vi.mock("@/hooks/useConfirmation", () => ({
  useConfirmation: () => ({
    confirm: vi.fn().mockResolvedValue(true),
    confirmationDialogProps: {},
    ConfirmationDialog: () => null,
  }),
}));

function task(overrides?: Partial<Task>): Task {
  return {
    id: "task-hr-1",
    projectId: "proj-1",
    category: "feature",
    title: "Approve me",
    description: "AI says it's good",
    priority: 2,
    internalStatus: "review_passed",
    needsReviewPoint: false,
    createdAt: "2026-04-22T10:00:00Z",
    updatedAt: "2026-04-22T11:00:00Z",
    startedAt: null,
    completedAt: null,
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
});

describe("HumanReviewTaskDetail", () => {
  it("renders the AI review summary", () => {
    render(<HumanReviewTaskDetail task={task()} />, { wrapper: TestWrapper });
    expect(screen.getAllByText("AI Review Summary").length).toBeGreaterThan(0);
  });

  it("renders the human-review status header", () => {
    render(<HumanReviewTaskDetail task={task()} />, { wrapper: TestWrapper });
    expect(screen.getByText("AI Review Passed")).toBeInTheDocument();
  });
});
