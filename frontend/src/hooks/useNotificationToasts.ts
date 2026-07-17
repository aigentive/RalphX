import { createElement, useEffect } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";

import { getAgentArtifactStateSnapshot } from "@/components/agents/agentArtifactState";
import { useAgentArtifactUiStore } from "@/components/agents/agentArtifactUiStore";
import { ATTENTION_CATEGORY_MAPPING } from "@/components/notifications/categoryMapping";
import { NotificationActionToast } from "@/components/notifications/NotificationActionToast";
import { performNotificationPrimaryAction } from "@/components/notifications/notificationNavigation";
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

const activeNotificationToastIds = new Set<string>();
const activeAgentConversationToasts = new Map<string, Notification>();
const pendingAgentConversationAcknowledgements = new Map<string, Notification>();

export function resetNotificationToastStateForTests() {
  activeNotificationToastIds.clear();
  activeAgentConversationToasts.clear();
  pendingAgentConversationAcknowledgements.clear();
}

function isFocusedWindow() {
  return document.visibilityState !== "hidden" && document.hasFocus();
}

function isAgentConversationNotification(notification: Notification): boolean {
  return (
    notification.target.kind === "agent_conversation" &&
    notification.target.conversationId !== undefined
  );
}

function isPlanReviewNotification(notification: Notification): boolean {
  return (
    notification.category === "plan_approval" ||
    notification.category === "team_plan_approval"
  );
}

function isAgentConversationNotificationSatisfied(
  notification: Notification,
): boolean {
  if (!isAgentConversationNotification(notification)) return false;
  const conversationId = notification.target.conversationId;
  if (
    useAgentSessionStore.getState().selectedConversationId !== conversationId
  ) {
    return false;
  }
  if (!isPlanReviewNotification(notification)) return true;

  const artifactState = getAgentArtifactStateSnapshot(conversationId, false);
  return (
    artifactState.isOpen &&
    artifactState.activeTab === "plan" &&
    !artifactState.hiddenTabs.includes("plan")
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

function dismissNotificationToast(notification: Notification) {
  activeNotificationToastIds.delete(notification.id);
  activeAgentConversationToasts.delete(notification.id);
  toast.dismiss(notification.id);
}

function acknowledgeActiveNotificationToast(
  notification: Notification,
  queryClient: ReturnType<typeof useQueryClient>,
) {
  if (!activeNotificationToastIds.delete(notification.id)) return;
  activeAgentConversationToasts.delete(notification.id);
  toast.dismiss(notification.id);
  markNotificationRead(notification, queryClient);
}

function acknowledgeIfNotificationTargetSatisfied(
  notification: Notification,
  queryClient: ReturnType<typeof useQueryClient>,
) {
  if (!isAgentConversationNotificationSatisfied(notification)) return;
  const wasActive = activeAgentConversationToasts.delete(notification.id);
  const wasPending = pendingAgentConversationAcknowledgements.delete(notification.id);
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
  const selectedConversationId = useAgentSessionStore(
    (state) => state.selectedConversationId,
  );
  const agentArtifactStates = useAgentArtifactUiStore(
    (state) => state.artifactByConversationId,
  );

  useEffect(() => {
    if (!notificationsPanelOpen) return;
    const visibleToastIds = [...activeNotificationToastIds];
    activeNotificationToastIds.clear();
    activeAgentConversationToasts.clear();
    for (const id of visibleToastIds) toast.dismiss(id);
  }, [notificationsPanelOpen]);

  useEffect(() => {
    for (const notification of activeAgentConversationToasts.values()) {
      acknowledgeIfNotificationTargetSatisfied(notification, queryClient);
    }
    for (const notification of pendingAgentConversationAcknowledgements.values()) {
      acknowledgeIfNotificationTargetSatisfied(notification, queryClient);
    }
  }, [agentArtifactStates, queryClient, selectedConversationId]);

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
    if (isAgentConversation) {
      activeAgentConversationToasts.set(notification.id, notification);
      if (isAgentConversationNotificationSatisfied(notification)) {
        acknowledgeIfNotificationTargetSatisfied(notification, queryClient);
        return;
      }
    }

    activeNotificationToastIds.add(notification.id);
    const dismiss = () => dismissNotificationToast(notification);
    const performAction = async () => {
      if (isAgentConversation) {
        pendingAgentConversationAcknowledgements.set(notification.id, notification);
      }
      try {
        const navigated = await performNotificationPrimaryAction(
          notification,
          queryClient,
        );
        if (!navigated) {
          pendingAgentConversationAcknowledgements.delete(notification.id);
          return;
        }
        if (isAgentConversation) {
          acknowledgeIfNotificationTargetSatisfied(notification, queryClient);
        } else {
          acknowledgeActiveNotificationToast(notification, queryClient);
        }
      } catch {
        pendingAgentConversationAcknowledgements.delete(notification.id);
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
        activeAgentConversationToasts.delete(notification.id);
      },
    });
  }), [bus, focusedToastsEnabled, mutedProjectIds, queryClient, ready]);
}
