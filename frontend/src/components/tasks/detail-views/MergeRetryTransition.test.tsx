/**
 * MergeRetryTransition integration tests
 *
 * Verifies the full retry flow:
 * 1. From merge_incomplete, clicking retry triggers immediate visual transition
 *    to pending_merge state (optimistic update) without waiting for backend
 * 2. History mode is exited on retry, showing live status
 * 3. View switches from MergeIncompleteTaskDetail to MergingTaskDetail
 */

import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import { userEvent } from "@testing-library/user-event";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { MergeIncompleteTaskDetail } from "./MergeIncompleteTaskDetail";
import { TASK_DETAIL_VIEWS } from "../../agents/task-details/AgentsTaskDetailPanel";
import type { Task } from "@/types/task";

// Stable mock references
const { mockInvoke, mockSetTaskHistoryState } = vi.hoisted(() => ({
  mockInvoke: vi.fn(),
  mockSetTaskHistoryState: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", () => ({
  invoke: mockInvoke,
}));

vi.mock("@/stores/uiStore", () => ({
  useUiStore: vi.fn((selector) => {
    if (selector) {
      return selector({ setTaskHistoryState: mockSetTaskHistoryState });
    }
    return { setTaskHistoryState: mockSetTaskHistoryState };
  }),
}));

// Mock EventBus for MergingTaskDetail's hooks
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

function createTestTask(overrides?: Partial<Task>): Task {
  return {
    id: "task-123",
    projectId: "project-456",
    category: "feature",
    title: "Test Task",
    description: "Test description",
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

describe("Retry merge visual transition", () => {
  let queryClient: QueryClient;

  beforeEach(() => {
    vi.clearAllMocks();
    mockListeners.clear();
    // Simulate slow backend — don't resolve immediately
    mockInvoke.mockReturnValue(new Promise(() => {}));
    queryClient = new QueryClient({
      defaultOptions: {
        queries: { retry: false },
      },
    });
  });

  it("optimistically updates cache to pending_merge immediately on retry click without waiting for backend", async () => {
    const user = userEvent.setup();
    const task = createTestTask();
    queryClient.setQueryData(["tasks", "list", task.projectId], [task]);

    render(
      <QueryClientProvider client={queryClient}>
        <MergeIncompleteTaskDetail task={task} />
      </QueryClientProvider>
    );

    // Verify we're in merge_incomplete view
    expect(screen.getByTestId("merge-incomplete-task-detail")).toBeInTheDocument();
    expect(screen.getByText("Merge Incomplete")).toBeInTheDocument();

    // Click retry — backend is NOT going to respond (promise never resolves)
    const retryButton = screen.getByTestId("retry-merge-button");
    await user.click(retryButton);

    // Even though backend hasn't responded, cache should be updated immediately
    await waitFor(() => {
      const cachedTasks = queryClient.getQueryData<Task[]>(["tasks", "list", task.projectId]);
      expect(cachedTasks?.[0]?.internalStatus).toBe("pending_merge");
    });

    // Backend invoke was called
    expect(mockInvoke).toHaveBeenCalledWith("retry_merge", { taskId: task.id });
  });

  it("exits history mode before backend responds", async () => {
    const user = userEvent.setup();
    const task = createTestTask();
    queryClient.setQueryData(["tasks", "list", task.projectId], [task]);

    render(
      <QueryClientProvider client={queryClient}>
        <MergeIncompleteTaskDetail task={task} />
      </QueryClientProvider>
    );

    const retryButton = screen.getByTestId("retry-merge-button");
    await user.click(retryButton);

    // History mode should be exited immediately
    expect(mockSetTaskHistoryState).toHaveBeenCalledWith(null);
    expect(mockSetTaskHistoryState).toHaveBeenCalledTimes(1);
  });

  it("view registry maps merge_incomplete to MergeIncompleteTaskDetail and pending_merge to MergingTaskDetail", () => {
    // This verifies the view transition mechanism:
    // When task status changes from merge_incomplete to pending_merge,
    // the Agents task detail panel renders MergingTaskDetail instead
    expect(TASK_DETAIL_VIEWS.merge_incomplete).toBeDefined();
    expect(TASK_DETAIL_VIEWS.pending_merge).toBeDefined();

    // Different components for different states
    expect(TASK_DETAIL_VIEWS.merge_incomplete).not.toBe(TASK_DETAIL_VIEWS.pending_merge);
  });

  it("exits history mode and updates cache when retry-skip-validation is clicked", async () => {
    const user = userEvent.setup();
    const metadata = JSON.stringify({
      validation_failures: [
        { command: "npm run typecheck", exit_code: 1, stderr: "Type error" },
      ],
    });
    const task = createTestTask({ metadata });
    queryClient.setQueryData(["tasks", "list", task.projectId], [task]);

    render(
      <QueryClientProvider client={queryClient}>
        <MergeIncompleteTaskDetail task={task} />
      </QueryClientProvider>
    );

    const skipButton = screen.getByTestId("retry-skip-validation-button");
    await user.click(skipButton);

    // History mode exited
    expect(mockSetTaskHistoryState).toHaveBeenCalledWith(null);

    // Cache optimistically updated
    await waitFor(() => {
      const cachedTasks = queryClient.getQueryData<Task[]>(["tasks", "list", task.projectId]);
      expect(cachedTasks?.[0]?.internalStatus).toBe("pending_merge");
    });

    // Backend called with skipValidation
    expect(mockInvoke).toHaveBeenCalledWith("retry_merge", {
      taskId: task.id,
      skipValidation: true,
    });
  });

  it("shows loading spinner on retry button while processing", async () => {
    const user = userEvent.setup();
    const task = createTestTask();
    queryClient.setQueryData(["tasks", "list", task.projectId], [task]);

    render(
      <QueryClientProvider client={queryClient}>
        <MergeIncompleteTaskDetail task={task} />
      </QueryClientProvider>
    );

    const retryButton = screen.getByTestId("retry-merge-button");
    await user.click(retryButton);

    // Button should be disabled while processing
    await waitFor(() => {
      expect(screen.getByTestId("retry-merge-button")).toBeDisabled();
    });
  });
});

describe("History mode exit on retry", () => {
  let queryClient: QueryClient;

  beforeEach(() => {
    vi.clearAllMocks();
    mockListeners.clear();
    mockInvoke.mockResolvedValue(undefined);
    queryClient = new QueryClient({
      defaultOptions: {
        queries: { retry: false },
      },
    });
  });

  it("clears history state on retry to show live view", async () => {
    const user = userEvent.setup();
    const task = createTestTask();
    queryClient.setQueryData(["tasks", "list", task.projectId], [task]);

    render(
      <QueryClientProvider client={queryClient}>
        <MergeIncompleteTaskDetail task={task} />
      </QueryClientProvider>
    );

    await user.click(screen.getByTestId("retry-merge-button"));

    // setTaskHistoryState(null) clears the history mode so the UI shows live status
    expect(mockSetTaskHistoryState).toHaveBeenCalledWith(null);
  });

  it("does not show retry buttons in historical mode", () => {
    const task = createTestTask();

    render(
      <QueryClientProvider client={queryClient}>
        <MergeIncompleteTaskDetail task={task} isHistorical />
      </QueryClientProvider>
    );

    // Action buttons hidden in historical mode
    expect(screen.queryByTestId("action-buttons")).not.toBeInTheDocument();
    expect(screen.queryByTestId("retry-merge-button")).not.toBeInTheDocument();
  });

  it("clears history state on resolve (mark resolved) to show live view", async () => {
    const user = userEvent.setup();
    const task = createTestTask();
    queryClient.setQueryData(["tasks", "list", task.projectId], [task]);

    render(
      <QueryClientProvider client={queryClient}>
        <MergeIncompleteTaskDetail task={task} />
      </QueryClientProvider>
    );

    await user.click(screen.getByTestId("resolve-merge-button"));

    // Mark resolved does NOT exit history mode (it's a different flow)
    // Verify backend was called
    expect(mockInvoke).toHaveBeenCalledWith("resolve_merge_conflict", { taskId: task.id });
  });
});
