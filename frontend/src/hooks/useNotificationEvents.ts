import { useEffect } from "react";
import { useQueryClient } from "@tanstack/react-query";

import { useEventBus } from "@/providers/EventProvider";
import { attentionKeys } from "./useAttentionItems";

const ATTENTION_INVALIDATION_EVENTS = [
  "review:update", "task:status_changed", "permission:request", "permission:expired",
  "agent:ask_user_question", "agent:question_resolved", "automation:updated",
  "automation:run:updated", "plan_artifact:created", "plan_artifact:approved",
  "pr_review_artifact:created", "pr_review_artifact:updated",
] as const;

export function useNotificationEvents() {
  const bus = useEventBus();
  const queryClient = useQueryClient();

  useEffect(() => {
    const invalidate = () => void queryClient.invalidateQueries({ queryKey: attentionKeys.all });
    const unsubscribes = ATTENTION_INVALIDATION_EVENTS.map((event) => bus.subscribe(event, invalidate));
    return () => unsubscribes.forEach((unsubscribe) => unsubscribe());
  }, [bus, queryClient]);
}
