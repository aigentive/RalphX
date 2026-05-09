import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { RevisionTaskDetail } from "./RevisionTaskDetail";
import type { Task } from "@/types/task";
import type { ReviewNoteResponse } from "@/lib/tauri";

vi.mock("@/hooks/useTaskSteps", () => ({
  useTaskSteps: vi.fn(),
}));

vi.mock("@/hooks/useReviews", () => ({
  useTaskStateHistory: vi.fn(),
}));

vi.mock("@/api/review-issues", () => ({
  reviewIssuesApi: {
    getByTaskId: vi.fn().mockResolvedValue([]),
  },
}));

vi.mock("../StepList", () => ({
  StepList: () => <div data-testid="mock-step-list" />,
}));

import { useTaskSteps } from "@/hooks/useTaskSteps";
import { useTaskStateHistory } from "@/hooks/useReviews";

const mockUseTaskSteps = vi.mocked(useTaskSteps);
const mockUseTaskStateHistory = vi.mocked(useTaskStateHistory);

function createTestTask(overrides?: Partial<Task>): Task {
  return {
    id: "task-123",
    projectId: "project-456",
    category: "feature",
    title: "Revision Task",
    description: "Task description",
    priority: 2,
    internalStatus: "revision_needed",
    needsReviewPoint: false,
    sourceProposalId: null,
    planArtifactId: null,
    createdAt: "2026-01-28T12:00:00+00:00",
    updatedAt: "2026-01-28T12:00:00+00:00",
    startedAt: "2026-01-28T12:00:00+00:00",
    completedAt: null,
    archivedAt: null,
    ...overrides,
  };
}

function TestWrapper({ children }: { children: React.ReactNode }) {
  const queryClient = new QueryClient({
    defaultOptions: {
      queries: { retry: false },
    },
  });
  return (
    <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
  );
}

describe("RevisionTaskDetail", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockUseTaskSteps.mockReturnValue({
      data: [],
      isLoading: false,
      isError: false,
    } as ReturnType<typeof useTaskSteps>);
    mockUseTaskStateHistory.mockReturnValue({
      data: [],
      isLoading: false,
      error: null,
      isEmpty: true,
      latestEntry: null,
      refetch: vi.fn(),
    });
  });

  it("renders system review feedback with preview + dialog for large hook notes", async () => {
    const user = userEvent.setup();
    const reviewNote: ReviewNoteResponse = {
      id: "note-hook",
      task_id: "task-123",
      reviewer: "system",
      outcome: "changes_requested",
      summary: "Repository commit hooks rejected the merge commit.",
      notes: [
        "Repository commit hooks rejected the merge commit.",
        "",
        "Full hook output:",
        "```text",
        "\u001b[31m[pre-commit]\u001b[0m design-token guards failed",
        ...Array.from({ length: 240 }, (_, index) => `TS2307 Cannot find module 'zod' — extended diagnostic ${index}`),
        "```",
      ].join("\n"),
      created_at: "2026-01-28T11:00:00+00:00",
    };

    mockUseTaskStateHistory.mockReturnValue({
      data: [reviewNote],
      isLoading: false,
      error: null,
      isEmpty: false,
      latestEntry: reviewNote,
      refetch: vi.fn(),
    });

    render(<RevisionTaskDetail task={createTestTask()} />, { wrapper: TestWrapper });

    expect(screen.getByText("Feedback to Address")).toBeInTheDocument();
    expect(screen.getByText("System Review Feedback")).toBeInTheDocument();
    expect(
      screen.getByText("Repository commit hooks rejected the merge commit.")
    ).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "View full feedback" }));

    expect(screen.getByText("Full revision feedback")).toBeInTheDocument();
    expect(screen.getByText(/design-token guards failed/)).toBeInTheDocument();
    expect(
      screen.queryByText((content) => content.includes("\u001b[31m"))
    ).not.toBeInTheDocument();
  });
});
