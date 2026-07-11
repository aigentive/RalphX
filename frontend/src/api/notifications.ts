import { TauriVoidSchema, typedInvoke } from "@/lib/tauri";
import { z } from "zod";
import {
  AttentionItemListSchema,
  NotificationPageSchema,
  type AttentionItem,
  type NotificationPage,
} from "@/types/notifications";

export const notificationsApi = {
  setDockBadgeCount: (count: number): Promise<null> =>
    typedInvoke("set_dock_badge_count", { count }, TauriVoidSchema),
  listAttentionItems: (projectId?: string): Promise<AttentionItem[]> =>
    typedInvoke(
      "list_attention_items",
      projectId ? { projectId } : {},
      AttentionItemListSchema,
    ),
  list: (options: { projectId?: string; cursor?: string; limit?: number } = {}): Promise<NotificationPage> =>
    typedInvoke(
      "list_notifications",
      {
        ...(options.projectId !== undefined && { projectId: options.projectId }),
        ...(options.cursor !== undefined && { cursor: options.cursor }),
        ...(options.limit !== undefined && { limit: options.limit }),
      },
      NotificationPageSchema,
    ),
  markRead: (id: string): Promise<null> =>
    typedInvoke("mark_notification_read", { id }, z.null()),
  markAllRead: (projectId?: string): Promise<null> =>
    typedInvoke(
      "mark_all_notifications_read",
      projectId !== undefined ? { projectId } : {},
      z.null(),
    ),
  getUnreadCount: (projectId?: string): Promise<number> =>
    typedInvoke(
      "get_unread_notification_count",
      projectId !== undefined ? { projectId } : {},
      z.number().int().nonnegative(),
    ),
} as const;
