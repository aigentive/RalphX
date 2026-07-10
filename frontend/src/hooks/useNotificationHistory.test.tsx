import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, renderHook, waitFor } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import type { ReactNode } from "react";

import { notificationsApi } from "@/api/notifications";
import { flattenNotificationPages, notificationKeys, useNotificationHistory } from "./useNotificationHistory";

vi.mock("@/api/notifications", () => ({
  notificationsApi: { list: vi.fn(), markRead: vi.fn(), markAllRead: vi.fn(), getUnreadCount: vi.fn() },
}));

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
});
