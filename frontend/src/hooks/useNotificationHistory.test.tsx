import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, renderHook, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { ReactNode } from "react";

import { notificationsApi } from "@/api/notifications";
import {
  flattenNotificationPages,
  notificationKeys,
  useNotificationHistory,
  useNotificationReadActions,
  useUnreadNotificationCount,
} from "./useNotificationHistory";

const { toastError } = vi.hoisted(() => ({ toastError: vi.fn() }));

vi.mock("@/api/notifications", () => ({
  notificationsApi: { list: vi.fn(), markRead: vi.fn(), markAllRead: vi.fn(), getUnreadCount: vi.fn() },
}));
vi.mock("sonner", () => ({ toast: { error: toastError } }));

describe("useNotificationHistory", () => {
  it("appends the older cursor page when Load older requests the next page", async () => {
    vi.mocked(notificationsApi.list)
      .mockResolvedValueOnce({ notifications: [{ id: "new", createdAt: "2026-07-10T10:00:00Z", category: "info", severity: "info", title: "New", target: { kind: "none" } }], cursor: "older", hasMore: true })
      .mockResolvedValueOnce({ notifications: [{ id: "old", createdAt: "2026-07-09T10:00:00Z", category: "info", severity: "info", title: "Old", target: { kind: "none" } }], hasMore: false });

    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    const wrapper = ({ children }: { children: ReactNode }) => <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>;
    const { result } = renderHook(() => useNotificationHistory("project-1"), { wrapper });
    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    await act(async () => { await result.current.fetchNextPage(); });

    expect(notificationsApi.list).toHaveBeenNthCalledWith(1, { projectId: "project-1", limit: 50 });
    expect(notificationsApi.list).toHaveBeenNthCalledWith(2, { projectId: "project-1", cursor: "older", limit: 50 });
    await waitFor(() => {
      const cached = queryClient.getQueryData(notificationKeys.history("project-1"));
      expect(flattenNotificationPages(cached as Parameters<typeof flattenNotificationPages>[0]).map((item) => item.id)).toEqual(["new", "old"]);
    });
  });

  it("does not fetch a disabled history query", async () => {
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    const wrapper = ({ children }: { children: ReactNode }) => <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>;

    const { result } = renderHook(() => useNotificationHistory(undefined, { enabled: false }), { wrapper });

    await waitFor(() => expect(result.current.fetchStatus).toBe("idle"));
    expect(notificationsApi.list).not.toHaveBeenCalled();
  });

  it("exposes a failed history load without inventing a cursor page", async () => {
    vi.mocked(notificationsApi.list).mockRejectedValueOnce(new Error("history unavailable"));
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    const wrapper = ({ children }: { children: ReactNode }) => <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>;

    const { result } = renderHook(() => useNotificationHistory(), { wrapper });

    await waitFor(() => expect(result.current.isError).toBe(true));
    expect(result.current.data).toBeUndefined();
    expect(flattenNotificationPages(result.current.data)).toEqual([]);
  });
});

describe("notification read queries", () => {
  function createWrapper(queryClient: QueryClient) {
    return ({ children }: { children: ReactNode }) => <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>;
  }

  const unreadNotification = {
    id: "unread", createdAt: "2026-07-10T10:00:00Z", category: "info" as const, severity: "info" as const,
    title: "Unread", target: { kind: "none" as const },
  };

  beforeEach(() => {
    vi.clearAllMocks();
  });

  it("loads the unread count for the requested project", async () => {
    vi.mocked(notificationsApi.getUnreadCount).mockResolvedValueOnce(3);
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    const { result } = renderHook(() => useUnreadNotificationCount("project-1"), { wrapper: createWrapper(queryClient) });

    await waitFor(() => expect(result.current.data).toBe(3));
    expect(notificationsApi.getUnreadCount).toHaveBeenCalledWith("project-1");
  });

  it("marks one unread row through the single-row action and refreshes its count", async () => {
    vi.mocked(notificationsApi.markRead).mockResolvedValue(null);
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    const invalidateQueries = vi.spyOn(queryClient, "invalidateQueries");
    queryClient.setQueryData(notificationKeys.history("project-1"), {
      pages: [{ notifications: [unreadNotification], hasMore: false }],
      pageParams: [undefined],
    });
    const { result } = renderHook(() => useNotificationReadActions("project-1"), { wrapper: createWrapper(queryClient) });

    act(() => result.current.markRead("unread"));

    expect(notificationsApi.markRead).toHaveBeenCalledWith("unread");
    expect(queryClient.getQueryData<{ pages: Array<{ notifications: Array<{ readAt?: string }> }> }>(
      notificationKeys.history("project-1"),
    )?.pages[0]?.notifications[0]?.readAt).toEqual(expect.any(String));
    await waitFor(() => expect(invalidateQueries).toHaveBeenCalledWith({
      queryKey: notificationKeys.unreadCount("project-1"),
    }));
  });

  it("marks all unread cached rows and does not toast after the authoritative all-read action succeeds", async () => {
    vi.mocked(notificationsApi.markAllRead).mockResolvedValue(null);
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    queryClient.setQueryData(notificationKeys.history(), {
      pages: [{ notifications: [unreadNotification, { ...unreadNotification, id: "already-read", readAt: "2026-07-09T10:00:00Z" }], hasMore: false }],
      pageParams: [undefined],
    });
    const { result } = renderHook(() => useNotificationReadActions(), { wrapper: createWrapper(queryClient) });

    await act(async () => { await result.current.markAllRead(); });

    expect(notificationsApi.markAllRead).toHaveBeenCalledWith(undefined);
    const cached = queryClient.getQueryData<{
      pages: Array<{ notifications: Array<{ id: string; readAt?: string }> }>;
    }>(notificationKeys.history());
    expect(cached?.pages[0]?.notifications[0]?.readAt).toEqual(expect.any(String));
    expect(cached?.pages[0]?.notifications[1]?.readAt).toBe("2026-07-09T10:00:00Z");
    expect(toastError).not.toHaveBeenCalled();
  });

  it("clears the unread count immediately while mark-all-read is pending", async () => {
    let resolveMarkAllRead!: (value: null) => void;
    vi.mocked(notificationsApi.markAllRead).mockReturnValue(new Promise((resolve) => {
      resolveMarkAllRead = resolve;
    }));
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    queryClient.setQueryData(notificationKeys.unreadCount(), 6);
    const { result } = renderHook(() => useNotificationReadActions(), { wrapper: createWrapper(queryClient) });

    let pending!: Promise<void>;
    act(() => {
      pending = result.current.markAllRead();
    });

    expect(queryClient.getQueryData(notificationKeys.unreadCount())).toBe(0);
    resolveMarkAllRead(null);
    await act(async () => pending);
  });

  it("reports and restores mark-all-read after failure", async () => {
    vi.mocked(notificationsApi.markAllRead).mockRejectedValueOnce(new Error("mark all failed"));
    const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false } } });
    queryClient.setQueryData(notificationKeys.history(), {
      pages: [{ notifications: [unreadNotification], hasMore: false }],
      pageParams: [undefined],
    });
    queryClient.setQueryData(notificationKeys.unreadCount(), 1);
    const { result } = renderHook(() => useNotificationReadActions(), { wrapper: createWrapper(queryClient) });

    await act(async () => { await result.current.markAllRead(); });

    const cached = queryClient.getQueryData<{
      pages: Array<{ notifications: Array<{ readAt?: string }> }>;
    }>(notificationKeys.history());
    expect(cached?.pages[0]?.notifications[0]?.readAt).toBeUndefined();
    expect(queryClient.getQueryData(notificationKeys.unreadCount())).toBe(1);
    expect(toastError).toHaveBeenCalledWith("mark all failed");
  });
});
