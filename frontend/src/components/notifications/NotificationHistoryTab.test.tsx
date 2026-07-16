import { act, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { TooltipProvider } from "@/components/ui/tooltip";
import { useNotificationHistory, useNotificationReadActions } from "@/hooks/useNotificationHistory";

import { NotificationHistoryTab } from "./NotificationHistoryTab";

vi.mock("@/hooks/useNotificationHistory", () => ({
  flattenNotificationPages: (data: { pages: Array<{ notifications: unknown[] }> } | undefined) =>
    data?.pages.flatMap((page) => page.notifications) ?? [],
  useNotificationHistory: vi.fn(),
  useNotificationReadActions: vi.fn(),
}));

const observerCallbacks: IntersectionObserverCallback[] = [];
class IntersectionObserverMock implements IntersectionObserver {
  readonly root = null;
  readonly rootMargin = "";
  readonly thresholds = [] as ReadonlyArray<number>;
  observe = vi.fn();
  unobserve = vi.fn();
  disconnect = vi.fn();
  takeRecords = vi.fn(() => []);
  constructor(callback: IntersectionObserverCallback) { observerCallbacks.push(callback); }
}

const notification = {
  id: "notification-1", createdAt: "2026-07-10T10:00:00Z", category: "task_failed" as const,
  severity: "action_required" as const, title: "Task failed", target: { kind: "none" as const },
};

function renderHistory(active = true, now = new Date("2026-07-10T10:00:00Z").getTime()) {
  return render(<TooltipProvider><NotificationHistoryTab active={active} now={now} onOpen={vi.fn()} /></TooltipProvider>);
}

describe("NotificationHistoryTab", () => {
  const markRead = vi.fn();
  const markAllRead = vi.fn();

  beforeEach(() => {
    vi.useFakeTimers();
    observerCallbacks.length = 0;
    window.IntersectionObserver = IntersectionObserverMock as unknown as typeof IntersectionObserver;
    vi.spyOn(window, "requestAnimationFrame").mockImplementation((callback: FrameRequestCallback) => setTimeout(() => callback(performance.now()), 0) as unknown as number);
    vi.spyOn(window, "cancelAnimationFrame").mockImplementation(() => undefined);
    vi.mocked(useNotificationHistory).mockReturnValue({
      data: { pages: [{ notifications: [notification] }] }, isLoading: false, hasNextPage: false,
      isFetchingNextPage: false, fetchNextPage: vi.fn(), refetch: vi.fn(), isError: false,
    } as ReturnType<typeof useNotificationHistory>);
    vi.mocked(useNotificationReadActions).mockReturnValue({ markRead, markAllRead });
  });

  afterEach(() => {
    vi.useRealTimers();
    vi.restoreAllMocks();
    vi.clearAllMocks();
  });

  it("keeps the history shell visible before deferred row hydration", () => {
    renderHistory();
    expect(screen.getByTestId("notification-history-skeletons")).toBeVisible();
    expect(screen.queryByTestId("notification-history-row-notification-1")).not.toBeInTheDocument();

    act(() => { vi.runAllTimers(); });
    expect(screen.getByTestId("notification-history-row-notification-1")).toBeVisible();
  });

  it("never marks rows read from history hydration or viewport visibility", () => {
    renderHistory();
    act(() => { vi.runAllTimers(); });

    expect(observerCallbacks).toHaveLength(0);
    act(() => { vi.advanceTimersByTime(2_000); });

    expect(markRead).not.toHaveBeenCalled();
  });

  it("marks all rows read from the explicit control", () => {
    renderHistory();
    act(() => { vi.runAllTimers(); });
    fireEvent.click(screen.getByRole("button", { name: "Mark all read" }));
    expect(markAllRead).toHaveBeenCalledOnce();
  });

  it("navigates and marks an unread history row read when clicked", () => {
    const onOpen = vi.fn();
    render(<TooltipProvider><NotificationHistoryTab active now={new Date("2026-07-10T10:00:00Z").getTime()} onOpen={onOpen} /></TooltipProvider>);
    act(() => { vi.runAllTimers(); });

    fireEvent.click(screen.getByTestId("notification-history-row-notification-1"));
    expect(onOpen).toHaveBeenCalledWith(notification);
    expect(markRead).toHaveBeenCalledWith("notification-1");
  });

  it("keeps long history rows readable and openable inside a narrow drawer", () => {
    const longTitle = "Task failed while reconciling extremely-long-worktree-name-without-natural-breaks-and-a-long-agent-title";
    const longBody = "The latest worker attempt reported a very long detail line with commit identifiers, branch names, and recovery context that still needs to be readable.";
    const longNotification = {
      ...notification,
      id: "notification-long",
      createdAt: "2026-07-08T08:00:00Z",
      title: longTitle,
      body: longBody,
    };
    const onOpen = vi.fn();
    vi.mocked(useNotificationHistory).mockReturnValue({
      data: { pages: [{ notifications: [longNotification] }] }, isLoading: false, hasNextPage: false,
      isFetchingNextPage: false, fetchNextPage: vi.fn(), refetch: vi.fn(), isError: false,
    } as ReturnType<typeof useNotificationHistory>);

    render(<div style={{ width: 260 }}><TooltipProvider><NotificationHistoryTab active now={new Date("2026-07-10T10:00:00Z").getTime()} onOpen={onOpen} /></TooltipProvider></div>);
    act(() => { vi.runAllTimers(); });

    const row = screen.getByTestId("notification-history-row-notification-long");
    expect(screen.getByRole("button", { name: "Mark all read" })).toBeVisible();
    expect(screen.getByLabelText("Refresh notification history")).toBeVisible();
    expect(row).toHaveClass("grid", "overflow-hidden");
    expect(within(row).getByText(longTitle)).toBeVisible();
    expect(within(row).getByText(longBody)).toBeVisible();
    expect(within(row).getByText("2d")).toBeVisible();

    fireEvent.click(row);
    expect(onOpen).toHaveBeenCalledWith(longNotification);
    expect(markRead).toHaveBeenCalledWith("notification-long");
  });

  it("renders a retryable history load error instead of its empty state", () => {
    const refetch = vi.fn();
    vi.mocked(useNotificationHistory).mockReturnValue({
      data: undefined, isLoading: false, isError: true, hasNextPage: false,
      isFetchingNextPage: false, fetchNextPage: vi.fn(), refetch,
    } as ReturnType<typeof useNotificationHistory>);
    renderHistory();
    act(() => { vi.runAllTimers(); });

    expect(screen.getByTestId("notification-history-load-error")).toHaveTextContent("Couldn't load notifications");
    expect(screen.queryByTestId("notification-history-empty-state")).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Retry" }));
    expect(refetch).toHaveBeenCalledOnce();
    expect(screen.getByTestId("refresh-notification-history")).toBeVisible();
  });

  it("keeps stale history rows visible when a refresh fails", () => {
    vi.mocked(useNotificationHistory).mockReturnValue({
      data: { pages: [{ notifications: [notification] }] }, isLoading: false, isError: true, hasNextPage: false,
      isFetchingNextPage: false, fetchNextPage: vi.fn(), refetch: vi.fn(),
    } as ReturnType<typeof useNotificationHistory>);
    renderHistory();
    act(() => { vi.runAllTimers(); });

    expect(screen.getByTestId("notification-history-row-notification-1")).toBeVisible();
    expect(screen.getByTestId("notification-history-stale-indicator")).toBeVisible();
  });

  it("updates memoized row relative time from the shared drawer clock", () => {
    const view = renderHistory(true, new Date("2026-07-10T10:00:00Z").getTime());
    act(() => { vi.runAllTimers(); });
    expect(screen.getByTestId("notification-history-row-notification-1")).toHaveTextContent("now");

    view.rerender(<TooltipProvider><NotificationHistoryTab active now={new Date("2026-07-10T10:01:00Z").getTime()} onOpen={vi.fn()} /></TooltipProvider>);
    expect(screen.getByTestId("notification-history-row-notification-1")).toHaveTextContent("1m");
  });
});
