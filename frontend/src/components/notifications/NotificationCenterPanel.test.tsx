import { act, fireEvent, render, screen } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { NotificationCenterPanel } from "./NotificationCenterPanel";
import { TooltipProvider } from "@/components/ui/tooltip";
import { useAttentionItems } from "@/hooks/useAttentionItems";
import { useNotificationReadActions } from "@/hooks/useNotificationHistory";
import { useTasksAwaitingReview } from "@/hooks/useReviews";
import { api } from "@/lib/tauri";
import { useTaskStore } from "@/stores/taskStore";
import { useProjectStore } from "@/stores/projectStore";
import { useUiStore } from "@/stores/uiStore";
import type { AttentionItem } from "@/types/notifications";
import type { Task } from "@/types/task";
import type { Project } from "@/types/project";

vi.mock("@/hooks/useAttentionItems", () => ({ useAttentionItems: vi.fn() }));
vi.mock("@/hooks/useNotificationHistory", () => ({ useNotificationReadActions: vi.fn() }));
vi.mock("@/hooks/useReviews", () => ({ useTasksAwaitingReview: vi.fn() }));
vi.mock("@/lib/tauri", () => ({ api: { tasks: { get: vi.fn() } } }));
vi.mock("@/components/reviews/ReviewDetailModal", async () => {
  const React = await import("react");
  return {
    ReviewDetailModal: ({ taskId, onClose }: { taskId: string; onClose: () => void }) => {
      React.useEffect(() => {
        const closeOnEscape = (event: KeyboardEvent) => { if (event.key === "Escape") onClose(); };
        window.addEventListener("keydown", closeOnEscape);
        return () => window.removeEventListener("keydown", closeOnEscape);
      }, [onClose]);
      return <div data-testid="review-detail-modal">{taskId}</div>;
    },
  };
});

const markAllRead = vi.fn();

const item: AttentionItem = {
  id: "task:task-1:failed", category: "task_failed", title: "Task failed",
  detail: "The agent stopped.", projectId: "project-1", createdAt: "2026-07-10T10:00:00Z",
  target: { kind: "task", taskId: "task-1" },
};

function awaitingReviewTasks(allTasks: Task[] = []): ReturnType<typeof useTasksAwaitingReview> {
  return {
    allTasks,
    aiTasks: [],
    humanTasks: [],
    isLoading: false,
    error: null,
    isEmpty: allTasks.length === 0,
    aiCount: 0,
    humanCount: 0,
    totalCount: allTasks.length,
    refetch: vi.fn(),
  };
}

function renderPanel(isOpen: boolean, onClose = vi.fn()) {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(<QueryClientProvider client={queryClient}><TooltipProvider><NotificationCenterPanel isOpen={isOpen} onClose={onClose} /></TooltipProvider></QueryClientProvider>);
}

async function revealDeferredContent() {
  await act(async () => {
    vi.advanceTimersByTime(1);
    await Promise.resolve();
    await Promise.resolve();
  });
}

describe("NotificationCenterPanel first-paint behavior", () => {
  beforeEach(() => {
    markAllRead.mockReset();
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-07-10T10:00:00Z"));
    vi.spyOn(window, "requestAnimationFrame").mockImplementation((callback: FrameRequestCallback) => setTimeout(() => callback(performance.now()), 0) as unknown as number);
    vi.spyOn(window, "cancelAnimationFrame").mockImplementation(() => undefined);
    vi.mocked(useAttentionItems).mockReturnValue({ data: [item], isLoading: false, isError: false, refetch: vi.fn() } as ReturnType<typeof useAttentionItems>);
    vi.mocked(useNotificationReadActions).mockReturnValue({ markRead: vi.fn(), markReadBatch: vi.fn(), markAllRead });
    vi.mocked(useTasksAwaitingReview).mockReturnValue(awaitingReviewTasks());
    vi.mocked(api.tasks.get).mockRejectedValue(new Error("Task not found"));
    useTaskStore.setState({ tasks: {} });
    useProjectStore.setState({ activeProjectId: "project-1" });
  });

  afterEach(() => { useProjectStore.getState().setProjects([]); useProjectStore.setState({ activeProjectId: null }); useTaskStore.setState({ tasks: {} }); useUiStore.getState().closeModal(); vi.useRealTimers(); vi.unstubAllGlobals(); vi.restoreAllMocks(); });

  it("renders the 400px shell and tab chrome synchronously on first open", () => {
    renderPanel(true);
    expect(screen.getByRole("complementary", { name: "Notifications" })).toBeVisible();
    expect(screen.getByRole("tab", { name: /needs action/i })).toBeVisible();
    expect(screen.getByTestId("notification-skeletons")).toBeVisible();
    expect(useTasksAwaitingReview).toHaveBeenCalledWith("project-1", { enabled: false });
  });

  it("defers attention rows until after a frame and macrotask", () => {
    renderPanel(true);
    expect(screen.queryByTestId(`attention-item-${item.id}`)).not.toBeInTheDocument();
    act(() => { vi.advanceTimersByTime(1); });
    expect(screen.getByTestId(`attention-item-${item.id}`)).toBeInTheDocument();
    expect(useTasksAwaitingReview).toHaveBeenLastCalledWith("project-1", { enabled: true });
  });

  it("keeps project attention rows visible because mute only gates alert delivery", () => {
    renderPanel(true);
    act(() => { vi.advanceTimersByTime(1); });
    expect(screen.getByTestId(`attention-item-${item.id}`)).toBeInTheDocument();
  });

  it("uses the global attention query and labels an item from another project by name", () => {
    const otherProject: Project = {
      id: "project-2", name: "Other project", workingDirectory: "/tmp/other", gitMode: "worktree",
      baseBranch: "main", worktreeParentDirectory: null, useFeatureBranches: true,
      mergeValidationMode: "block", detectedAnalysis: null, customAnalysis: null, analyzedAt: null,
      githubPrEnabled: false, createdAt: "2026-07-10T10:00:00Z", updatedAt: "2026-07-10T10:00:00Z",
    };
    useProjectStore.getState().setProjects([otherProject]);
    useProjectStore.getState().selectProject("project-1");
    vi.mocked(useAttentionItems).mockReturnValue({
      data: [{ ...item, projectId: "project-2" }], isLoading: false,
    } as ReturnType<typeof useAttentionItems>);

    renderPanel(true);
    act(() => { vi.advanceTimersByTime(1); });

    expect(useAttentionItems).toHaveBeenCalledWith(undefined, expect.objectContaining({ enabled: true }));
    expect(screen.getByTestId(`attention-item-${item.id}`)).toHaveTextContent("Other project");
  });

  it("shows the unread History cue and the header overflow actions", () => {
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    render(
      <QueryClientProvider client={queryClient}>
        <TooltipProvider>
          <NotificationCenterPanel isOpen onClose={vi.fn()} hasUnreadHistory />
        </TooltipProvider>
      </QueryClientProvider>,
    );

    expect(screen.getByLabelText("Unread notification history")).toBeInTheDocument();
    fireEvent.pointerDown(screen.getByRole("button", { name: "Notification actions" }), {
      button: 0,
      ctrlKey: false,
    });
    expect(screen.getByRole("menuitem", { name: "Mark all read" })).toBeInTheDocument();
    expect(screen.getByRole("menuitem", { name: "Notification settings" })).toBeInTheDocument();
  });

  it("marks all history read from the overflow menu", () => {
    renderPanel(true);
    fireEvent.pointerDown(screen.getByRole("button", { name: "Notification actions" }), { button: 0, ctrlKey: false });

    fireEvent.click(screen.getByRole("menuitem", { name: "Mark all read" }));

    expect(markAllRead).toHaveBeenCalledOnce();
  });

  it("closes the drawer and opens the notification settings section from the overflow menu", () => {
    const onClose = vi.fn();
    renderPanel(true, onClose);
    fireEvent.pointerDown(screen.getByRole("button", { name: "Notification actions" }), { button: 0, ctrlKey: false });

    fireEvent.click(screen.getByRole("menuitem", { name: "Notification settings" }));

    expect(onClose).toHaveBeenCalledOnce();
    expect(useUiStore.getState().activeModal).toBe("settings");
    expect(useUiStore.getState().modalContext).toEqual({ section: "notifications" });
  });

  it("shows an empty action state once a loaded global attention query has no supported groups", () => {
    vi.mocked(useAttentionItems).mockReturnValue({ data: [], isLoading: false, isError: false, refetch: vi.fn() } as ReturnType<typeof useAttentionItems>);
    renderPanel(true);
    act(() => { vi.advanceTimersByTime(1); });

    expect(screen.getByTestId("attention-empty-state")).toHaveTextContent("Nothing needs your attention.");
  });

  it("closes the panel from Escape and its explicit close button", () => {
    const onClose = vi.fn();
    renderPanel(true, onClose);

    fireEvent.keyDown(window, { key: "Escape" });
    fireEvent.click(screen.getByTestId("notifications-panel-close"));

    expect(onClose).toHaveBeenCalledTimes(2);
  });

  it("renders a retryable attention load error instead of all clear", () => {
    const refetch = vi.fn();
    vi.mocked(useAttentionItems).mockReturnValue({ data: [], isLoading: false, isError: true, refetch } as ReturnType<typeof useAttentionItems>);
    renderPanel(true);
    act(() => { vi.advanceTimersByTime(1); });

    expect(screen.getByTestId("attention-load-error")).toHaveTextContent("Couldn't load notifications");
    expect(screen.queryByTestId("attention-empty-state")).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Retry" }));
    expect(refetch).toHaveBeenCalledOnce();
  });

  it("keeps stale attention rows visible when a refresh fails", () => {
    vi.mocked(useAttentionItems).mockReturnValue({ data: [item], isLoading: false, isError: true, refetch: vi.fn() } as ReturnType<typeof useAttentionItems>);
    renderPanel(true);
    act(() => { vi.advanceTimersByTime(1); });

    expect(screen.getByTestId(`attention-item-${item.id}`)).toBeVisible();
    expect(screen.getByTestId("attention-stale-indicator")).toBeVisible();
  });

  it("refreshes relative labels on the shared drawer clock", () => {
    renderPanel(true);
    act(() => { vi.advanceTimersByTime(1); });
    expect(screen.getByTestId(`attention-item-${item.id}`)).toHaveTextContent("now");

    act(() => { vi.advanceTimersByTime(60_000); });
    expect(screen.getByTestId(`attention-item-${item.id}`)).toHaveTextContent("1m");
  });

  it("lets the review modal own Escape while the drawer stays open", () => {
    const onClose = vi.fn();
    const reviewItem: AttentionItem = { ...item, id: "task:task-review:review", category: "review_needed", target: { kind: "task", taskId: "task-review" } };
    const reviewTask: Task = {
      id: "task-review", projectId: "project-1", category: "feature", title: "Review this", description: null,
      priority: 1, internalStatus: "review_passed", needsReviewPoint: false, createdAt: "2026-07-10T10:00:00Z", updatedAt: "2026-07-10T10:00:00Z",
      startedAt: null, completedAt: null, archivedAt: null, blockedReason: null, taskBranch: null, worktreePath: null, mergeCommitSha: null, metadata: null,
    };
    useTaskStore.setState({ tasks: { [reviewTask.id]: reviewTask } });
    vi.mocked(useAttentionItems).mockReturnValue({ data: [reviewItem], isLoading: false, isError: false, refetch: vi.fn() } as ReturnType<typeof useAttentionItems>);
    renderPanel(true, onClose);
    act(() => { vi.advanceTimersByTime(1); });
    fireEvent.click(screen.getByTestId("review-button-task-review"));

    fireEvent.keyDown(window, { key: "Escape" });
    expect(screen.queryByTestId("review-detail-modal")).not.toBeInTheDocument();
    expect(screen.getByRole("complementary", { name: "Notifications" })).toBeVisible();
    expect(onClose).not.toHaveBeenCalled();
  });

  it("omits unresolved project identifiers", () => {
    vi.mocked(useAttentionItems).mockReturnValue({ data: [{ ...item, projectId: "project-unknown-uuid" }], isLoading: false, isError: false, refetch: vi.fn() } as ReturnType<typeof useAttentionItems>);
    renderPanel(true);
    act(() => { vi.advanceTimersByTime(1); });

    expect(screen.getByTestId(`attention-item-${item.id}`)).not.toHaveTextContent("project-unknown-uuid");
  });

  it("moves focus into the drawer and returns it to the topbar trigger on close", () => {
    const trigger = document.createElement("button");
    trigger.id = "notifications-toggle";
    document.body.append(trigger);
    const view = renderPanel(true);
    expect(screen.getByTestId("notifications-panel-close")).toHaveFocus();

    view.rerender(<QueryClientProvider client={new QueryClient()}><TooltipProvider><NotificationCenterPanel isOpen={false} onClose={vi.fn()} /></TooltipProvider></QueryClientProvider>);
    expect(trigger).toHaveFocus();
    trigger.remove();
  });

  it("keeps content through visual close then unmounts it after paint", () => {
    const view = renderPanel(true);
    act(() => { vi.advanceTimersByTime(1); });
    expect(screen.getByTestId(`attention-item-${item.id}`)).toBeInTheDocument();
    view.rerender(<QueryClientProvider client={new QueryClient()}><TooltipProvider><NotificationCenterPanel isOpen={false} onClose={vi.fn()} /></TooltipProvider></QueryClientProvider>);
    expect(screen.getByTestId(`attention-item-${item.id}`)).toBeInTheDocument();
    act(() => { vi.advanceTimersByTime(1); });
    expect(screen.queryByTestId(`attention-item-${item.id}`)).not.toBeInTheDocument();
  });

  it("keeps review rows on the existing card and in-place detail-modal flow", () => {
    const reviewItem: AttentionItem = { ...item, id: "task:task-review:review", category: "review_needed", target: { kind: "task", taskId: "task-review" } };
    const reviewTask: Task = {
      id: "task-review", projectId: "project-1", category: "feature", title: "Review this", description: null,
      priority: 1, internalStatus: "review_passed", needsReviewPoint: false, createdAt: "2026-07-10T10:00:00Z", updatedAt: "2026-07-10T10:00:00Z",
      startedAt: null, completedAt: null, archivedAt: null, blockedReason: null, taskBranch: null, worktreePath: null, mergeCommitSha: null, metadata: null,
    };
    useTaskStore.setState({ tasks: { [reviewTask.id]: reviewTask } });
    vi.mocked(useAttentionItems).mockReturnValue({ data: [reviewItem], isLoading: false } as ReturnType<typeof useAttentionItems>);
    renderPanel(true);
    act(() => { vi.advanceTimersByTime(1); });
    fireEvent.click(screen.getByTestId("review-button-task-review"));
    expect(screen.getByTestId("review-detail-modal")).toHaveTextContent("task-review");
  });

  it("renders a review card from awaiting-review query when the task store is empty", () => {
    const reviewItem: AttentionItem = { ...item, id: "task:task-review:review", category: "review_needed", target: { kind: "task", taskId: "task-review" } };
    const reviewTask: Task = {
      id: "task-review", projectId: "project-1", category: "feature", title: "Review this", description: null,
      priority: 1, internalStatus: "review_passed", needsReviewPoint: false, createdAt: "2026-07-10T10:00:00Z", updatedAt: "2026-07-10T10:00:00Z",
      startedAt: null, completedAt: null, archivedAt: null, blockedReason: null, taskBranch: null, worktreePath: null, mergeCommitSha: null, metadata: null,
    };
    vi.mocked(useAttentionItems).mockReturnValue({ data: [reviewItem], isLoading: false, isError: false, refetch: vi.fn() } as ReturnType<typeof useAttentionItems>);
    vi.mocked(useTasksAwaitingReview).mockReturnValue(awaitingReviewTasks([reviewTask]));

    renderPanel(true);
    act(() => { vi.advanceTimersByTime(1); });

    expect(screen.getByTestId("task-review-card-task-review")).toBeVisible();
    expect(screen.queryByTestId(`attention-item-${reviewItem.id}`)).not.toBeInTheDocument();
  });

  it("resolves a cross-project review card by task id when neither local fallback has the task", async () => {
    const reviewItem: AttentionItem = {
      ...item,
      id: "task:task-other-project:review",
      category: "review_needed",
      projectId: "project-2",
      target: { kind: "task", taskId: "task-other-project" },
    };
    const reviewTask: Task = {
      id: "task-other-project", projectId: "project-2", category: "feature", title: "Review another project", description: null,
      priority: 1, internalStatus: "review_passed", needsReviewPoint: false, createdAt: "2026-07-10T10:00:00Z", updatedAt: "2026-07-10T10:00:00Z",
      startedAt: null, completedAt: null, archivedAt: null, blockedReason: null, taskBranch: null, worktreePath: null, mergeCommitSha: null, metadata: null,
    };
    let resolveTask: ((task: Task) => void) | undefined;
    vi.mocked(useAttentionItems).mockReturnValue({ data: [reviewItem], isLoading: false, isError: false, refetch: vi.fn() } as ReturnType<typeof useAttentionItems>);
    vi.mocked(api.tasks.get).mockImplementation(() => new Promise<Task>((resolve) => { resolveTask = resolve; }));

    renderPanel(true);
    await revealDeferredContent();

    expect(api.tasks.get).toHaveBeenCalledWith("task-other-project");
    await act(async () => {
      resolveTask?.(reviewTask);
      await Promise.resolve();
      vi.advanceTimersByTime(1);
      await Promise.resolve();
    });
    expect(screen.getByTestId("task-review-card-task-other-project")).toBeVisible();
    expect(screen.queryByTestId(`attention-item-${reviewItem.id}`)).not.toBeInTheDocument();
  });

  it("keeps a generic row when the review task id fetch fails and no fallback can resolve it", async () => {
    const reviewItem: AttentionItem = { ...item, id: "task:missing-review:review", category: "review_needed", target: { kind: "task", taskId: "missing-review" } };
    vi.mocked(useAttentionItems).mockReturnValue({ data: [reviewItem], isLoading: false, isError: false, refetch: vi.fn() } as ReturnType<typeof useAttentionItems>);

    renderPanel(true);
    await revealDeferredContent();

    expect(api.tasks.get).toHaveBeenCalledWith("missing-review");
    expect(screen.getByTestId(`attention-item-${reviewItem.id}`)).toBeVisible();
    expect(screen.queryByTestId("task-review-card-missing-review")).not.toBeInTheDocument();
  });

  it("falls back to the generic attention row when no review task is available", () => {
    const reviewItem: AttentionItem = { ...item, id: "task:task-review:review", category: "review_needed", target: { kind: "task", taskId: "task-review" } };
    vi.mocked(useAttentionItems).mockReturnValue({ data: [reviewItem], isLoading: false, isError: false, refetch: vi.fn() } as ReturnType<typeof useAttentionItems>);

    renderPanel(true);
    act(() => { vi.advanceTimersByTime(1); });

    expect(screen.getByTestId(`attention-item-${reviewItem.id}`)).toBeVisible();
    expect(screen.queryByTestId("task-review-card-task-review")).not.toBeInTheDocument();
  });

  it("falls back to the task store when the awaiting-review query is empty", () => {
    const reviewItem: AttentionItem = { ...item, id: "task:task-review:review", category: "review_needed", target: { kind: "task", taskId: "task-review" } };
    const reviewTask: Task = {
      id: "task-review", projectId: "project-1", category: "feature", title: "Review this", description: null,
      priority: 1, internalStatus: "review_passed", needsReviewPoint: false, createdAt: "2026-07-10T10:00:00Z", updatedAt: "2026-07-10T10:00:00Z",
      startedAt: null, completedAt: null, archivedAt: null, blockedReason: null, taskBranch: null, worktreePath: null, mergeCommitSha: null, metadata: null,
    };
    useTaskStore.setState({ tasks: { [reviewTask.id]: reviewTask } });
    vi.mocked(useAttentionItems).mockReturnValue({ data: [reviewItem], isLoading: false, isError: false, refetch: vi.fn() } as ReturnType<typeof useAttentionItems>);

    renderPanel(true);
    act(() => { vi.advanceTimersByTime(1); });

    expect(screen.getByTestId("task-review-card-task-review")).toBeVisible();
    expect(screen.queryByTestId(`attention-item-${reviewItem.id}`)).not.toBeInTheDocument();
  });

  it("re-raises the existing permission dialog with the backend request id", () => {
    const permissionItem: AttentionItem = { ...item, id: "permission:request-1", category: "permission_request", target: { kind: "none" } };
    const onClose = vi.fn();
    const reopen = vi.fn();
    window.addEventListener("ralphx:open-permission-dialog", reopen);
    vi.mocked(useAttentionItems).mockReturnValue({ data: [permissionItem], isLoading: false } as ReturnType<typeof useAttentionItems>);
    renderPanel(true, onClose);
    act(() => { vi.advanceTimersByTime(1); });
    fireEvent.click(screen.getByTestId(`attention-item-${permissionItem.id}`));
    expect(reopen).toHaveBeenCalledWith(expect.objectContaining({ detail: { requestId: "request-1" } }));
    expect(onClose).toHaveBeenCalledOnce();
    window.removeEventListener("ralphx:open-permission-dialog", reopen);
  });

  it("disables expired permission actions without reopening the dialog", () => {
    const permissionItem: AttentionItem = { ...item, id: "permission:request-1", category: "permission_request", createdAt: "2026-07-10T09:55:30Z", target: { kind: "none" } };
    const onClose = vi.fn();
    const reopen = vi.fn();
    window.addEventListener("ralphx:open-permission-dialog", reopen);
    vi.mocked(useAttentionItems).mockReturnValue({ data: [permissionItem], isLoading: false, isError: false, refetch: vi.fn() } as ReturnType<typeof useAttentionItems>);
    renderPanel(true, onClose);
    act(() => { vi.advanceTimersByTime(1); vi.advanceTimersByTime(30_000); });

    expect(screen.getByRole("button", { name: "Expired" })).toBeDisabled();
    fireEvent.click(screen.getByTestId(`attention-item-${permissionItem.id}`));
    fireEvent.click(screen.getByRole("button", { name: "Expired" }));
    expect(reopen).not.toHaveBeenCalled();
    expect(onClose).not.toHaveBeenCalled();
    window.removeEventListener("ralphx:open-permission-dialog", reopen);
  });

  it("opens a non-expired permission action once and supports Enter while ignoring unrelated keys", () => {
    const permissionItem: AttentionItem = {
      ...item,
      id: "permission:request-keyboard",
      category: "permission_request",
      target: { kind: "none" },
    };
    const onClose = vi.fn();
    const reopen = vi.fn();
    window.addEventListener("ralphx:open-permission-dialog", reopen);
    vi.mocked(useAttentionItems).mockReturnValue({
      data: [permissionItem], isLoading: false, isError: false, refetch: vi.fn(),
    } as ReturnType<typeof useAttentionItems>);
    renderPanel(true, onClose);
    act(() => { vi.advanceTimersByTime(1); });

    const row = screen.getByTestId(`attention-item-${permissionItem.id}`);
    fireEvent.click(screen.getByRole("button", { name: "Respond" }));
    expect(reopen).toHaveBeenCalledTimes(1);
    expect(onClose).toHaveBeenCalledOnce();

    fireEvent.keyDown(row, { key: "ArrowDown" });
    expect(reopen).toHaveBeenCalledTimes(1);
    fireEvent.keyDown(row, { key: "Enter" });

    expect(reopen).toHaveBeenLastCalledWith(expect.objectContaining({
      detail: { requestId: "request-keyboard" },
    }));
    expect(onClose).toHaveBeenCalledTimes(2);
    window.removeEventListener("ralphx:open-permission-dialog", reopen);
  });
});
