import { useCallback } from "react";
import { useInfiniteQuery, useQuery, useQueryClient, type InfiniteData } from "@tanstack/react-query";
import { toast } from "sonner";

import { notificationsApi } from "@/api/notifications";
import type { NotificationPage } from "@/types/notifications";

const HISTORY_PAGE_LIMIT = 50;

export const notificationKeys = {
  all: ["notifications"] as const,
  history: (projectId?: string) => [...notificationKeys.all, "history", projectId ?? "all"] as const,
  unreadCount: (projectId?: string) => [...notificationKeys.all, "unread-count", projectId ?? "all"] as const,
};

export function useNotificationHistory(projectId?: string, options: { enabled?: boolean } = {}) {
  return useInfiniteQuery<NotificationPage, Error>({
    queryKey: notificationKeys.history(projectId),
    queryFn: ({ pageParam }) => notificationsApi.list({
      ...(projectId !== undefined && { projectId }),
      ...(typeof pageParam === "string" && { cursor: pageParam }),
      limit: HISTORY_PAGE_LIMIT,
    }),
    getNextPageParam: (lastPage) => lastPage.hasMore ? lastPage.cursor : undefined,
    initialPageParam: undefined as string | undefined,
    staleTime: 30_000,
    enabled: options.enabled ?? true,
  });
}

export function flattenNotificationPages(data: InfiniteData<NotificationPage> | undefined) {
  return data?.pages.flatMap((page) => page.notifications) ?? [];
}

export function useUnreadNotificationCount(projectId?: string) {
  return useQuery({
    queryKey: notificationKeys.unreadCount(projectId),
    queryFn: () => notificationsApi.getUnreadCount(projectId),
    staleTime: 30_000,
    placeholderData: (previousData) => previousData,
  });
}

function markRowsRead(
  queryClient: ReturnType<typeof useQueryClient>,
  projectId: string | undefined,
  ids: readonly string[],
) {
  const uniqueIds = [...new Set(ids)];
  if (uniqueIds.length === 0) return;

  const readAt = new Date().toISOString();
  queryClient.setQueryData<InfiniteData<NotificationPage>>(
    notificationKeys.history(projectId),
    (current) => current && {
      ...current,
      pages: current.pages.map((page) => ({
        ...page,
        notifications: page.notifications.map((notification) =>
          uniqueIds.includes(notification.id) && notification.readAt === undefined
            ? { ...notification, readAt }
            : notification,
        ),
      })),
    },
  );
  void Promise.all(uniqueIds.map((id) => notificationsApi.markRead(id))).finally(() => {
    void queryClient.invalidateQueries({ queryKey: notificationKeys.unreadCount(projectId) });
  });
}

export function useNotificationReadActions(projectId?: string) {
  const queryClient = useQueryClient();

  const markRead = useCallback((id: string) => {
    markRowsRead(queryClient, projectId, [id]);
  }, [projectId, queryClient]);

  const markAllRead = useCallback(async () => {
    const queryKey = notificationKeys.history(projectId);
    const unreadCountQueryKey = notificationKeys.unreadCount(projectId);
    const previous = queryClient.getQueryData<InfiniteData<NotificationPage>>(queryKey);
    const previousUnreadCount = queryClient.getQueryData<number>(unreadCountQueryKey);
    void queryClient.cancelQueries({ queryKey: unreadCountQueryKey });
    queryClient.setQueryData<InfiniteData<NotificationPage>>(
      queryKey,
      (current) => current && {
        ...current,
        pages: current.pages.map((page) => ({
          ...page,
          notifications: page.notifications.map((notification) =>
            notification.readAt === undefined ? { ...notification, readAt: new Date().toISOString() } : notification,
          ),
        })),
      },
    );
    queryClient.setQueryData(unreadCountQueryKey, 0);
    try {
      await notificationsApi.markAllRead(projectId);
      await queryClient.invalidateQueries({ queryKey: unreadCountQueryKey });
    } catch (error) {
      queryClient.setQueryData(queryKey, previous);
      queryClient.setQueryData(unreadCountQueryKey, previousUnreadCount);
      await Promise.all([
        queryClient.invalidateQueries({ queryKey }),
        queryClient.invalidateQueries({ queryKey: unreadCountQueryKey }),
      ]);
      toast.error(error instanceof Error ? error.message : "Failed to mark notifications as read");
    }
  }, [projectId, queryClient]);

  return { markRead, markAllRead };
}
