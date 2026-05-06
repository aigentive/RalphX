import React from "react";
import { describe, it, expect, beforeEach, vi } from "vitest";
import { render, screen, act, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { TaskDetailOverlay } from "./TaskDetailOverlay";
import { useTaskStore } from "@/stores/taskStore";
import { useUiStore } from "@/stores/uiStore";
import { useIdeationStore } from "@/stores/ideationStore";
import type { Task } from "@/types/task";

vi.mock("./TaskDetailPanel", () => ({
  TaskDetailPanel: (props: Record<string, unknown>) => (
    <div
      data-testid="mock-task-detail-panel"
      data-view-as-status={String(props.viewAsStatus ?? "")}
      data-view-timestamp={String(props.viewTimestamp ?? "")}
      data-use-view-registry={String(props.useViewRegistry ?? "")}
    />
  ),
}));

vi.mock("./TaskEditForm", () => ({
  TaskEditForm: ({
    onSave,
    onCancel,
  }: {
    onSave: (input: Record<string, unknown>) => void;
    onCancel: () => void;
  }) => (
    <div data-testid="mock-task-edit-form">
      <button data-testid="edit-form-save" onClick={() => onSave({ title: "Updated" })}>
        Save
      </button>
      <button data-testid="edit-form-cancel" onClick={onCancel}>
        Cancel
      </button>
    </div>
  ),
}));

vi.mock("./StatusDropdown", () => ({
  StatusDropdown: ({ onTransition }: { onTransition: (s: string) => void }) => (
    <button
      data-testid="mock-status-dropdown"
      onClick={() => onTransition("ready")}
    >
      Status
    </button>
  ),
}));

vi.mock("./StateTimelineNav", () => ({
  StateTimelineNav: () => <div data-testid="mock-state-timeline-nav" />,
}));

vi.mock("@/components/tasks/AuditTrailDialog", () => ({
  AuditTrailDialog: ({
    isOpen,
    onClose,
  }: {
    isOpen: boolean;
    onClose: () => void;
  }) =>
    isOpen ? (
      <div data-testid="mock-audit-trail-dialog">
        <button data-testid="audit-trail-close" onClick={onClose}>
          close
        </button>
      </div>
    ) : null,
}));

vi.mock("sonner", () => ({
  toast: {
    error: vi.fn(),
    success: vi.fn(),
  },
}));

vi.mock("@/hooks/useTasks", () => ({
  taskKeys: {
    all: ["tasks"],
    detail: (taskId: string) => ["tasks", "detail", taskId],
  },
  useTasks: vi.fn(() => ({ data: [] })),
}));

const updateMutate = vi.fn();
const moveMutate = vi.fn();
const archiveMutate = vi.fn();
const restoreMutate = vi.fn();

vi.mock("@/hooks/useTaskMutation", () => ({
  useTaskMutation: vi.fn(() => ({
    updateMutation: { mutate: updateMutate, isPending: false },
    moveMutation: { mutate: moveMutate, isPending: false },
    archiveMutation: { mutate: archiveMutate },
    restoreMutation: { mutate: restoreMutate },
    isArchiving: false,
    isRestoring: false,
  })),
}));

const createIdeationMutateAsync = vi.fn();

vi.mock("@/hooks/useIdeation", () => ({
  useCreateIdeationSession: vi.fn(() => ({
    mutateAsync: createIdeationMutateAsync,
    isPending: false,
  })),
}));

const confirmMock = vi.fn(async () => true);

vi.mock("@/hooks/useConfirmation", () => ({
  useConfirmation: vi.fn(() => ({
    confirm: confirmMock,
    confirmationDialogProps: {},
    ConfirmationDialog: () => null,
  })),
}));

function createTestTask(overrides?: Partial<Task>): Task {
  return {
    id: "task-123",
    projectId: "project-456",
    category: "feature",
    title: "Test Task",
    description: "Test description",
    priority: 2,
    internalStatus: "ready",
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

function renderOverlay(
  task: Task,
  props?: Partial<React.ComponentProps<typeof TaskDetailOverlay>>
) {
  const queryClient = new QueryClient({
    defaultOptions: {
      queries: { retry: false },
    },
  });
  useTaskStore.getState().setTasks([task]);
  useUiStore.getState().setSelectedTaskId(task.id);
  return render(
    <QueryClientProvider client={queryClient}>
      <TaskDetailOverlay projectId={task.projectId} {...props} />
    </QueryClientProvider>
  );
}

describe("TaskDetailOverlay", () => {
  beforeEach(() => {
    useTaskStore.getState().setTasks([]);
    useUiStore.getState().setSelectedTaskId(null);
    useUiStore.getState().setTaskHistoryState(null);
    updateMutate.mockReset();
    moveMutate.mockReset();
    archiveMutate.mockReset();
    restoreMutate.mockReset();
    createIdeationMutateAsync.mockReset();
    confirmMock.mockReset();
    confirmMock.mockResolvedValue(true);
  });

  it("hides edit controls for managed plan merge tasks waiting on PR", () => {
    renderOverlay(
      createTestTask({
        category: "plan_merge",
        internalStatus: "waiting_on_pr",
        taskBranch: null,
      })
    );

    expect(screen.queryByTestId("task-overlay-edit-button")).not.toBeInTheDocument();
    expect(screen.queryByTestId("mock-status-dropdown")).not.toBeInTheDocument();
    expect(screen.getByTestId("task-overlay-category")).toHaveTextContent("Plan merge");
    expect(screen.queryByText("plan_merge")).not.toBeInTheDocument();
  });

  it("keeps edit controls for regular user-created tasks", () => {
    renderOverlay(createTestTask({ category: "feature", internalStatus: "ready" }));

    expect(screen.getByTestId("task-overlay-edit-button")).toBeInTheDocument();
    expect(screen.getByTestId("mock-status-dropdown")).toBeInTheDocument();
  });

  it("renders priority metadata below the title in DOM order", () => {
    renderOverlay(createTestTask({ category: "feature", internalStatus: "ready" }));

    const title = screen.getByTestId("task-overlay-title");
    const priority = screen.getByTestId("task-overlay-priority");

    expect(
      title.compareDocumentPosition(priority) & Node.DOCUMENT_POSITION_FOLLOWING
    ).toBeTruthy();
  });

  it("centers task detail content when constrained by the host layout", () => {
    renderOverlay(createTestTask({ category: "feature", internalStatus: "ready" }), {
      constrainContent: true,
    });

    expect(screen.getByTestId("task-detail-content-frame")).toHaveClass(
      "mx-auto",
      "max-w-[1500px]"
    );
  });

  it("adds accessible names and app tooltips to header icon buttons", async () => {
    const user = userEvent.setup();
    renderOverlay(createTestTask({ category: "feature", internalStatus: "backlog" }));

    const archiveButton = screen.getByRole("button", { name: "Archive task" });
    expect(screen.getByRole("button", { name: "Start Ideation" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Edit task" })).toBeInTheDocument();
    expect(archiveButton).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Audit Trail" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Close" })).toBeInTheDocument();

    await user.hover(archiveButton);
    expect((await screen.findAllByText("Archive task")).length).toBeGreaterThan(0);
  });

  it("renders nothing when no task is selected", () => {
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    const { container } = render(
      <QueryClientProvider client={queryClient}>
        <TaskDetailOverlay projectId="project-456" />
      </QueryClientProvider>
    );
    expect(container.firstChild).toBeNull();
  });

  it("renders nothing when selectedTaskId set but task missing", () => {
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    useUiStore.getState().setSelectedTaskId("missing-id");
    const warn = vi.spyOn(console, "warn").mockImplementation(() => {});
    const { container } = render(
      <QueryClientProvider client={queryClient}>
        <TaskDetailOverlay projectId="project-456" />
      </QueryClientProvider>
    );
    expect(container.firstChild).toBeNull();
    expect(warn).toHaveBeenCalled();
    warn.mockRestore();
  });

  it("hides edit/status controls for system-controlled statuses", () => {
    renderOverlay(createTestTask({ internalStatus: "executing" }));
    expect(screen.queryByTestId("task-overlay-edit-button")).not.toBeInTheDocument();
    expect(screen.queryByTestId("mock-status-dropdown")).not.toBeInTheDocument();
  });

  it("shows archived badge and restore button for archived task", () => {
    renderOverlay(
      createTestTask({
        internalStatus: "approved",
        archivedAt: "2026-02-01T12:00:00+00:00",
      })
    );
    expect(screen.getByTestId("archived-badge")).toBeInTheDocument();
    expect(screen.getByTestId("task-overlay-restore-button")).toBeInTheDocument();
    expect(screen.queryByTestId("task-overlay-archive-button")).not.toBeInTheDocument();
    expect(screen.queryByTestId("task-overlay-edit-button")).not.toBeInTheDocument();
  });

  it("toggles edit mode and renders edit form, cancels back to view", async () => {
    const user = userEvent.setup();
    renderOverlay(createTestTask({ internalStatus: "ready" }));
    await user.click(screen.getByTestId("task-overlay-edit-button"));
    expect(screen.getByTestId("mock-task-edit-form")).toBeInTheDocument();
    await user.click(screen.getByTestId("edit-form-cancel"));
    expect(screen.queryByTestId("mock-task-edit-form")).not.toBeInTheDocument();
  });

  it("calls updateMutation on edit form save", async () => {
    const user = userEvent.setup();
    renderOverlay(createTestTask({ internalStatus: "ready" }));
    await user.click(screen.getByTestId("task-overlay-edit-button"));
    await user.click(screen.getByTestId("edit-form-save"));
    expect(updateMutate).toHaveBeenCalledWith(
      expect.objectContaining({ taskId: "task-123", input: { title: "Updated" } }),
      expect.any(Object)
    );
  });

  it("invokes moveMutation when status changes via StatusDropdown", async () => {
    const user = userEvent.setup();
    renderOverlay(createTestTask({ internalStatus: "ready" }));
    await user.click(screen.getByTestId("mock-status-dropdown"));
    expect(moveMutate).toHaveBeenCalledWith({ taskId: "task-123", toStatus: "ready" });
  });

  it("archives task after confirmation", async () => {
    const user = userEvent.setup();
    confirmMock.mockResolvedValueOnce(true);
    renderOverlay(createTestTask({ internalStatus: "ready" }));
    await user.click(screen.getByTestId("task-overlay-archive-button"));
    await waitFor(() => expect(archiveMutate).toHaveBeenCalledWith("task-123", expect.any(Object)));
  });

  it("does not archive when user cancels confirmation", async () => {
    const user = userEvent.setup();
    confirmMock.mockResolvedValueOnce(false);
    renderOverlay(createTestTask({ internalStatus: "ready" }));
    await user.click(screen.getByTestId("task-overlay-archive-button"));
    await waitFor(() => expect(confirmMock).toHaveBeenCalled());
    expect(archiveMutate).not.toHaveBeenCalled();
  });

  it("restores archived task after confirmation", async () => {
    const user = userEvent.setup();
    confirmMock.mockResolvedValueOnce(true);
    renderOverlay(
      createTestTask({
        internalStatus: "approved",
        archivedAt: "2026-02-01T12:00:00+00:00",
      })
    );
    await user.click(screen.getByTestId("task-overlay-restore-button"));
    await waitFor(() => expect(restoreMutate).toHaveBeenCalledWith("task-123", expect.any(Object)));
  });

  it("does not restore when user cancels confirmation", async () => {
    const user = userEvent.setup();
    confirmMock.mockResolvedValueOnce(false);
    renderOverlay(
      createTestTask({
        internalStatus: "approved",
        archivedAt: "2026-02-01T12:00:00+00:00",
      })
    );
    await user.click(screen.getByTestId("task-overlay-restore-button"));
    await waitFor(() => expect(confirmMock).toHaveBeenCalled());
    expect(restoreMutate).not.toHaveBeenCalled();
  });

  it("starts ideation: creates session, switches view, closes overlay", async () => {
    const user = userEvent.setup();
    createIdeationMutateAsync.mockResolvedValueOnce({
      id: "session-1",
      title: "Ideation: Test Task",
      projectId: "project-456",
    });

    renderOverlay(createTestTask({ internalStatus: "backlog" }));
    await user.click(screen.getByTestId("task-overlay-ideation-button"));

    await waitFor(() => expect(createIdeationMutateAsync).toHaveBeenCalled());
    await waitFor(() => expect(useUiStore.getState().currentView).toBe("ideation"));
    expect(useIdeationStore.getState().sessions["session-1"]).toBeDefined();
    expect(useIdeationStore.getState().activeSessionId).toBe("session-1");
  });

  it("toasts error when ideation session creation fails", async () => {
    const user = userEvent.setup();
    const { toast } = await import("sonner");
    createIdeationMutateAsync.mockRejectedValueOnce(new Error("boom"));
    const errSpy = vi.spyOn(console, "error").mockImplementation(() => {});

    renderOverlay(createTestTask({ internalStatus: "backlog" }));
    await user.click(screen.getByTestId("task-overlay-ideation-button"));
    await waitFor(() => expect(toast.error).toHaveBeenCalledWith("Failed to start ideation session"));
    errSpy.mockRestore();
  });

  it("opens audit trail dialog from header button and closes it", async () => {
    const user = userEvent.setup();
    renderOverlay(createTestTask({ internalStatus: "ready" }));
    await user.click(screen.getByTestId("task-overlay-audit-trail-button"));
    expect(screen.getByTestId("mock-audit-trail-dialog")).toBeInTheDocument();
    await user.click(screen.getByTestId("audit-trail-close"));
    expect(screen.queryByTestId("mock-audit-trail-dialog")).not.toBeInTheDocument();
  });

  it("close button clears selectedTaskId by default", async () => {
    const user = userEvent.setup();
    renderOverlay(createTestTask({ internalStatus: "ready" }));
    await user.click(screen.getByTestId("task-overlay-close"));
    expect(useUiStore.getState().selectedTaskId).toBeNull();
  });

  it("close button calls onCloseOverride when provided", async () => {
    const user = userEvent.setup();
    const onClose = vi.fn();
    renderOverlay(createTestTask({ internalStatus: "ready" }), {
      onCloseOverride: onClose,
      selectedTaskIdOverride: "task-123",
    });
    await user.click(screen.getByTestId("task-overlay-close"));
    expect(onClose).toHaveBeenCalled();
    // global selection should remain unchanged when override provided
    expect(useUiStore.getState().selectedTaskId).toBe("task-123");
  });

  it("Escape exits edit mode first, then closes overlay on second press", async () => {
    const user = userEvent.setup();
    renderOverlay(createTestTask({ internalStatus: "ready" }));

    await user.click(screen.getByTestId("task-overlay-edit-button"));
    expect(screen.getByTestId("mock-task-edit-form")).toBeInTheDocument();

    await user.keyboard("{Escape}");
    expect(screen.queryByTestId("mock-task-edit-form")).not.toBeInTheDocument();

    await user.keyboard("{Escape}");
    expect(useUiStore.getState().selectedTaskId).toBeNull();
  });

  it("backdrop click closes overlay; clicking content does not", async () => {
    const user = userEvent.setup();
    renderOverlay(createTestTask({ internalStatus: "ready" }));

    // clicking inside content does not close
    await user.click(screen.getByTestId("task-overlay-title"));
    expect(useUiStore.getState().selectedTaskId).toBe("task-123");

    // clicking backdrop closes
    await user.click(screen.getByTestId("task-overlay-backdrop"));
    expect(useUiStore.getState().selectedTaskId).toBeNull();
  });

  it("renders history mode banner and forwards viewAsStatus to TaskDetailPanel", async () => {
    renderOverlay(createTestTask({ internalStatus: "ready" }));
    // Set history state AFTER mount so the selectedTaskId effect doesn't clear it
    act(() => {
      useUiStore.getState().setTaskHistoryState({
        status: "executing",
        timestamp: "2026-02-15T10:00:00+00:00",
        conversationId: "conv-1",
        agentRunId: "run-1",
      });
    });

    expect(await screen.findByTestId("history-mode-banner")).toHaveTextContent(
      /Viewing: Executing/
    );
    const panel = screen.getByTestId("mock-task-detail-panel");
    expect(panel.getAttribute("data-view-as-status")).toBe("executing");
    expect(panel.getAttribute("data-view-timestamp")).toBe("2026-02-15T10:00:00+00:00");
    expect(panel.getAttribute("data-use-view-registry")).toBe("true");
  });

  it("renders footer when provided", () => {
    renderOverlay(createTestTask({ internalStatus: "ready" }), {
      footer: <div data-testid="overlay-footer">Footer</div>,
    });
    expect(screen.getByTestId("overlay-footer")).toBeInTheDocument();
  });

  it("falls back to default content frame width when constrainContent=false", () => {
    renderOverlay(createTestTask({ internalStatus: "ready" }));
    const frame = screen.getByTestId("task-detail-content-frame");
    expect(frame).toHaveClass("w-full");
    expect(frame).not.toHaveClass("mx-auto");
  });

  it("uses unknown priority fallback color", () => {
    renderOverlay(createTestTask({ priority: 99, internalStatus: "ready" }));
    expect(screen.getByTestId("task-overlay-priority")).toHaveTextContent("P99");
  });

  it("resets edit mode and clears history state when selectedTaskId changes", async () => {
    const user = userEvent.setup();
    const taskA = createTestTask({ id: "task-a", internalStatus: "ready" });
    const taskB = createTestTask({ id: "task-b", title: "Task B", internalStatus: "ready" });
    useTaskStore.getState().setTasks([taskA, taskB]);

    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    useUiStore.getState().setSelectedTaskId("task-a");
    useUiStore.getState().setTaskHistoryState({
      status: "executing",
      timestamp: "2026-02-15T10:00:00+00:00",
    });

    const { rerender } = render(
      <QueryClientProvider client={queryClient}>
        <TaskDetailOverlay projectId="project-456" />
      </QueryClientProvider>
    );

    // enter edit mode
    await user.click(screen.getByTestId("task-overlay-edit-button"));
    expect(screen.getByTestId("mock-task-edit-form")).toBeInTheDocument();

    // switch task → effect should reset edit state and clear history
    act(() => {
      useUiStore.getState().setSelectedTaskId("task-b");
    });
    rerender(
      <QueryClientProvider client={queryClient}>
        <TaskDetailOverlay projectId="project-456" />
      </QueryClientProvider>
    );

    expect(screen.queryByTestId("mock-task-edit-form")).not.toBeInTheDocument();
    expect(useUiStore.getState().taskHistoryState).toBeNull();
  });
});
