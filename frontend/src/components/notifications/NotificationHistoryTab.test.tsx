import { act, fireEvent, render, screen } from "@testing-library/react";
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

function intersectLatestObserver() {
  const callback = observerCallbacks.at(-1);
  if (!callback) throw new Error("observer was not created");
  callback([{ isIntersecting: true, target: screen.getByTestId("notification-history-row-notification-1") } as IntersectionObserverEntry], {} as IntersectionObserver);
}

describe("NotificationHistoryTab", () => {
  const markRead = vi.fn();
  const markReadBatch = vi.fn();
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
    vi.mocked(useNotificationReadActions).mockReturnValue({ markRead, markReadBatch, markAllRead });
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

  it("does not mark rows read just because the drawer/history tab opened", () => {
    renderHistory();
    act(() => { vi.runAllTimers(); });
    expect(markReadBatch).not.toHaveBeenCalled();
  });

  it("marks each visible unread row once after one second through the deduped batch window", () => {
    renderHistory();
    act(() => { vi.runAllTimers(); });
    intersectLatestObserver();
    act(() => { vi.advanceTimersByTime(1_099); });
    expect(markReadBatch).not.toHaveBeenCalled();

    act(() => { vi.advanceTimersByTime(1); });
    expect(markReadBatch).toHaveBeenCalledTimes(1);
    expect(markReadBatch).toHaveBeenCalledWith(["notification-1"]);

    intersectLatestObserver();
    act(() => { vi.advanceTimersByTime(2_000); });
    expect(markReadBatch).toHaveBeenCalledTimes(1);
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
