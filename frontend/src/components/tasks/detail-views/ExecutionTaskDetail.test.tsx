/**
 * ExecutionTaskDetail component tests
 */

import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { ExecutionTaskDetail } from "./ExecutionTaskDetail";
import type { Task } from "@/types/task";
import type { ReviewNoteResponse } from "@/lib/tauri";

vi.mock("@/hooks/useTaskSteps", () => ({
  useTaskSteps: vi.fn(),
  useStepProgress: vi.fn(),
}));

vi.mock("@/hooks/useReviews", () => ({
  useTaskStateHistory: vi.fn(),
}));

vi.mock("@/api/review-issues", () => ({
  reviewIssuesApi: {
    getByTaskId: vi.fn().mockResolvedValue([]),
  },
}));

const mockConfirmation = {
  confirm: vi.fn(async () => true),
  confirmationDialogProps: {},
  ConfirmationDialog: () => null,
};

vi.mock("@/hooks/useConfirmation", () => ({
  useConfirmation: vi.fn(() => mockConfirmation),
}));

vi.mock("@/lib/tauri", () => ({
  api: {
    tasks: {
      stop: vi.fn(async () => ({})),
      move: vi.fn(async () => ({})),
    },
  },
}));

// Mock EventBus for useValidationEvents hook
const mockListeners = new Map<string, Set<(payload: unknown) => void>>();
const stableBus = {
  subscribe: (eventName: string, callback: (payload: unknown) => void) => {
    if (!mockListeners.has(eventName)) {
      mockListeners.set(eventName, new Set());
    }
    mockListeners.get(eventName)!.add(callback);
    return () => {
      mockListeners.get(eventName)?.delete(callback);
    };
  },
};

vi.mock("@/providers/EventProvider", () => ({
  useEventBus: () => stableBus,
}));

const mockStepList = vi.fn(({ taskId, editable }) => (
  <div data-testid="mock-step-list" data-task-id={taskId} data-editable={String(editable)} />
));

vi.mock("../StepList", () => ({
  StepList: (props: unknown) => mockStepList(props),
}));

import { useTaskSteps, useStepProgress } from "@/hooks/useTaskSteps";
import { useTaskStateHistory } from "@/hooks/useReviews";
import { api } from "@/lib/tauri";

const mockUseTaskSteps = vi.mocked(useTaskSteps);
const mockUseStepProgress = vi.mocked(useStepProgress);
const mockUseTaskStateHistory = vi.mocked(useTaskStateHistory);
const mockApiTasksStop = vi.mocked(api.tasks.stop);
const mockApiTasksMove = vi.mocked(api.tasks.move);

function createTestTask(overrides?: Partial<Task>): Task {
  return {
    id: "task-123",
    projectId: "project-456",
    category: "feature",
    title: "Test Task Title",
    description: "Test task description",
    priority: 2,
    internalStatus: "executing",
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

describe("ExecutionTaskDetail", () => {
  beforeEach(() => {
    vi.clearAllMocks();

    mockUseTaskSteps.mockReturnValue({
      data: [],
      isLoading: false,
      isError: false,
    } as ReturnType<typeof useTaskSteps>);

    mockUseStepProgress.mockReturnValue({
      data: {
        total: 0,
        completed: 0,
        inProgress: 0,
        pending: 0,
        skipped: 0,
        failed: 0,
        percentComplete: 0,
      },
      isLoading: false,
      isError: false,
    } as ReturnType<typeof useStepProgress>);

    mockUseTaskStateHistory.mockReturnValue({
      data: [],
      isLoading: false,
      error: null,
      isEmpty: true,
      latestEntry: null,
      refetch: vi.fn(),
    });
  });

  it("renders executing status banner and description", () => {
    const task = createTestTask({
      internalStatus: "executing",
      description: "Task description here",
    });
    render(<ExecutionTaskDetail task={task} />, { wrapper: TestWrapper });

    expect(screen.getByTestId("execution-task-detail")).toBeInTheDocument();
    expect(screen.getByText("Executing Task")).toBeInTheDocument();
    expect(screen.getByText("Live")).toBeInTheDocument();
    expect(screen.getByText("Task description here")).toBeInTheDocument();
  });

  it("renders progress section when step progress exists", () => {
    const task = createTestTask();
    mockUseStepProgress.mockReturnValue({
      data: {
        total: 4,
        completed: 2,
        inProgress: 1,
        pending: 1,
        skipped: 0,
        failed: 0,
        percentComplete: 50,
      },
      isLoading: false,
      isError: false,
    } as ReturnType<typeof useStepProgress>);

    render(<ExecutionTaskDetail task={task} />, { wrapper: TestWrapper });

    const progressSection = screen.getByTestId("execution-progress-section");
    expect(progressSection).toBeInTheDocument();
    expect(screen.getByText("50%")).toBeInTheDocument();
    expect(progressSection.textContent).toContain("Step 2 of 4");
  });

  it("excludes skipped steps from the progress step denominator", () => {
    const task = createTestTask();
    mockUseStepProgress.mockReturnValue({
      data: {
        total: 4,
        completed: 3,
        inProgress: 0,
        pending: 0,
        skipped: 1,
        failed: 0,
        percentComplete: 100,
      },
      isLoading: false,
      isError: false,
    } as ReturnType<typeof useStepProgress>);

    render(<ExecutionTaskDetail task={task} />, { wrapper: TestWrapper });

    const progressSection = screen.getByTestId("execution-progress-section");
    expect(screen.getByText("100%")).toBeInTheDocument();
    expect(progressSection.textContent).toContain("Step 3 of 3");
    expect(progressSection.textContent).not.toContain("Step 3 of 4");
  });

  it("renders revision feedback section for re_executing tasks", () => {
    const task = createTestTask({ internalStatus: "re_executing" });
    const reviewNote: ReviewNoteResponse = {
      id: "note-1",
      task_id: task.id,
      reviewer: "ai",
      outcome: "changes_requested",
      notes: "Missing error handling in auth.ts",
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

    render(<ExecutionTaskDetail task={task} />, { wrapper: TestWrapper });

    expect(screen.getByTestId("revision-feedback-banner")).toBeInTheDocument();
    expect(screen.getByText("Feedback Being Addressed")).toBeInTheDocument();
    expect(screen.getByText("Addressing")).toBeInTheDocument();
    expect(screen.getByText("Missing error handling in auth.ts")).toBeInTheDocument();
  });

  it("renders system review feedback with preview + dialog for large hook notes", async () => {
    const user = userEvent.setup();
    const task = createTestTask({ internalStatus: "re_executing" });
    const reviewNote: ReviewNoteResponse = {
      id: "note-hook",
      task_id: task.id,
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

    render(<ExecutionTaskDetail task={task} />, { wrapper: TestWrapper });

    expect(screen.getByText("System Feedback")).toBeInTheDocument();
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

  it("shows revision feedback loading state while history is loading", () => {
    const task = createTestTask({ internalStatus: "re_executing" });
    mockUseTaskStateHistory.mockReturnValue({
      data: [],
      isLoading: true,
      error: null,
      isEmpty: true,
      latestEntry: null,
      refetch: vi.fn(),
    });

    render(<ExecutionTaskDetail task={task} />, { wrapper: TestWrapper });

    expect(screen.getByTestId("revision-feedback-banner")).toBeInTheDocument();
    expect(screen.queryByText("AI Feedback")).not.toBeInTheDocument();
  });

  it("renders revision feedback section for re_executing tasks with human reviewer", () => {
    const task = createTestTask({ internalStatus: "re_executing" });
    const reviewNote: ReviewNoteResponse = {
      id: "note-2",
      task_id: task.id,
      reviewer: "human",
      outcome: "changes_requested",
      notes: "Please add better error messages",
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

    render(<ExecutionTaskDetail task={task} />, { wrapper: TestWrapper });

    expect(screen.getByTestId("revision-feedback-banner")).toBeInTheDocument();
    expect(screen.getByText("Feedback Being Addressed")).toBeInTheDocument();
    expect(screen.getByText("Human Feedback")).toBeInTheDocument();
    expect(screen.queryByText("AI Feedback")).not.toBeInTheDocument();
    expect(screen.getByText("Please add better error messages")).toBeInTheDocument();
  });

  it("hides revision feedback section when re_executing but no feedback and not loading", () => {
    const task = createTestTask({ internalStatus: "re_executing" });
    mockUseTaskStateHistory.mockReturnValue({
      data: [],
      isLoading: false,
      error: null,
      isEmpty: true,
      latestEntry: null,
      refetch: vi.fn(),
    });

    render(<ExecutionTaskDetail task={task} />, { wrapper: TestWrapper });

    expect(screen.queryByTestId("revision-feedback-banner")).not.toBeInTheDocument();
  });

  it("renders steps section when task has steps", () => {
    const task = createTestTask();
    mockUseTaskSteps.mockReturnValue({
      data: [
        {
          id: "step-1",
          taskId: task.id,
          title: "Step 1",
          status: "completed",
          order: 0,
          createdAt: "2026-01-28T12:00:00+00:00",
          updatedAt: "2026-01-28T12:00:00+00:00",
        },
      ],
      isLoading: false,
      isError: false,
    } as ReturnType<typeof useTaskSteps>);

    render(<ExecutionTaskDetail task={task} />, { wrapper: TestWrapper });

    expect(screen.getByTestId("execution-steps-section")).toBeInTheDocument();
    expect(screen.getByTestId("mock-step-list")).toBeInTheDocument();
  });

  it("shows loading state while fetching steps", () => {
    const task = createTestTask();
    mockUseTaskSteps.mockReturnValue({
      data: undefined,
      isLoading: true,
      isError: false,
    } as ReturnType<typeof useTaskSteps>);

    render(<ExecutionTaskDetail task={task} />, { wrapper: TestWrapper });

    expect(screen.getByTestId("execution-steps-loading")).toBeInTheDocument();
  });

  describe("action buttons dropdown", () => {
    beforeEach(() => {
      vi.clearAllMocks();
      mockConfirmation.confirm = vi.fn(async () => true);
    });

    it("renders action buttons section when task is executing", () => {
      const task = createTestTask({ internalStatus: "executing" });
      render(<ExecutionTaskDetail task={task} />, { wrapper: TestWrapper });

      expect(screen.getByTestId("action-buttons-section")).toBeInTheDocument();
      expect(screen.getByTestId("action-dropdown-trigger")).toBeInTheDocument();
    });

    it("does not render action buttons when isHistorical is true", () => {
      const task = createTestTask({ internalStatus: "executing" });
      render(<ExecutionTaskDetail task={task} isHistorical={true} />, {
        wrapper: TestWrapper,
      });

      expect(screen.queryByTestId("action-buttons-section")).not.toBeInTheDocument();
    });

    it("displays stop and cancel options in dropdown menu", async () => {
      const user = userEvent.setup();
      const task = createTestTask({ internalStatus: "executing" });
      render(<ExecutionTaskDetail task={task} />, { wrapper: TestWrapper });

      const trigger = screen.getByTestId("action-dropdown-trigger");
      await user.click(trigger);

      expect(screen.getByTestId("stop-action")).toBeInTheDocument();
      expect(screen.getByTestId("cancel-action")).toBeInTheDocument();
    });

    it("calls api.tasks.stop when stop action is clicked", async () => {
      const user = userEvent.setup();
      const task = createTestTask({ internalStatus: "executing" });
      mockConfirmation.confirm = vi.fn(async () => true);

      render(<ExecutionTaskDetail task={task} />, { wrapper: TestWrapper });

      const trigger = screen.getByTestId("action-dropdown-trigger");
      await user.click(trigger);

      const stopAction = screen.getByTestId("stop-action");
      await user.click(stopAction);

      await waitFor(() => {
        expect(mockApiTasksStop).toHaveBeenCalledWith(task.id);
      });
    });

    it("calls api.tasks.move with cancelled when cancel action is clicked", async () => {
      const user = userEvent.setup();
      const task = createTestTask({ internalStatus: "executing" });
      mockConfirmation.confirm = vi.fn(async () => true);

      render(<ExecutionTaskDetail task={task} />, { wrapper: TestWrapper });

      const trigger = screen.getByTestId("action-dropdown-trigger");
      await user.click(trigger);

      const cancelAction = screen.getByTestId("cancel-action");
      await user.click(cancelAction);

      await waitFor(() => {
        expect(mockApiTasksMove).toHaveBeenCalledWith(task.id, "cancelled");
      });
    });

    it("does not call api when confirmation is cancelled", async () => {
      const user = userEvent.setup();
      const task = createTestTask({ internalStatus: "executing" });
      mockConfirmation.confirm = vi.fn(async () => false);

      render(<ExecutionTaskDetail task={task} />, { wrapper: TestWrapper });

      const trigger = screen.getByTestId("action-dropdown-trigger");
      await user.click(trigger);

      const stopAction = screen.getByTestId("stop-action");
      await user.click(stopAction);

      expect(mockApiTasksStop).not.toHaveBeenCalled();
    });

    it("renders an error paragraph when the stop mutation rejects", async () => {
      const user = userEvent.setup();
      const task = createTestTask({ internalStatus: "executing" });
      mockConfirmation.confirm = vi.fn(async () => true);
      mockApiTasksStop.mockRejectedValueOnce(new Error("backend offline"));

      render(<ExecutionTaskDetail task={task} />, { wrapper: TestWrapper });

      await user.click(screen.getByTestId("action-dropdown-trigger"));
      await user.click(screen.getByTestId("stop-action"));

      // The mutation onError handler stores the message and renders the
      // text-[0.75rem] error paragraph branch.
      await waitFor(() => {
        expect(screen.getByText("backend offline")).toBeInTheDocument();
      });
    });
  });
});
