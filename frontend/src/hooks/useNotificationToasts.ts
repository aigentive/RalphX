import { useEffect } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";

import { ATTENTION_CATEGORY_MAPPING } from "@/components/notifications/categoryMapping";
import { navigateNotification } from "@/components/notifications/notificationNavigation";
import { notificationsApi } from "@/api/notifications";
import { notificationKeys } from "@/hooks/useNotificationHistory";
import { useEventBus } from "@/providers/EventProvider";
import { useAgentSessionStore } from "@/stores/agentSessionStore";
import { useUiStore } from "@/stores/uiStore";
import {
  NotificationSchema,
  type Notification,
} from "@/types/notifications";

import { useNotificationPreferences } from "./useNotificationPreferences";

const DEFAULT_TOAST_DURATION_MS = 8_000;
const PERMISSION_WINDOW_MS = 5 * 60_000;
const activeNotificationToastIds = new Set<string>();
const activeAgentConversationToasts = new Map<string, Notification>();

export function resetNotificationToastStateForTests() {
  activeNotificationToastIds.clear();
  activeAgentConversationToasts.clear();
}

function isFocusedWindow() {
  return document.visibilityState !== "hidden" && document.hasFocus();
}

function toastDuration(notification: Notification): number {
  if (notification.category !== "permission_request") return DEFAULT_TOAST_DURATION_MS;
  const expiresAt = new Date(notification.createdAt).getTime() + PERMISSION_WINDOW_MS;
  return Math.max(1_000, Math.min(PERMISSION_WINDOW_MS, expiresAt - Date.now()));
}

function isAgentConversationNotification(notification: Notification): boolean {
  return (
    notification.target.kind === "agent_conversation" &&
    notification.target.conversationId !== undefined
  );
}

function markNotificationRead(
  notification: Notification,
  queryClient: ReturnType<typeof useQueryClient>,
) {
  void notificationsApi.markRead(notification.id).finally(() => {
    void queryClient.invalidateQueries({ queryKey: notificationKeys.all });
  });
}

function acknowledgeAgentConversationToast(
  notification: Notification,
  queryClient: ReturnType<typeof useQueryClient>,
) {
  if (!activeAgentConversationToasts.delete(notification.id)) return;
  activeNotificationToastIds.delete(notification.id);
  toast.dismiss(notification.id);
  markNotificationRead(notification, queryClient);
}

export function useNotificationToasts() {
  const bus = useEventBus();
  const queryClient = useQueryClient();
  const { ready, focusedToastsEnabled, mutedProjectIds } = useNotificationPreferences();
  const notificationsPanelOpen = useUiStore((state) => state.notificationsPanelOpen);
  const selectedConversationId = useAgentSessionStore(
    (state) => state.selectedConversationId,
  );

  useEffect(() => {
    if (!notificationsPanelOpen) return;
    for (const id of activeNotificationToastIds) toast.dismiss(id);
    activeNotificationToastIds.clear();
    activeAgentConversationToasts.clear();
  }, [notificationsPanelOpen]);

  useEffect(() => {
    if (!selectedConversationId) return;
    for (const notification of activeAgentConversationToasts.values()) {
      if (notification.target.conversationId === selectedConversationId) {
        acknowledgeAgentConversationToast(notification, queryClient);
      }
    }
  }, [queryClient, selectedConversationId]);

  useEffect(() => bus.subscribe<unknown>("notification:created", (payload) => {
    const parsed = NotificationSchema.safeParse(payload);
    if (!parsed.success) return;
    const notification = parsed.data;
    if (
      notification.severity !== "action_required" ||
      !ready ||
      !focusedToastsEnabled ||
      (notification.projectId !== undefined && mutedProjectIds.includes(notification.projectId)) ||
      useUiStore.getState().notificationsPanelOpen ||
      !isFocusedWindow()
    ) return;

    const presentation = ATTENTION_CATEGORY_MAPPING[notification.category];
    const isAgentConversation = isAgentConversationNotification(notification);
    if (
      isAgentConversation &&
      useAgentSessionStore.getState().selectedConversationId ===
        notification.target.conversationId
    ) {
      markNotificationRead(notification, queryClient);
      return;
    }

    activeNotificationToastIds.add(notification.id);
    if (isAgentConversation) {
      activeAgentConversationToasts.set(notification.id, notification);
    }
    toast.warning(notification.title, {
      id: notification.id,
      ...(notification.body !== undefined && { description: notification.body }),
      duration: isAgentConversation ? Infinity : toastDuration(notification),
      ...(isAgentConversation && {
        closeButton: true,
        closeButtonAriaLabel: "Dismiss notification",
      }),
      onDismiss: () => {
        activeNotificationToastIds.delete(notification.id);
        activeAgentConversationToasts.delete(notification.id);
      },
      onAutoClose: () => {
        activeNotificationToastIds.delete(notification.id);
        activeAgentConversationToasts.delete(notification.id);
      },
      action: {
        label: presentation.action ?? "Open",
        onClick: () => {
          void navigateNotification(notification, queryClient).then((navigated) => {
            if (isAgentConversation) {
              if (navigated) {
                acknowledgeAgentConversationToast(notification, queryClient);
              }
              return;
            }
            markNotificationRead(notification, queryClient);
          });
        },
      },
    });
  }), [bus, focusedToastsEnabled, mutedProjectIds, queryClient, ready]);
}
