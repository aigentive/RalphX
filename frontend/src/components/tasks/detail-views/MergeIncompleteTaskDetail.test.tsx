/**
 * MergeIncompleteTaskDetail component tests
 * Tests recovery timeline rendering with various event scenarios
 */

import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { MergeIncompleteTaskDetail } from "./MergeIncompleteTaskDetail";
import type { Task, MergeRecoveryEvent } from "@/types/task";

// Create stable mock functions using vi.hoisted
const { mockInvoke, mockSetTaskHistoryState } = vi.hoisted(() => ({
  mockInvoke: vi.fn(),
  mockSetTaskHistoryState: vi.fn(),
}));

// Mock Tauri API
vi.mock("@tauri-apps/api/core", () => ({
  invoke: mockInvoke,
}));

// Mock useMergePipeline to avoid invoke interference
vi.mock("@/hooks/useMergePipeline", () => ({
  useMergePipeline: vi.fn().mockReturnValue({ data: undefined }),
}));

// Mock usePlanBranchForTask to avoid consuming mockRejectedValueOnce
vi.mock("@/hooks/usePlanBranchForTask", () => ({
  usePlanBranchForTask: vi.fn().mockReturnValue({ data: undefined }),
}));

// Mock useUiStore
vi.mock("@/stores/uiStore", () => ({
  useUiStore: vi.fn((selector) => {
    if (selector) {
      return selector({ setTaskHistoryState: mockSetTaskHistoryState });
    }
    return { setTaskHistoryState: mockSetTaskHistoryState };
  }),
}));

function createTestTask(overrides?: Partial<Task>): Task {
  return {
    id: "task-123",
    projectId: "project-456",
    category: "feature",
    title: "Test Task Title",
    description: "Test task description",
    priority: 2,
    internalStatus: "merge_incomplete",
    needsReviewPoint: false,
    createdAt: "2026-01-28T12:00:00+00:00",
    updatedAt: "2026-01-28T12:00:00+00:00",
    startedAt: null,
    completedAt: null,
    archivedAt: null,
    blockedReason: null,
    taskBranch: "ralphx/ralphx/task-123",
    worktreePath: null,
    mergeCommitSha: null,
    metadata: null,
    ...overrides,
  };
}

function createRecoveryEvent(overrides?: Partial<MergeRecoveryEvent>): MergeRecoveryEvent {
  return {
    at: "2026-02-11T08:00:00+00:00",
    kind: "deferred",
    source: "auto",
    reason_code: "target_branch_busy",
    message: "Merge deferred due to concurrent merge",
    target_branch: "ralphx/ralphx/plan-main",
    source_branch: "ralphx/ralphx/task-123",
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

describe("MergeIncompleteTaskDetail", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockInvoke.mockResolvedValue(undefined);
  });

  it("renders container with status banner", () => {
    const task = createTestTask();
    render(<MergeIncompleteTaskDetail task={task} />, { wrapper: TestWrapper });

    expect(screen.getByTestId("merge-incomplete-task-detail")).toBeInTheDocument();
    expect(screen.getByText("Merge Incomplete")).toBeInTheDocument();
  });

  it("shows fallback message when no recovery events exist", () => {
    const task = createTestTask();
    render(<MergeIncompleteTaskDetail task={task} />, { wrapper: TestWrapper });

    expect(screen.getByTestId("recovery-attempts-section")).toBeInTheDocument();
    expect(screen.getByText("No recorded recovery attempts for this task.")).toBeInTheDocument();
  });

  it("renders recovery timeline when events exist", () => {
    const events: MergeRecoveryEvent[] = [
      createRecoveryEvent({
        kind: "deferred",
        message: "Merge deferred due to active merge on target",
      }),
      createRecoveryEvent({
        kind: "auto_retry_triggered",
        message: "Auto-retry triggered after blocker completed",
        at: "2026-02-11T08:05:00+00:00",
      }),
    ];

    const metadata = JSON.stringify({
      merge_recovery: {
        version: 1,
        events,
        last_state: "retrying",
      },
    });

    const task = createTestTask({ metadata });
    render(<MergeIncompleteTaskDetail task={task} />, { wrapper: TestWrapper });

    expect(screen.getByTestId("recovery-attempts-section")).toBeInTheDocument();
    expect(screen.getByText("Merge deferred due to active merge on target")).toBeInTheDocument();
    expect(screen.getByText("Auto-retry triggered after blocker completed")).toBeInTheDocument();
  });

  it("displays event metadata correctly", () => {
    const events: MergeRecoveryEvent[] = [
      createRecoveryEvent({
        kind: "deferred",
        message: "Deferred merge",
        blocking_task_id: "blocker-task-456",
        target_branch: "ralphx/ralphx/plan-main",
        reason_code: "target_branch_busy",
        attempt: 2,
      }),
    ];

    const metadata = JSON.stringify({
      merge_recovery: {
        version: 1,
        events,
        last_state: "deferred",
      },
    });

    const task = createTestTask({ metadata });
    render(<MergeIncompleteTaskDetail task={task} />, { wrapper: TestWrapper });

    // Check for metadata display (blocker ID is truncated to first 8 chars)
    expect(screen.getByText(/blocker-/i)).toBeInTheDocument();
    expect(screen.getByText(/ralphx\/ralphx\/plan-main/i)).toBeInTheDocument();
    expect(screen.getByText(/target branch busy/i)).toBeInTheDocument();
    expect(screen.getByText(/Attempt #2/i)).toBeInTheDocument();
  });

  it("shows auto-recovery badge when auto-retry events exist", () => {
    const events: MergeRecoveryEvent[] = [
      createRecoveryEvent({
        kind: "auto_retry_triggered",
        source: "auto",
        message: "Auto-retry triggered",
      }),
    ];

    const metadata = JSON.stringify({
      merge_recovery: {
        version: 1,
        events,
        last_state: "retrying",
      },
    });

    const task = createTestTask({ metadata });
    render(<MergeIncompleteTaskDetail task={task} />, { wrapper: TestWrapper });

    expect(screen.getByText("Auto-recovery attempted")).toBeInTheDocument();
  });

  it("shows deferred badge when deferred events exist", () => {
    const events: MergeRecoveryEvent[] = [
      createRecoveryEvent({
        kind: "deferred",
        message: "Merge deferred",
      }),
    ];

    const metadata = JSON.stringify({
      merge_recovery: {
        version: 1,
        events,
        last_state: "deferred",
      },
    });

    const task = createTestTask({ metadata });
    render(<MergeIncompleteTaskDetail task={task} />, { wrapper: TestWrapper });

    expect(screen.getByText("Deferred due to active merge")).toBeInTheDocument();
  });

  it("shows last attempt failed badge when last event is failure", () => {
    const events: MergeRecoveryEvent[] = [
      createRecoveryEvent({
        kind: "attempt_started",
        message: "Retry started",
      }),
      createRecoveryEvent({
        kind: "attempt_failed",
        message: "Retry failed",
        at: "2026-02-11T08:05:00+00:00",
      }),
    ];

    const metadata = JSON.stringify({
      merge_recovery: {
        version: 1,
        events,
        last_state: "failed",
      },
    });

    const task = createTestTask({ metadata });
    render(<MergeIncompleteTaskDetail task={task} />, { wrapper: TestWrapper });

    expect(screen.getByText("Last attempt failed")).toBeInTheDocument();
  });

  it("displays multiple event types in chronological order", () => {
    const events: MergeRecoveryEvent[] = [
      createRecoveryEvent({
        kind: "deferred",
        message: "First: deferred",
        at: "2026-02-11T08:00:00+00:00",
      }),
      createRecoveryEvent({
        kind: "auto_retry_triggered",
        message: "Second: retry triggered",
        at: "2026-02-11T08:05:00+00:00",
      }),
      createRecoveryEvent({
        kind: "attempt_started",
        message: "Third: attempt started",
        at: "2026-02-11T08:06:00+00:00",
      }),
      createRecoveryEvent({
        kind: "attempt_failed",
        message: "Fourth: attempt failed",
        at: "2026-02-11T08:07:00+00:00",
      }),
    ];

    const metadata = JSON.stringify({
      merge_recovery: {
        version: 1,
        events,
        last_state: "failed",
      },
    });

    const task = createTestTask({ metadata });
    render(<MergeIncompleteTaskDetail task={task} />, { wrapper: TestWrapper });

    expect(screen.getByText("First: deferred")).toBeInTheDocument();
    expect(screen.getByText("Second: retry triggered")).toBeInTheDocument();
    expect(screen.getByText("Third: attempt started")).toBeInTheDocument();
    expect(screen.getByText("Fourth: attempt failed")).toBeInTheDocument();
  });

  it("truncates oversized attempt messages and opens full output in a dialog", async () => {
    const user = userEvent.setup();
    const longMessage =
      "Failed to commit rebase+squash in worktree: stdout=[pre-commit][design-token guards] " +
      "error TS2307: Cannot find module 'zod'. ".repeat(12);
    const events: MergeRecoveryEvent[] = [
      createRecoveryEvent({
        kind: "attempt_failed",
        message: longMessage,
      }),
    ];

    const metadata = JSON.stringify({
      merge_recovery: {
        version: 1,
        events,
        last_state: "failed",
      },
    });

    const task = createTestTask({ metadata });
    render(<MergeIncompleteTaskDetail task={task} />, { wrapper: TestWrapper });

    expect(screen.queryByText(longMessage)).not.toBeInTheDocument();
    expect(screen.getByText(/Failed to commit rebase\+squash in worktree/i)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "View full output" })).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "View full output" }));

    const dialog = screen.getByTestId("merge-attempt-message-dialog");
    expect(dialog).toBeInTheDocument();
    expect(dialog).toHaveTextContent("Failed to commit rebase+squash in worktree");
    expect(dialog).toHaveTextContent("Cannot find module 'zod'");
  });

  it("preserves existing What Happened section", () => {
    const metadata = JSON.stringify({
      error: "Git error: branch locked",
      source_branch: "ralphx/ralphx/task-123",
      target_branch: "ralphx/ralphx/plan-main",
    });

    const task = createTestTask({ metadata });
    render(<MergeIncompleteTaskDetail task={task} />, { wrapper: TestWrapper });

    expect(screen.getByTestId("error-context-section")).toBeInTheDocument();
    expect(screen.getByText("What Happened")).toBeInTheDocument();
    expect(screen.getByText("Git error: branch locked")).toBeInTheDocument();
  });

  it("renders diagnostic info and supports manual_retry / attempt_succeeded event kinds", () => {
    const events: MergeRecoveryEvent[] = [
      createRecoveryEvent({
        kind: "manual_retry",
        source: "user",
        message: "User triggered manual retry",
        at: "2026-02-11T08:00:00+00:00",
      }),
      createRecoveryEvent({
        kind: "attempt_succeeded",
        source: "system",
        message: "Merge attempt succeeded",
        at: "2026-02-11T08:01:00+00:00",
      }),
    ];

    const metadata = JSON.stringify({
      error: "Git operation error: lock contention",
      diagnostic_info: "rebase aborted; index.lock present at .git/index.lock",
      source_branch: "ralphx/ralphx/task-123",
      target_branch: "ralphx/ralphx/plan-main",
      merge_recovery: {
        version: 1,
        events,
        last_state: "succeeded",
      },
    });

    const task = createTestTask({ metadata });
    render(<MergeIncompleteTaskDetail task={task} />, { wrapper: TestWrapper });

    // diagnosticInfo branch (covers patch line in ErrorContextCard)
    expect(
      screen.getByText(/rebase aborted; index\.lock present/i),
    ).toBeInTheDocument();

    // manual_retry + attempt_succeeded labels (covers patch lines in RecoveryTimeline labels/colors)
    expect(screen.getByText("Manual Retry")).toBeInTheDocument();
    expect(screen.getByText("Succeeded")).toBeInTheDocument();
    expect(screen.getByText("User triggered manual retry")).toBeInTheDocument();
    expect(screen.getByText("Merge attempt succeeded")).toBeInTheDocument();
  });

  it("explains hook environment failures as escalated setup issues, not code changes", () => {
    const metadata = JSON.stringify({
      error:
        "Git operation error: Failed to commit rebase+squash in worktree: stderr=[pre-commit] typecheck\nsrc/api/task-graph.ts(7,19): error TS2307: Cannot find module 'zod'",
      merge_hook_failure_kind: "environment_failure",
      merge_hook_blocked_reason: "hook_environment_failure",
      merge_hook_failure_repeat_count: 0,
    });

    const task = createTestTask({ metadata });
    render(<MergeIncompleteTaskDetail task={task} />, { wrapper: TestWrapper });

    expect(screen.getAllByText("Escalated").length).toBeGreaterThan(0);
    expect(
      screen.getByText("Repository hook environment failed — action required"),
    ).toBeInTheDocument();
    expect(
      screen.getByText(/could not run reliably in this isolated worktree environment/i),
    ).toBeInTheDocument();
    expect(
      screen.getByText(/Fix the hook dependencies or worktree setup/i),
    ).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: /Retry after environment fix/i }),
    ).toBeInTheDocument();
  });

  it("explains repeated hook failures as a stopped loop", () => {
    const metadata = JSON.stringify({
      error:
        "Git operation error: Failed to commit rebase+squash in worktree: stderr=[pre-commit] design-token guards failed",
      merge_hook_failure_kind: "policy_failure",
      merge_hook_blocked_reason: "repeated_hook_failure",
      merge_hook_failure_repeat_count: 1,
    });

    const task = createTestTask({ metadata });
    render(<MergeIncompleteTaskDetail task={task} />, { wrapper: TestWrapper });

    expect(screen.getAllByText("Escalated").length).toBeGreaterThan(0);
    expect(
      screen.getByText("Same repository hook failure repeated — loop stopped"),
    ).toBeInTheDocument();
    expect(screen.getByText(/Repeat count: 1/i)).toBeInTheDocument();
    expect(
      screen.getByText(/stopped the automatic loop/i),
    ).toBeInTheDocument();
  });

  it("truncates oversized What Happened output and opens full output in a dialog", async () => {
    const user = userEvent.setup();
    const longError =
      "Git operation error: Failed to commit rebase+squash in worktree: stdout=, stderr=[pre-commit] " +
      "error TS2307: Cannot find module 'zod'.\n".repeat(40);
    const metadata = JSON.stringify({
      error: longError,
      source_branch: "ralphx/ralphx/task-123",
      target_branch: "ralphx/ralphx/plan-main",
    });

    const task = createTestTask({ metadata });
    render(<MergeIncompleteTaskDetail task={task} />, { wrapper: TestWrapper });

    expect(screen.queryByText(longError)).not.toBeInTheDocument();
    expect(screen.getByText(/Git operation error: Failed to commit rebase\+squash in worktree/i)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "View full output" })).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: "View full output" }));

    const dialog = screen.getByTestId("merge-error-context-dialog");
    expect(dialog).toBeInTheDocument();
    expect(dialog).toHaveTextContent("Failed to commit rebase+squash in worktree");
    expect(dialog).toHaveTextContent("Cannot find module 'zod'");
  });

  it("preserves action buttons in non-historical mode", () => {
    const task = createTestTask();
    render(<MergeIncompleteTaskDetail task={task} />, { wrapper: TestWrapper });

    expect(screen.getByTestId("action-buttons")).toBeInTheDocument();
    expect(screen.getByTestId("retry-merge-button")).toBeInTheDocument();
    expect(screen.getByTestId("resolve-merge-button")).toBeInTheDocument();
  });

  it("hides action buttons in historical mode", () => {
    const task = createTestTask();
    render(<MergeIncompleteTaskDetail task={task} isHistorical />, {
      wrapper: TestWrapper,
    });

    expect(screen.queryByTestId("action-buttons")).not.toBeInTheDocument();
  });

  it("handles malformed metadata gracefully", () => {
    const task = createTestTask({ metadata: "not valid json" });
    render(<MergeIncompleteTaskDetail task={task} />, { wrapper: TestWrapper });

    // Should still render with fallback
    expect(screen.getByTestId("merge-incomplete-task-detail")).toBeInTheDocument();
    expect(screen.getByText("No recorded recovery attempts for this task.")).toBeInTheDocument();
  });

  describe("handleRetryMerge error handling", () => {
    it("displays string rejection verbatim", async () => {
      const user = userEvent.setup();
      mockInvoke.mockRejectedValueOnce("Git merge failed: branch deleted");

      const task = createTestTask();
      render(<MergeIncompleteTaskDetail task={task} />, { wrapper: TestWrapper });

      await user.click(screen.getByTestId("retry-merge-button"));

      await waitFor(() => {
        expect(screen.getByText("Git merge failed: branch deleted")).toBeInTheDocument();
      });
    });

    it("displays object rejection with .message verbatim", async () => {
      const user = userEvent.setup();
      mockInvoke.mockRejectedValueOnce({ message: "Lock file exists at .git/index.lock" });

      const task = createTestTask();
      render(<MergeIncompleteTaskDetail task={task} />, { wrapper: TestWrapper });

      await user.click(screen.getByTestId("retry-merge-button"));

      await waitFor(() => {
        expect(screen.getByText("Lock file exists at .git/index.lock")).toBeInTheDocument();
      });
    });

    it("displays fallback for unknown object rejection", async () => {
      const user = userEvent.setup();
      mockInvoke.mockRejectedValueOnce({});

      const task = createTestTask();
      render(<MergeIncompleteTaskDetail task={task} />, { wrapper: TestWrapper });

      await user.click(screen.getByTestId("retry-merge-button"));

      await waitFor(() => {
        expect(screen.getByText("Failed to retry merge")).toBeInTheDocument();
      });
    });
  });

  describe("handleRetrySkipValidation error handling", () => {
    function createValidationFailureTask(): Task {
      const metadata = JSON.stringify({
        error: "Validation failed",
        validation_failures: [{ command: "npm run typecheck", exit_code: 1 }],
      });
      return createTestTask({ metadata });
    }

    it("displays string rejection verbatim", async () => {
      const user = userEvent.setup();
      mockInvoke.mockRejectedValueOnce("Merge failed: cannot fast-forward");

      const task = createValidationFailureTask();
      render(<MergeIncompleteTaskDetail task={task} />, { wrapper: TestWrapper });

      await user.click(screen.getByTestId("retry-skip-validation-button"));

      await waitFor(() => {
        expect(screen.getByText("Merge failed: cannot fast-forward")).toBeInTheDocument();
      });
    });

    it("displays object rejection with .message verbatim", async () => {
      const user = userEvent.setup();
      mockInvoke.mockRejectedValueOnce({ message: "Worktree directory missing" });

      const task = createValidationFailureTask();
      render(<MergeIncompleteTaskDetail task={task} />, { wrapper: TestWrapper });

      await user.click(screen.getByTestId("retry-skip-validation-button"));

      await waitFor(() => {
        expect(screen.getByText("Worktree directory missing")).toBeInTheDocument();
      });
    });

    it("displays fallback for unknown object rejection", async () => {
      const user = userEvent.setup();
      mockInvoke.mockRejectedValueOnce({});

      const task = createValidationFailureTask();
      render(<MergeIncompleteTaskDetail task={task} />, { wrapper: TestWrapper });

      await user.click(screen.getByTestId("retry-skip-validation-button"));

      await waitFor(() => {
        expect(screen.getByText("Failed to retry merge (skipping validation)")).toBeInTheDocument();
      });
    });
  });

  describe("handleMarkResolved error handling", () => {
    it("displays string rejection verbatim", async () => {
      const user = userEvent.setup();
      mockInvoke.mockRejectedValueOnce("Task not in merge_incomplete state");

      const task = createTestTask();
      render(<MergeIncompleteTaskDetail task={task} />, { wrapper: TestWrapper });

      await user.click(screen.getByTestId("resolve-merge-button"));

      await waitFor(() => {
        expect(screen.getByText("Task not in merge_incomplete state")).toBeInTheDocument();
      });
    });

    it("displays object rejection with .message verbatim", async () => {
      const user = userEvent.setup();
      mockInvoke.mockRejectedValueOnce({ message: "Branch not found: ralphx/task-123" });

      const task = createTestTask();
      render(<MergeIncompleteTaskDetail task={task} />, { wrapper: TestWrapper });

      await user.click(screen.getByTestId("resolve-merge-button"));

      await waitFor(() => {
        expect(screen.getByText("Branch not found: ralphx/task-123")).toBeInTheDocument();
      });
    });

    it("displays fallback for unknown object rejection", async () => {
      const user = userEvent.setup();
      mockInvoke.mockRejectedValueOnce({});

      const task = createTestTask();
      render(<MergeIncompleteTaskDetail task={task} />, { wrapper: TestWrapper });

      await user.click(screen.getByTestId("resolve-merge-button"));

      await waitFor(() => {
        expect(screen.getByText("Failed to mark merge as resolved")).toBeInTheDocument();
      });
    });
  });

  it("exits history mode and optimistically updates to pending_merge when retry is clicked", async () => {
    const user = userEvent.setup();
    const queryClient = new QueryClient({
      defaultOptions: {
        queries: { retry: false },
      },
    });

    // Set up initial task data in the query cache
    const task = createTestTask();
    queryClient.setQueryData(["tasks", "list", task.projectId], [task]);

    render(
      <QueryClientProvider client={queryClient}>
        <MergeIncompleteTaskDetail task={task} />
      </QueryClientProvider>
    );

    // Click retry button
    const retryButton = screen.getByTestId("retry-merge-button");
    await user.click(retryButton);

    // Verify setHistoryState(null) was called to exit history mode
    expect(mockSetTaskHistoryState).toHaveBeenCalledWith(null);

    // Verify the query cache was optimistically updated
    await waitFor(() => {
      const cachedTasks = queryClient.getQueryData<Task[]>(["tasks", "list", task.projectId]);
      expect(cachedTasks).toBeDefined();
      expect(cachedTasks?.[0]?.internalStatus).toBe("pending_merge");
    });

    // Verify backend command was invoked
    expect(mockInvoke).toHaveBeenCalledWith("retry_merge", { taskId: task.id });
  });

  it("exits history mode and optimistically updates to pending_merge when retry skip validation is clicked", async () => {
    const user = userEvent.setup();
    const queryClient = new QueryClient({
      defaultOptions: {
        queries: { retry: false },
      },
    });

    // Create task with validation failures to show skip validation button
    const metadata = JSON.stringify({
      validation_failures: [
        { command: "npm run typecheck", exit_code: 1, stderr: "Type error" }
      ]
    });
    const task = createTestTask({ metadata });
    queryClient.setQueryData(["tasks", "list", task.projectId], [task]);

    render(
      <QueryClientProvider client={queryClient}>
        <MergeIncompleteTaskDetail task={task} />
      </QueryClientProvider>
    );

    // Click retry skip validation button
    const retrySkipButton = screen.getByTestId("retry-skip-validation-button");
    await user.click(retrySkipButton);

    // Verify setHistoryState(null) was called to exit history mode
    expect(mockSetTaskHistoryState).toHaveBeenCalledWith(null);

    // Verify the query cache was optimistically updated
    await waitFor(() => {
      const cachedTasks = queryClient.getQueryData<Task[]>(["tasks", "list", task.projectId]);
      expect(cachedTasks).toBeDefined();
      expect(cachedTasks?.[0]?.internalStatus).toBe("pending_merge");
    });

    // Verify backend command was invoked with skipValidation flag
    expect(mockInvoke).toHaveBeenCalledWith("retry_merge", { taskId: task.id, skipValidation: true });
  });

  it("rolls back optimistic update when retry fails", async () => {
    const user = userEvent.setup();
    const queryClient = new QueryClient({
      defaultOptions: {
        queries: { retry: false },
      },
    });

    // Mock invoke to fail
    mockInvoke.mockRejectedValueOnce(new Error("Merge failed"));

    const task = createTestTask();
    queryClient.setQueryData(["tasks", "list", task.projectId], [task]);

    render(
      <QueryClientProvider client={queryClient}>
        <MergeIncompleteTaskDetail task={task} />
      </QueryClientProvider>
    );

    // Click retry button
    const retryButton = screen.getByTestId("retry-merge-button");
    await user.click(retryButton);

    // Wait for error handling
    await waitFor(() => {
      expect(screen.getByText("Merge failed")).toBeInTheDocument();
    });

    // Verify query was invalidated to rollback optimistic update
    expect(mockInvoke).toHaveBeenCalledWith("retry_merge", { taskId: task.id });
  });
});
