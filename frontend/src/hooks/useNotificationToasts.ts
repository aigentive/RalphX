import { useEffect } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";

import { ATTENTION_CATEGORY_MAPPING } from "@/components/notifications/categoryMapping";
import { navigateNotification } from "@/components/notifications/notificationNavigation";
import { notificationsApi } from "@/api/notifications";
import { notificationKeys } from "@/hooks/useNotificationHistory";
import { useEventBus } from "@/providers/EventProvider";
import { useUiStore } from "@/stores/uiStore";
import { NotificationSchema, type Notification } from "@/types/notifications";

import { useNotificationPreferences } from "./useNotificationPreferences";

const DEFAULT_TOAST_DURATION_MS = 8_000;
const PERMISSION_WINDOW_MS = 5 * 60_000;

function isFocusedWindow() {
  return document.visibilityState !== "hidden" && document.hasFocus();
}

function toastDuration(notification: Notification): number {
  if (notification.category !== "permission_request") return DEFAULT_TOAST_DURATION_MS;
  const expiresAt = new Date(notification.createdAt).getTime() + PERMISSION_WINDOW_MS;
  return Math.max(1_000, Math.min(PERMISSION_WINDOW_MS, expiresAt - Date.now()));
}

export function useNotificationToasts() {
  const bus = useEventBus();
  const queryClient = useQueryClient();
  const { focusedToastsEnabled } = useNotificationPreferences();

  useEffect(() => bus.subscribe<unknown>("notification:created", (payload) => {
    const parsed = NotificationSchema.safeParse(payload);
    if (!parsed.success) return;
    const notification = parsed.data;
    if (
      notification.severity !== "action_required" ||
      !focusedToastsEnabled ||
      useUiStore.getState().notificationsPanelOpen ||
      !isFocusedWindow()
    ) return;

    const presentation = ATTENTION_CATEGORY_MAPPING[notification.category];
    toast.warning(notification.title, {
      ...(notification.body !== undefined && { description: notification.body }),
      duration: toastDuration(notification),
      action: {
        label: presentation.action ?? "Open",
        onClick: () => {
          navigateNotification(notification, queryClient);
          void notificationsApi.markRead(notification.id).finally(() => {
            void queryClient.invalidateQueries({ queryKey: notificationKeys.unreadCount(notification.projectId) });
          });
        },
      },
    });
  }), [bus, focusedToastsEnabled, queryClient]);
}
