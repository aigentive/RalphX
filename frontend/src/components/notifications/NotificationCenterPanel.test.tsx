import { act, fireEvent, render, screen } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { NotificationCenterPanel, type NotificationCenterPanelProps } from "./NotificationCenterPanel";
import { automationsApi } from "@/api/automations";
import { permissionApi } from "@/api/permission";
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

const reviewQueryResults = vi.hoisted(() => new Map<string, unknown>());

vi.mock("@tanstack/react-query", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@tanstack/react-query")>();
  return {
    ...actual,
    useQueries: ({
      queries,
    }: {
      queries: Array<{
        enabled?: boolean;
        queryFn: () => Promise<unknown>;
        queryKey: readonly unknown[];
      }>;
    }) => queries.map((query) => {
      const taskIdCandidate = query.queryKey[query.queryKey.length - 1];
      const taskId = typeof taskIdCandidate === "string" ? taskIdCandidate : undefined;
      if (query.enabled) {
        void query.queryFn().catch(() => undefined);
      }
      return { data: taskId ? reviewQueryResults.get(taskId) : undefined };
    }),
  };
});
vi.mock("@/hooks/useAttentionItems", () => ({ useAttentionItems: vi.fn() }));
vi.mock("@/api/automations", () => ({ automationsApi: { resume: vi.fn() } }));
vi.mock("@/api/permission", () => ({ permissionApi: { getPendingPermissions: vi.fn(), listPendingPermissionGates: vi.fn() } }));
vi.mock("@/hooks/useNotificationHistory", () => ({ useNotificationReadActions: vi.fn() }));
vi.mock("@/hooks/useReviews", () => ({ useTasksAwaitingReview: vi.fn() }));
vi.mock("@/lib/tauri", () => ({ api: { tasks: { get: vi.fn() } } }));
vi.mock("@/components/ui/scroll-area", async () => {
  const React = await import("react");
  return {
    ScrollArea: ({ children, className }: { children: React.ReactNode; className?: string }) => (
      <div className={className}>{children}</div>
    ),
  };
});
vi.mock("@/components/ui/tooltip", async () => {
  const React = await import("react");
  return {
    TooltipProvider: ({ children }: { children: React.ReactNode }) => <React.Fragment>{children}</React.Fragment>,
    Tooltip: ({ children }: { children: React.ReactNode }) => <React.Fragment>{children}</React.Fragment>,
    TooltipTrigger: ({ children }: { children: React.ReactNode }) => <React.Fragment>{children}</React.Fragment>,
    TooltipContent: ({ children }: { children: React.ReactNode }) => <div>{children}</div>,
  };
});
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
let animationFrameCallbacks = new Map<number, FrameRequestCallback>();
let nextAnimationFrameId = 1;

const item: AttentionItem = {
  id: "task:task-1:failed", category: "task_failed", title: "Task failed",
  detail: "The agent stopped.", projectId: "project-1", createdAt: "2026-07-10T10:00:00Z",
  target: { kind: "task", taskId: "task-1" },
};

const longUnbrokenText = "LongNotificationContent".repeat(10);

function createProject(overrides: Partial<Project> = {}): Project {
  return {
    id: "project-1", name: "Project", workingDirectory: "/tmp/project", gitMode: "worktree",
    baseBranch: "main", worktreeParentDirectory: null, useFeatureBranches: true,
    mergeValidationMode: "block", detectedAnalysis: null, customAnalysis: null, analyzedAt: null,
    githubPrEnabled: false, createdAt: "2026-07-10T10:00:00Z", updatedAt: "2026-07-10T10:00:00Z",
    ...overrides,
  };
}

function createReviewTask(overrides: Partial<Task> = {}): Task {
  return {
    id: "task-review", projectId: "project-1", category: "feature", title: "Review this", description: null,
    priority: 1, internalStatus: "review_passed", needsReviewPoint: false, createdAt: "2026-07-10T10:00:00Z", updatedAt: "2026-07-10T10:00:00Z",
    startedAt: null, completedAt: null, archivedAt: null, blockedReason: null, taskBranch: null, worktreePath: null, mergeCommitSha: null, metadata: null,
    ...overrides,
  };
}

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

async function renderPanel(
  isOpen: boolean,
  onClose = vi.fn(),
  props: Partial<Omit<NotificationCenterPanelProps, "isOpen" | "onClose">> = {},
) {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  let view: ReturnType<typeof render> | undefined;
  await act(async () => {
    view = render(
      <QueryClientProvider client={queryClient}>
        <TooltipProvider>
          <NotificationCenterPanel isOpen={isOpen} onClose={onClose} {...props} />
        </TooltipProvider>
      </QueryClientProvider>,
    );
    await Promise.resolve();
    await Promise.resolve();
  });
  if (!view) {
    throw new Error("Notification panel failed to render");
  }
  return view;
}

async function revealDeferredContent() {
  await act(async () => {
    const callbacks = Array.from(animationFrameCallbacks.entries());
    animationFrameCallbacks.clear();
    for (const [, callback] of callbacks) {
      callback(performance.now());
    }
    vi.advanceTimersByTime(0);
    await Promise.resolve();
    await Promise.resolve();
  });
}

describe("NotificationCenterPanel first-paint behavior", () => {
  beforeEach(() => {
    markAllRead.mockReset();
    reviewQueryResults.clear();
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-07-10T10:00:00Z"));
    animationFrameCallbacks = new Map();
    nextAnimationFrameId = 1;
    vi.spyOn(window, "requestAnimationFrame").mockImplementation((callback: FrameRequestCallback) => {
      const id = nextAnimationFrameId;
      nextAnimationFrameId += 1;
      animationFrameCallbacks.set(id, callback);
      return id;
    });
    vi.spyOn(window, "cancelAnimationFrame").mockImplementation((id: number) => {
      animationFrameCallbacks.delete(id);
    });
    vi.mocked(useAttentionItems).mockReturnValue({ data: [item], isLoading: false, isError: false, refetch: vi.fn() } as ReturnType<typeof useAttentionItems>);
    vi.mocked(useNotificationReadActions).mockReturnValue({ markRead: vi.fn(), markAllRead });
    vi.mocked(useTasksAwaitingReview).mockReturnValue(awaitingReviewTasks());
    vi.mocked(api.tasks.get).mockRejectedValue(new Error("Task not found"));
    vi.mocked(automationsApi.resume).mockReset();
    vi.mocked(permissionApi.getPendingPermissions).mockReset();
    vi.mocked(permissionApi.listPendingPermissionGates).mockReset();
    vi.mocked(permissionApi.getPendingPermissions).mockResolvedValue([]);
    vi.mocked(permissionApi.listPendingPermissionGates).mockResolvedValue([]);
    useTaskStore.setState({ tasks: {} });
    useProjectStore.setState({ activeProjectId: "project-1" });
  });

  afterEach(() => {
    act(() => {
      useProjectStore.getState().setProjects([]);
      useProjectStore.setState({ activeProjectId: null });
      useTaskStore.setState({ tasks: {} });
      useUiStore.getState().closeModal();
    });
    animationFrameCallbacks.clear();
    vi.useRealTimers();
    vi.unstubAllGlobals();
    vi.restoreAllMocks();
  });

  it("renders the 400px shell and tab chrome synchronously on first open", async () => {
    await renderPanel(true);
    expect(screen.getByRole("complementary", { name: "Notifications" })).toBeVisible();
    expect(screen.getByRole("tab", { name: /needs action/i })).toBeVisible();
    expect(screen.getByTestId("notification-skeletons")).toBeVisible();
    expect(useTasksAwaitingReview).toHaveBeenCalledWith("project-1", { enabled: false });
  });

  it("defers attention rows until after a frame and macrotask", async () => {
    await renderPanel(true);
    expect(screen.queryByTestId(`attention-item-${item.id}`)).not.toBeInTheDocument();
    await revealDeferredContent();
    expect(screen.getByTestId(`attention-item-${item.id}`)).toBeInTheDocument();
    expect(useTasksAwaitingReview).toHaveBeenLastCalledWith("project-1", { enabled: true });
  });

  it("keeps project attention rows visible because mute only gates alert delivery", async () => {
    await renderPanel(true);
    await revealDeferredContent();
    expect(screen.getByTestId(`attention-item-${item.id}`)).toBeInTheDocument();
  });

  it("resumes a still-paused automation directly from its notification action", async () => {
    const pausedItem: AttentionItem = {
      id: "automation:automation-1:paused",
      category: "automation_paused",
      title: "Automation paused: Release pipeline",
      detail: "Signal verification failed",
      projectId: "project-1",
      createdAt: "2026-07-10T10:00:00Z",
      target: {
        kind: "automation_run",
        projectId: "project-1",
        automationId: "automation-1",
      },
    };
    vi.mocked(useAttentionItems).mockReturnValue({
      data: [pausedItem],
      isLoading: false,
      isError: false,
      refetch: vi.fn(),
    } as ReturnType<typeof useAttentionItems>);
    vi.mocked(automationsApi.resume).mockResolvedValue({} as never);

    await renderPanel(true);
    await revealDeferredContent();
    fireEvent.click(screen.getByRole("button", { name: "Resume" }));
    await act(async () => Promise.resolve());

    expect(automationsApi.resume).toHaveBeenCalledWith("automation-1");
  });

  it("uses the global attention query and labels an item from another project by name", async () => {
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

    await renderPanel(true);
    await revealDeferredContent();

    expect(useAttentionItems).toHaveBeenCalledWith(undefined, expect.objectContaining({ enabled: true }));
    expect(screen.getByTestId(`attention-item-${item.id}`)).toHaveTextContent("Other project");
  });

  it("shows the unread History cue and the header overflow actions", async () => {
    await renderPanel(true, vi.fn(), { hasUnreadHistory: true });

    expect(screen.getByLabelText("Unread notification history")).toBeInTheDocument();
    fireEvent.pointerDown(screen.getByRole("button", { name: "Notification actions" }), {
      button: 0,
      ctrlKey: false,
    });
    expect(screen.getByRole("menuitem", { name: "Mark all read" })).toBeInTheDocument();
    expect(screen.getByRole("menuitem", { name: "Notification settings" })).toBeInTheDocument();
  });

  it("marks all history read from the overflow menu", async () => {
    await renderPanel(true);
    fireEvent.pointerDown(screen.getByRole("button", { name: "Notification actions" }), { button: 0, ctrlKey: false });

    fireEvent.click(screen.getByRole("menuitem", { name: "Mark all read" }));

    expect(markAllRead).toHaveBeenCalledOnce();
  });

  it("closes the drawer and opens the notification settings section from the overflow menu", async () => {
    const onClose = vi.fn();
    await renderPanel(true, onClose);
    fireEvent.pointerDown(screen.getByRole("button", { name: "Notification actions" }), { button: 0, ctrlKey: false });

    fireEvent.click(screen.getByRole("menuitem", { name: "Notification settings" }));

    expect(onClose).toHaveBeenCalledOnce();
    expect(useUiStore.getState().activeModal).toBe("settings");
    expect(useUiStore.getState().modalContext).toEqual({ section: "notifications" });
  });

  it("shows an empty action state once a loaded global attention query has no supported groups", async () => {
    vi.mocked(useAttentionItems).mockReturnValue({ data: [], isLoading: false, isError: false, refetch: vi.fn() } as ReturnType<typeof useAttentionItems>);
    await renderPanel(true);
    await revealDeferredContent();

    expect(screen.getByTestId("attention-empty-state")).toHaveTextContent("Nothing needs your attention.");
  });

  it("closes the panel from Escape and its explicit close button", async () => {
    const onClose = vi.fn();
    await renderPanel(true, onClose);

    fireEvent.keyDown(window, { key: "Escape" });
    fireEvent.click(screen.getByTestId("notifications-panel-close"));

    expect(onClose).toHaveBeenCalledTimes(2);
  });

  it("renders a retryable attention load error instead of all clear", async () => {
    const refetch = vi.fn();
    vi.mocked(useAttentionItems).mockReturnValue({ data: [], isLoading: false, isError: true, refetch } as ReturnType<typeof useAttentionItems>);
    await renderPanel(true);
    await revealDeferredContent();

    expect(screen.getByTestId("attention-load-error")).toHaveTextContent("Couldn't load notifications");
    expect(screen.queryByTestId("attention-empty-state")).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Retry" }));
    expect(refetch).toHaveBeenCalledOnce();
  });

  it("keeps stale attention rows visible when a refresh fails", async () => {
    vi.mocked(useAttentionItems).mockReturnValue({ data: [item], isLoading: false, isError: true, refetch: vi.fn() } as ReturnType<typeof useAttentionItems>);
    await renderPanel(true);
    await revealDeferredContent();

    expect(screen.getByTestId(`attention-item-${item.id}`)).toBeVisible();
    expect(screen.getByTestId("attention-stale-indicator")).toBeVisible();
  });

  it("refreshes relative labels on the shared drawer clock", async () => {
    await renderPanel(true);
    await revealDeferredContent();
    expect(screen.getByTestId(`attention-item-${item.id}`)).toHaveTextContent("now");

    act(() => { vi.advanceTimersByTime(60_000); });
    expect(screen.getByTestId(`attention-item-${item.id}`)).toHaveTextContent("1m");
  });

  it("lets the review modal own Escape while the drawer stays open", async () => {
    const onClose = vi.fn();
    const reviewItem: AttentionItem = { ...item, id: "task:task-review:review", category: "review_needed", target: { kind: "task", taskId: "task-review" } };
    const reviewTask: Task = {
      id: "task-review", projectId: "project-1", category: "feature", title: "Review this", description: null,
      priority: 1, internalStatus: "review_passed", needsReviewPoint: false, createdAt: "2026-07-10T10:00:00Z", updatedAt: "2026-07-10T10:00:00Z",
      startedAt: null, completedAt: null, archivedAt: null, blockedReason: null, taskBranch: null, worktreePath: null, mergeCommitSha: null, metadata: null,
    };
    useTaskStore.setState({ tasks: { [reviewTask.id]: reviewTask } });
    vi.mocked(useAttentionItems).mockReturnValue({ data: [reviewItem], isLoading: false, isError: false, refetch: vi.fn() } as ReturnType<typeof useAttentionItems>);
    await renderPanel(true, onClose);
    await revealDeferredContent();
    fireEvent.click(screen.getByTestId("review-button-task-review"));

    fireEvent.keyDown(window, { key: "Escape" });
    expect(screen.queryByTestId("review-detail-modal")).not.toBeInTheDocument();
    expect(screen.getByRole("complementary", { name: "Notifications" })).toBeVisible();
    expect(onClose).not.toHaveBeenCalled();
  });

  it("omits unresolved project identifiers", async () => {
    vi.mocked(useAttentionItems).mockReturnValue({ data: [{ ...item, projectId: "project-unknown-uuid" }], isLoading: false, isError: false, refetch: vi.fn() } as ReturnType<typeof useAttentionItems>);
    await renderPanel(true);
    await revealDeferredContent();

    expect(screen.getByTestId(`attention-item-${item.id}`)).not.toHaveTextContent("project-unknown-uuid");
  });

  it("contains long generic attention content without crowding the action", async () => {
    const project = createProject({ id: "project-long", name: `Project ${longUnbrokenText}` });
    const longItem: AttentionItem = {
      ...item,
      id: "task:task-long:failed",
      title: `Task ${longUnbrokenText}`,
      detail: `The agent produced ${longUnbrokenText} while reporting a failure.`,
      projectId: project.id,
      target: { kind: "task", taskId: "task-long" },
    };
    useProjectStore.getState().setProjects([project]);
    useProjectStore.getState().selectProject("project-1");
    vi.mocked(useAttentionItems).mockReturnValue({ data: [longItem], isLoading: false, isError: false, refetch: vi.fn() } as ReturnType<typeof useAttentionItems>);

    await renderPanel(true);
    await revealDeferredContent();

    const row = screen.getByTestId(`attention-item-${longItem.id}`);
    expect(row).toHaveClass("min-w-0", "overflow-hidden");
    expect(row).toHaveTextContent(project.name);
    expect(screen.getByText(longItem.title)).toHaveClass("min-w-0", "truncate");
    expect(screen.getByText(longItem.detail ?? "")).toHaveClass("break-words");
    expect(screen.getByRole("button", { name: "Open task" })).toHaveClass("max-w-full", "shrink-0");
  });

  it("moves focus into the drawer and returns it to the topbar trigger on close", async () => {
    const trigger = document.createElement("button");
    trigger.id = "notifications-toggle";
    document.body.append(trigger);
    const view = await renderPanel(true);
    expect(screen.getByTestId("notifications-panel-close")).toHaveFocus();

    await act(async () => {
      view.rerender(<QueryClientProvider client={new QueryClient()}><TooltipProvider><NotificationCenterPanel isOpen={false} onClose={vi.fn()} /></TooltipProvider></QueryClientProvider>);
      await Promise.resolve();
    });
    expect(trigger).toHaveFocus();
    trigger.remove();
  });

  it("keeps content through visual close then unmounts it after paint", async () => {
    const view = await renderPanel(true);
    await revealDeferredContent();
    expect(screen.getByTestId(`attention-item-${item.id}`)).toBeInTheDocument();
    await act(async () => {
      view.rerender(<QueryClientProvider client={new QueryClient()}><TooltipProvider><NotificationCenterPanel isOpen={false} onClose={vi.fn()} /></TooltipProvider></QueryClientProvider>);
      await Promise.resolve();
    });
    expect(screen.getByTestId(`attention-item-${item.id}`)).toBeInTheDocument();
    await revealDeferredContent();
    expect(screen.queryByTestId(`attention-item-${item.id}`)).not.toBeInTheDocument();
  });

  it("keeps review rows on the existing card and in-place detail-modal flow", async () => {
    const reviewItem: AttentionItem = { ...item, id: "task:task-review:review", category: "review_needed", target: { kind: "task", taskId: "task-review" } };
    const reviewTask: Task = {
      id: "task-review", projectId: "project-1", category: "feature", title: "Review this", description: null,
      priority: 1, internalStatus: "review_passed", needsReviewPoint: false, createdAt: "2026-07-10T10:00:00Z", updatedAt: "2026-07-10T10:00:00Z",
      startedAt: null, completedAt: null, archivedAt: null, blockedReason: null, taskBranch: null, worktreePath: null, mergeCommitSha: null, metadata: null,
    };
    useTaskStore.setState({ tasks: { [reviewTask.id]: reviewTask } });
    vi.mocked(useAttentionItems).mockReturnValue({ data: [reviewItem], isLoading: false } as ReturnType<typeof useAttentionItems>);
    await renderPanel(true);
    await revealDeferredContent();
    fireEvent.click(screen.getByTestId("review-button-task-review"));
    expect(screen.getByTestId("review-detail-modal")).toHaveTextContent("task-review");
  });

  it("renders review notifications with a panel-contained review card", async () => {
    const reviewItem: AttentionItem = { ...item, id: "task:task-review:review", category: "review_needed", target: { kind: "task", taskId: "task-review" } };
    const reviewTask = createReviewTask({
      title: `Review ${longUnbrokenText}`,
      description: `Review description ${longUnbrokenText} must wrap inside the notification drawer.`,
    });
    useTaskStore.setState({ tasks: { [reviewTask.id]: reviewTask } });
    vi.mocked(useAttentionItems).mockReturnValue({ data: [reviewItem], isLoading: false, isError: false, refetch: vi.fn() } as ReturnType<typeof useAttentionItems>);

    await renderPanel(true);
    await revealDeferredContent();

    const card = screen.getByTestId("task-review-card-task-review");
    expect(card).toHaveAttribute("data-presentation", "panel");
    expect(card).toHaveClass("min-w-0", "overflow-hidden", "p-3", "shadow-none");
    expect(screen.getByTestId("task-review-title")).toHaveClass("line-clamp-2", "break-words");
    expect(screen.getByTestId("task-review-description")).toHaveClass("break-words");
    expect(screen.getByRole("button", { name: "Review" })).toHaveClass("w-full", "min-w-0");
  });

  it("renders a review card from awaiting-review query when the task store is empty", async () => {
    const reviewItem: AttentionItem = { ...item, id: "task:task-review:review", category: "review_needed", target: { kind: "task", taskId: "task-review" } };
    const reviewTask: Task = {
      id: "task-review", projectId: "project-1", category: "feature", title: "Review this", description: null,
      priority: 1, internalStatus: "review_passed", needsReviewPoint: false, createdAt: "2026-07-10T10:00:00Z", updatedAt: "2026-07-10T10:00:00Z",
      startedAt: null, completedAt: null, archivedAt: null, blockedReason: null, taskBranch: null, worktreePath: null, mergeCommitSha: null, metadata: null,
    };
    vi.mocked(useAttentionItems).mockReturnValue({ data: [reviewItem], isLoading: false, isError: false, refetch: vi.fn() } as ReturnType<typeof useAttentionItems>);
    vi.mocked(useTasksAwaitingReview).mockReturnValue(awaitingReviewTasks([reviewTask]));

    await renderPanel(true);
    await revealDeferredContent();

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
    vi.mocked(useAttentionItems).mockReturnValue({ data: [reviewItem], isLoading: false, isError: false, refetch: vi.fn() } as ReturnType<typeof useAttentionItems>);
    reviewQueryResults.set(reviewTask.id, reviewTask);
    vi.mocked(api.tasks.get).mockResolvedValue(reviewTask);

    await renderPanel(true);
    await revealDeferredContent();

    expect(api.tasks.get).toHaveBeenCalledWith("task-other-project");
    expect(screen.getByTestId("task-review-card-task-other-project")).toBeVisible();
    expect(screen.queryByTestId(`attention-item-${reviewItem.id}`)).not.toBeInTheDocument();
  });

  it("keeps a generic row when the review task id fetch fails and no fallback can resolve it", async () => {
    const reviewItem: AttentionItem = { ...item, id: "task:missing-review:review", category: "review_needed", target: { kind: "task", taskId: "missing-review" } };
    vi.mocked(useAttentionItems).mockReturnValue({ data: [reviewItem], isLoading: false, isError: false, refetch: vi.fn() } as ReturnType<typeof useAttentionItems>);

    await renderPanel(true);
    await revealDeferredContent();

    expect(api.tasks.get).toHaveBeenCalledWith("missing-review");
    expect(screen.getByTestId(`attention-item-${reviewItem.id}`)).toBeVisible();
    expect(screen.queryByTestId("task-review-card-missing-review")).not.toBeInTheDocument();
  });

  it("falls back to the generic attention row when no review task is available", async () => {
    const reviewItem: AttentionItem = { ...item, id: "task:task-review:review", category: "review_needed", target: { kind: "task", taskId: "task-review" } };
    vi.mocked(useAttentionItems).mockReturnValue({ data: [reviewItem], isLoading: false, isError: false, refetch: vi.fn() } as ReturnType<typeof useAttentionItems>);

    await renderPanel(true);
    await revealDeferredContent();

    expect(screen.getByTestId(`attention-item-${reviewItem.id}`)).toBeVisible();
    expect(screen.queryByTestId("task-review-card-task-review")).not.toBeInTheDocument();
  });

  it("falls back to the task store when the awaiting-review query is empty", async () => {
    const reviewItem: AttentionItem = { ...item, id: "task:task-review:review", category: "review_needed", target: { kind: "task", taskId: "task-review" } };
    const reviewTask: Task = {
      id: "task-review", projectId: "project-1", category: "feature", title: "Review this", description: null,
      priority: 1, internalStatus: "review_passed", needsReviewPoint: false, createdAt: "2026-07-10T10:00:00Z", updatedAt: "2026-07-10T10:00:00Z",
      startedAt: null, completedAt: null, archivedAt: null, blockedReason: null, taskBranch: null, worktreePath: null, mergeCommitSha: null, metadata: null,
    };
    useTaskStore.setState({ tasks: { [reviewTask.id]: reviewTask } });
    vi.mocked(useAttentionItems).mockReturnValue({ data: [reviewItem], isLoading: false, isError: false, refetch: vi.fn() } as ReturnType<typeof useAttentionItems>);

    await renderPanel(true);
    await revealDeferredContent();

    expect(screen.getByTestId("task-review-card-task-review")).toBeVisible();
    expect(screen.queryByTestId(`attention-item-${reviewItem.id}`)).not.toBeInTheDocument();
  });

  it("re-raises the existing permission dialog with the backend request id", async () => {
    const permissionItem: AttentionItem = { ...item, id: "permission:request-1", category: "permission_request", target: { kind: "none" } };
    const onClose = vi.fn();
    const reopen = vi.fn();
    window.addEventListener("ralphx:open-permission-dialog", reopen);
    vi.mocked(permissionApi.listPendingPermissionGates).mockResolvedValue([
      { request_id: "request-1", tool_name: "Bash", tool_input: {} },
    ]);
    vi.mocked(useAttentionItems).mockReturnValue({ data: [permissionItem], isLoading: false } as ReturnType<typeof useAttentionItems>);
    await renderPanel(true, onClose);
    await revealDeferredContent();
    await act(async () => {
      fireEvent.click(screen.getByTestId(`attention-item-${permissionItem.id}`));
      await Promise.resolve();
    });
    expect(reopen).toHaveBeenCalledWith(expect.objectContaining({ detail: { requestId: "request-1" } }));
    expect(onClose).toHaveBeenCalledOnce();
    window.removeEventListener("ralphx:open-permission-dialog", reopen);
  });

  it("disables expired permission actions without reopening the dialog", async () => {
    const permissionItem: AttentionItem = { ...item, id: "permission:request-1", category: "permission_request", createdAt: "2026-07-10T09:55:30Z", target: { kind: "none" } };
    const onClose = vi.fn();
    const reopen = vi.fn();
    window.addEventListener("ralphx:open-permission-dialog", reopen);
    vi.mocked(useAttentionItems).mockReturnValue({ data: [permissionItem], isLoading: false, isError: false, refetch: vi.fn() } as ReturnType<typeof useAttentionItems>);
    await renderPanel(true, onClose);
    await revealDeferredContent();
    act(() => { vi.advanceTimersByTime(30_000); });

    expect(screen.getByRole("button", { name: "Expired" })).toBeDisabled();
    fireEvent.click(screen.getByTestId(`attention-item-${permissionItem.id}`));
    fireEvent.click(screen.getByRole("button", { name: "Expired" }));
    expect(reopen).not.toHaveBeenCalled();
    expect(onClose).not.toHaveBeenCalled();
    window.removeEventListener("ralphx:open-permission-dialog", reopen);
  });

  it("opens a non-expired permission action once and supports Enter while ignoring unrelated keys", async () => {
    const permissionItem: AttentionItem = {
      ...item,
      id: "permission:request-keyboard",
      category: "permission_request",
      target: { kind: "none" },
    };
    const onClose = vi.fn();
    const reopen = vi.fn();
    window.addEventListener("ralphx:open-permission-dialog", reopen);
    vi.mocked(permissionApi.listPendingPermissionGates).mockResolvedValue([
      { request_id: "request-keyboard", tool_name: "Bash", tool_input: {} },
    ]);
    vi.mocked(useAttentionItems).mockReturnValue({
      data: [permissionItem], isLoading: false, isError: false, refetch: vi.fn(),
    } as ReturnType<typeof useAttentionItems>);
    await renderPanel(true, onClose);
    await revealDeferredContent();

    const row = screen.getByTestId(`attention-item-${permissionItem.id}`);
    await act(async () => {
      fireEvent.click(screen.getByRole("button", { name: "Respond" }));
      await Promise.resolve();
    });
    expect(reopen).toHaveBeenCalledTimes(1);
    expect(onClose).toHaveBeenCalledOnce();

    fireEvent.keyDown(row, { key: "ArrowDown" });
    expect(reopen).toHaveBeenCalledTimes(1);
    await act(async () => {
      fireEvent.keyDown(row, { key: "Enter" });
      await Promise.resolve();
    });

    expect(reopen).toHaveBeenLastCalledWith(expect.objectContaining({
      detail: { requestId: "request-keyboard" },
    }));
    expect(onClose).toHaveBeenCalledTimes(2);
    window.removeEventListener("ralphx:open-permission-dialog", reopen);
  });
});
