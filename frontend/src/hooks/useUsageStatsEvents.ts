import { useEffect } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { useEventBus } from "@/providers/EventProvider";
import { conversationStatsKey } from "@/hooks/useConversationStats";
import { insightsChatUsageStatsKeys } from "@/hooks/useInsightsMetrics";
import { projectChatUsageStatsKeys } from "@/hooks/useProjectChatUsageStats";
import { taskChatUsageStatsKeys } from "@/hooks/useTaskChatUsageStats";

type UsageUpdatedPayload = {
  conversation_id: string;
  context_id?: string;
  context_type?: string;
};

const TASK_CONTEXTS = new Set(["task", "task_execution", "review", "merge"]);

/** Always-on invalidation for persisted usage, including background conversations. */
export function useUsageStatsEvents() {
  const bus = useEventBus();
  const queryClient = useQueryClient();

  useEffect(
    () =>
      bus.subscribe<UsageUpdatedPayload>("agent:usage_updated", (payload) => {
        queryClient.invalidateQueries({
          queryKey: conversationStatsKey(payload.conversation_id),
        });

        if (payload.context_type === "project" && payload.context_id) {
          queryClient.invalidateQueries({
            queryKey: projectChatUsageStatsKeys.byProject(payload.context_id),
          });
        } else if (
          payload.context_id &&
          TASK_CONTEXTS.has(payload.context_type ?? "")
        ) {
          queryClient.invalidateQueries({
            queryKey: taskChatUsageStatsKeys.byTask(payload.context_id),
          });
          queryClient.invalidateQueries({
            queryKey: projectChatUsageStatsKeys.all,
          });
        } else if (payload.context_type === "ideation") {
          // The event carries an ideation-session ID, not its owning project ID.
          queryClient.invalidateQueries({
            queryKey: projectChatUsageStatsKeys.all,
          });
        }

        queryClient.invalidateQueries({
          queryKey: insightsChatUsageStatsKeys.all,
        });
      }),
    [bus, queryClient],
  );
}
