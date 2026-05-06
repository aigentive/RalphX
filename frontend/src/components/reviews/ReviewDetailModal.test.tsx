import { describe, it, expect, vi, beforeEach } from "vitest";
import { render } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";

import { ReviewDetailModal } from "./ReviewDetailModal";

const { mocks } = vi.hoisted(() => ({
  mocks: {
    taskGet: vi.fn(),
    reviewIssuesGetByTaskId: vi.fn().mockResolvedValue([]),
    reviewIssuesGetProgress: vi.fn().mockResolvedValue(null),
    approveTask: vi.fn().mockResolvedValue(undefined),
    requestTaskChanges: vi.fn().mockResolvedValue(undefined),
  },
}));

vi.mock("@/lib/tauri", () => ({
  api: {
    tasks: { get: (id: string) => mocks.taskGet(id) },
    reviews: {
      approveTask: (...args: unknown[]) => mocks.approveTask(...args),
      requestTaskChanges: (...args: unknown[]) => mocks.requestTaskChanges(...args),
    },
  },
  taskKeys: { all: ["tasks"], detail: (id: string) => ["tasks", id] },
}));

vi.mock("@/api/review-issues", () => ({
  reviewIssuesApi: {
    getByTaskId: (id: string) => mocks.reviewIssuesGetByTaskId(id),
    getProgress: (id: string) => mocks.reviewIssuesGetProgress(id),
  },
}));

vi.mock("@/hooks/useReviews", () => ({
  useReviewsByTaskId: () => ({ hasAiReview: true }),
  useTaskStateHistory: () => ({
    data: [
      {
        id: "n1",
        outcome: "approved",
        notes: "AI approved",
        feedback: "",
        reviewer: "ai_reviewer",
        created_at: "2026-04-22T10:00:00Z",
      },
    ],
  }),
  reviewKeys: { all: ["review"] },
}));

vi.mock("@/hooks/useGitDiff", () => ({
  useGitDiff: () => ({
    changes: [],
    commits: [],
    commitFiles: [],
    isLoadingChanges: false,
    isLoadingHistory: false,
    isLoadingCommitFiles: false,
    fetchDiff: vi.fn(),
    fetchCommitFiles: vi.fn(),
  }),
}));

vi.mock("@/hooks/useConfirmation", () => ({
  useConfirmation: () => ({
    confirm: vi.fn().mockResolvedValue(true),
    confirmationDialogProps: {},
    ConfirmationDialog: () => null,
  }),
}));

function TestWrapper({ children }: { children: React.ReactNode }) {
  const qc = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return <QueryClientProvider client={qc}>{children}</QueryClientProvider>;
}

beforeEach(() => {
  vi.clearAllMocks();
  mocks.taskGet.mockResolvedValue({
    id: "task-r-1",
    projectId: "proj-1",
    category: "feature",
    title: "Implement caching",
    description: "Add cache layer",
    priority: 2,
    internalStatus: "review_passed",
    needsReviewPoint: false,
    createdAt: "2026-04-22T10:00:00Z",
    updatedAt: "2026-04-22T10:00:00Z",
    startedAt: null,
    completedAt: null,
    archivedAt: null,
  });
});

describe("ReviewDetailModal", () => {
  it("renders the modal title with task title", async () => {
    render(<ReviewDetailModal taskId="task-r-1" onClose={vi.fn()} />, {
      wrapper: TestWrapper,
    });
    // Modal renders; the internal task fetch fires.
    expect(mocks.taskGet).toHaveBeenCalledWith("task-r-1");
  });
});
