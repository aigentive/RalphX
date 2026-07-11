import { act, fireEvent, render, screen } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { NotificationCenterPanel } from "./NotificationCenterPanel";
import { TooltipProvider } from "@/components/ui/tooltip";
import { useAttentionItems } from "@/hooks/useAttentionItems";
import { useTaskStore } from "@/stores/taskStore";
import type { AttentionItem } from "@/types/notifications";
import type { Task } from "@/types/task";

vi.mock("@/hooks/useAttentionItems", () => ({ useAttentionItems: vi.fn() }));
vi.mock("@/components/reviews/ReviewDetailModal", () => ({ ReviewDetailModal: ({ taskId }: { taskId: string }) => <div data-testid="review-detail-modal">{taskId}</div> }));

const item: AttentionItem = {
  id: "task:task-1:failed", category: "task_failed", title: "Task failed",
  detail: "The agent stopped.", projectId: "project-1", createdAt: "2026-07-10T10:00:00Z",
  target: { kind: "task", taskId: "task-1" },
};

function renderPanel(isOpen: boolean, onClose = vi.fn()) {
  const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
  return render(<QueryClientProvider client={queryClient}><TooltipProvider><NotificationCenterPanel projectId="project-1" isOpen={isOpen} onClose={onClose} /></TooltipProvider></QueryClientProvider>);
}

describe("NotificationCenterPanel first-paint behavior", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    vi.spyOn(window, "requestAnimationFrame").mockImplementation((callback: FrameRequestCallback) => setTimeout(() => callback(performance.now()), 0) as unknown as number);
    vi.spyOn(window, "cancelAnimationFrame").mockImplementation(() => undefined);
    vi.mocked(useAttentionItems).mockReturnValue({ data: [item], isLoading: false } as ReturnType<typeof useAttentionItems>);
  });

  afterEach(() => { vi.useRealTimers(); vi.unstubAllGlobals(); vi.restoreAllMocks(); });

  it("renders the 400px shell and tab chrome synchronously on first open", () => {
    renderPanel(true);
    expect(screen.getByRole("complementary", { name: "Notifications" })).toBeVisible();
    expect(screen.getByRole("tab", { name: /needs action/i })).toBeVisible();
    expect(screen.getByTestId("notification-skeletons")).toBeVisible();
  });

  it("defers attention rows until after a frame and macrotask", () => {
    renderPanel(true);
    expect(screen.queryByTestId(`attention-item-${item.id}`)).not.toBeInTheDocument();
    act(() => { vi.advanceTimersByTime(1); });
    expect(screen.getByTestId(`attention-item-${item.id}`)).toBeInTheDocument();
  });

  it("keeps project attention rows visible because mute only gates alert delivery", () => {
    renderPanel(true);
    act(() => { vi.runAllTimers(); });
    expect(screen.getByTestId(`attention-item-${item.id}`)).toBeInTheDocument();
  });

  it("keeps content through visual close then unmounts it after paint", () => {
    const view = renderPanel(true);
    act(() => { vi.runAllTimers(); });
    expect(screen.getByTestId(`attention-item-${item.id}`)).toBeInTheDocument();
    view.rerender(<QueryClientProvider client={new QueryClient()}><TooltipProvider><NotificationCenterPanel projectId="project-1" isOpen={false} onClose={vi.fn()} /></TooltipProvider></QueryClientProvider>);
    expect(screen.getByTestId(`attention-item-${item.id}`)).toBeInTheDocument();
    act(() => { vi.runAllTimers(); });
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
    act(() => { vi.runAllTimers(); });
    fireEvent.click(screen.getByTestId("review-button-task-review"));
    expect(screen.getByTestId("review-detail-modal")).toHaveTextContent("task-review");
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
});
