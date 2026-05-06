import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";

import { EscalatedTaskDetail } from "./EscalatedTaskDetail";
import type { Task } from "@/types/task";

const { reviewIssuesGetByTaskId } = vi.hoisted(() => ({
  reviewIssuesGetByTaskId: vi.fn().mockResolvedValue([]),
}));

vi.mock("@/api/review-issues", () => ({
  reviewIssuesApi: { getByTaskId: (id: string) => reviewIssuesGetByTaskId(id) },
}));

vi.mock("@/hooks/useReviews", () => ({
  useTaskStateHistory: () => ({
    data: [
      {
        id: "h1",
        outcome: "escalated",
        notes: "Need human input on tradeoff",
        feedback: "Cannot decide between X and Y",
        reviewer: "ai_reviewer",
        created_at: "2026-04-22T12:00:00Z",
        followup_session_id: "session-followup-1",
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
      reReviewTask: vi.fn().mockResolvedValue(undefined),
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
    id: "task-esc-1",
    projectId: "proj-1",
    category: "feature",
    title: "Resolve API tradeoff",
    description: "Decide REST vs gRPC",
    priority: 2,
    internalStatus: "escalated",
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

describe("EscalatedTaskDetail", () => {
  it("renders the escalation header section", () => {
    render(<EscalatedTaskDetail task={task()} />, { wrapper: TestWrapper });
    expect(screen.getByTestId("escalated-task-detail")).toBeInTheDocument();
    expect(screen.getByText("Why AI Escalated")).toBeInTheDocument();
  });

  it("renders the follow-up session card when escalation review has followup_session_id", () => {
    render(<EscalatedTaskDetail task={task()} />, { wrapper: TestWrapper });
    expect(screen.getByTestId("followup-session-section")).toBeInTheDocument();
    expect(screen.getByText("session-followup-1")).toBeInTheDocument();
  });
});
