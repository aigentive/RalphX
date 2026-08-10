import { createElement, useEffect } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";

import { ATTENTION_CATEGORY_MAPPING } from "@/components/notifications/categoryMapping";
import { NotificationActionToast } from "@/components/notifications/NotificationActionToast";
import { performNotificationPrimaryAction } from "@/components/notifications/notificationNavigation";
import { notificationsApi } from "@/api/notifications";
import { notificationKeys } from "@/hooks/useNotificationHistory";
import { useEventBus } from "@/providers/EventProvider";
import {
  useAgentSessionStore,
  type VisibleAgentScope,
} from "@/stores/agentSessionStore";
import { useUiStore } from "@/stores/uiStore";
import {
  NotificationSchema,
  type Notification,
  type NotificationTarget,
} from "@/types/notifications";

import { useNotificationPreferences } from "./useNotificationPreferences";

const activeNotificationToastIds = new Set<string>();
const activeTargetedToasts = new Map<string, Notification>();
const pendingNonAgentAcknowledgements = new Set<string>();
const pendingTargetedAcknowledgements = new Map<string, Notification>();

export function resetNotificationToastStateForTests() {
  activeNotificationToastIds.clear();
  activeTargetedToasts.clear();
  pendingNonAgentAcknowledgements.clear();
  pendingTargetedAcknowledgements.clear();
}

function isFocusedWindow() {
  return document.visibilityState !== "hidden" && document.hasFocus();
}

export function isNotificationTargetVisible(
  target: NotificationTarget,
  scope: VisibleAgentScope | null,
): boolean {
  if (!scope) return false;
  if (target.kind === "agent_conversation") {
    return target.conversationId === scope.visibleConversationId;
  }
  if (target.kind !== "automation_run" || !target.runId) {
    return false;
  }
  return (
    target.runId === scope.automationRunId &&
    target.conversationId === scope.automationConversationId &&
    (target.setupConversationId === undefined ||
      target.setupConversationId === scope.workspaceConversationId)
  );
}

function isTargetedNotification(notification: Notification): boolean {
  return (
    (notification.target.kind === "agent_conversation" &&
      notification.target.conversationId !== undefined) ||
    (notification.target.kind === "automation_run" &&
      notification.target.runId !== undefined &&
      notification.target.conversationId !== undefined)
  );
}

function waitsForVisibleTargetAfterAction(notification: Notification): boolean {
  return (
    notification.category !== "permission_request" &&
    isTargetedNotification(notification)
  );
}

function isNotificationTargetSatisfied(notification: Notification): boolean {
  return isNotificationTargetVisible(
    notification.target,
    useAgentSessionStore.getState().visibleAgentScope,
  );
}

function markNotificationRead(
  notification: Notification,
  queryClient: ReturnType<typeof useQueryClient>,
) {
  void notificationsApi
    .markRead(notification.id)
    .catch((error: unknown) => {
      console.error("Failed to mark notification read:", error);
    })
    .finally(() => {
      void queryClient.invalidateQueries({ queryKey: notificationKeys.all });
    });
}

function dismissNotificationToast(notification: Notification) {
  activeNotificationToastIds.delete(notification.id);
  activeTargetedToasts.delete(notification.id);
  pendingNonAgentAcknowledgements.delete(notification.id);
  pendingTargetedAcknowledgements.delete(notification.id);
  toast.dismiss(notification.id);
}

function acknowledgePendingNonAgentNotification(
  notification: Notification,
  queryClient: ReturnType<typeof useQueryClient>,
) {
  if (!pendingNonAgentAcknowledgements.delete(notification.id)) return;
  if (activeNotificationToastIds.delete(notification.id)) {
    toast.dismiss(notification.id);
  }
  markNotificationRead(notification, queryClient);
}

function acknowledgeIfNotificationTargetSatisfied(
  notification: Notification,
  queryClient: ReturnType<typeof useQueryClient>,
) {
  if (!isNotificationTargetSatisfied(notification)) return;
  const wasActive = activeTargetedToasts.delete(notification.id);
  const wasPending = pendingTargetedAcknowledgements.delete(notification.id);
  if (!wasActive && !wasPending) return;
  if (activeNotificationToastIds.delete(notification.id)) {
    toast.dismiss(notification.id);
  }
  markNotificationRead(notification, queryClient);
}

export function useNotificationToasts() {
  const bus = useEventBus();
  const queryClient = useQueryClient();
  const { ready, focusedToastsEnabled, mutedProjectIds } = useNotificationPreferences();
  const notificationsPanelOpen = useUiStore((state) => state.notificationsPanelOpen);
  const visibleAgentScope = useAgentSessionStore(
    (state) => state.visibleAgentScope,
  );

  useEffect(() => {
    if (!notificationsPanelOpen) return;
    const visibleToastIds = [...activeNotificationToastIds];
    activeNotificationToastIds.clear();
    activeTargetedToasts.clear();
    for (const id of visibleToastIds) toast.dismiss(id);
  }, [notificationsPanelOpen]);

  useEffect(() => {
    for (const notification of activeTargetedToasts.values()) {
      acknowledgeIfNotificationTargetSatisfied(notification, queryClient);
    }
    for (const notification of pendingTargetedAcknowledgements.values()) {
      acknowledgeIfNotificationTargetSatisfied(notification, queryClient);
    }
  }, [queryClient, visibleAgentScope]);

  useEffect(
    () =>
      bus.subscribe<unknown>("notification:updated", (payload) => {
        const parsed = NotificationSchema.safeParse(payload);
        if (!parsed.success || parsed.data.readAt === undefined) return;
        if (
          !activeNotificationToastIds.has(parsed.data.id) &&
          !activeTargetedToasts.has(parsed.data.id) &&
          !pendingNonAgentAcknowledgements.has(parsed.data.id) &&
          !pendingTargetedAcknowledgements.has(parsed.data.id)
        ) {
          return;
        }
        dismissNotificationToast(parsed.data);
      }),
    [bus],
  );

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
    const isTargeted = isTargetedNotification(notification);
    if (isTargeted) {
      activeTargetedToasts.set(notification.id, notification);
      if (isNotificationTargetSatisfied(notification)) {
        acknowledgeIfNotificationTargetSatisfied(notification, queryClient);
        return;
      }
    }

    activeNotificationToastIds.add(notification.id);
    const dismiss = () => dismissNotificationToast(notification);
    const performAction = async () => {
      const waitsForVisibleTarget = waitsForVisibleTargetAfterAction(notification);
      if (waitsForVisibleTarget) {
        pendingTargetedAcknowledgements.set(notification.id, notification);
      } else {
        pendingNonAgentAcknowledgements.add(notification.id);
      }
      try {
        const navigated = await performNotificationPrimaryAction(
          notification,
          queryClient,
        );
        if (!navigated) {
          pendingNonAgentAcknowledgements.delete(notification.id);
          pendingTargetedAcknowledgements.delete(notification.id);
          return;
        }
        if (waitsForVisibleTarget) {
          acknowledgeIfNotificationTargetSatisfied(notification, queryClient);
        } else {
          acknowledgePendingNonAgentNotification(notification, queryClient);
        }
      } catch {
        pendingNonAgentAcknowledgements.delete(notification.id);
        pendingTargetedAcknowledgements.delete(notification.id);
        // Keep the durable notification visible and unread when its action fails.
      }
    };
    toast.warning(createElement(NotificationActionToast, {
      actionLabel: presentation.action ?? "Open",
      ...(notification.body !== undefined && { body: notification.body }),
      onAction: performAction,
      onDismiss: dismiss,
      title: notification.title,
    }), {
      id: notification.id,
      duration: Infinity,
      onDismiss: () => {
        activeNotificationToastIds.delete(notification.id);
        activeTargetedToasts.delete(notification.id);
      },
    });
  }), [bus, focusedToastsEnabled, mutedProjectIds, queryClient, ready]);
}
