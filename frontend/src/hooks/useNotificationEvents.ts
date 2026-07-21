import { useEffect } from "react";
import { useQueryClient } from "@tanstack/react-query";

import { notificationsApi } from "@/api/notifications";
import { navigateNotification } from "@/components/notifications/notificationNavigation";
import { useEventBus } from "@/providers/EventProvider";
import { NotificationSchema } from "@/types/notifications";
import { attentionKeys } from "./useAttentionItems";
import { notificationKeys } from "./useNotificationHistory";

const ATTENTION_INVALIDATION_EVENTS = [
  "review:update",
  "task:status_changed",
  "permission:request",
  "permission:expired",
  "permission:resolved",
  "agent:ask_user_question",
  "agent:question_resolved",
  "automation:updated",
  "automation:run:updated",
  "plan_artifact:created",
  "plan_artifact:approved",
  "pr_review_artifact:created",
  "pr_review_artifact:updated",
] as const;

export function useNotificationEvents() {
  const bus = useEventBus();
  const queryClient = useQueryClient();

  useEffect(() => {
    const invalidate = () =>
      void queryClient.invalidateQueries({ queryKey: attentionKeys.all });
    const unsubscribes = ATTENTION_INVALIDATION_EVENTS.map((event) =>
      bus.subscribe(event, invalidate),
    );
    const invalidateNotificationHistory = () =>
      void queryClient.invalidateQueries({ queryKey: notificationKeys.all });
    unsubscribes.push(
      bus.subscribe("notification:created", () => {
        invalidate();
        invalidateNotificationHistory();
      }),
      bus.subscribe("notification:updated", () => {
        invalidate();
        invalidateNotificationHistory();
      }),
      bus.subscribe<unknown>("notification:desktop_activated", (payload) => {
        const parsed = NotificationSchema.safeParse(payload);
        if (!parsed.success) return;
        const notification = parsed.data;
        void navigateNotification(notification, queryClient).then(
          (navigated) => {
            if (!navigated) return;
            void notificationsApi
              .markRead(notification.id)
              .catch(() => undefined)
              .finally(invalidateNotificationHistory);
          },
        );
      }),
    );
    return () => unsubscribes.forEach((unsubscribe) => unsubscribe());
  }, [bus, queryClient]);
}
