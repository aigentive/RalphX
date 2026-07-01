import { useEffect, useMemo } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";

import { chatApi, type AgentConversationRuntimeStatus } from "@/api/chat";
import type { Unsubscribe } from "@/lib/event-bus";
import { useEventBus } from "@/providers/EventProvider";

import { reconcileAgentConversationRuntimeStatus } from "./agentConversationRuntimeStore";

export const agentConversationRuntimeStatusKeys = {
  all: ["agents", "conversation-runtime-status"] as const,
  detail: (conversationId: string | null | undefined) =>
    [...agentConversationRuntimeStatusKeys.all, conversationId ?? "none"] as const,
};

interface UseAgentConversationRuntimeStatusOptions {
  enabled?: boolean;
  storeKey?: string | null | undefined;
}

export function useAgentConversationRuntimeStatus(
  conversationId: string | null | undefined,
  options: UseAgentConversationRuntimeStatusOptions = {},
) {
  const queryClient = useQueryClient();
  const bus = useEventBus();
  const enabled = Boolean(conversationId) && (options.enabled ?? true);
  const queryKey = useMemo(
    () => agentConversationRuntimeStatusKeys.detail(conversationId),
    [conversationId],
  );

  const query = useQuery<AgentConversationRuntimeStatus | null, Error>({
    queryKey,
    queryFn: async () => {
      if (!conversationId) return null;
      const statuses = await chatApi.getAgentConversationRuntimeStatuses([
        conversationId,
      ]);
      return statuses[conversationId] ?? null;
    },
    enabled,
    staleTime: 2_000,
    refetchInterval: (query) =>
      query.state.data?.isRunning ? 5_000 : false,
    refetchOnWindowFocus: enabled,
  });

  useEffect(() => {
    if (!enabled) return;

    const invalidate = () => {
      void queryClient.invalidateQueries({ queryKey });
    };
    const unsubscribes: Unsubscribe[] = [
      bus.subscribe("agent:run_started", invalidate),
      bus.subscribe("agent:run_completed", invalidate),
      bus.subscribe("agent:turn_completed", invalidate),
      bus.subscribe("agent:stopped", invalidate),
      bus.subscribe("agent:error", invalidate),
      bus.subscribe("task:status_changed", invalidate),
      bus.subscribe("execution:status_changed", invalidate),
      bus.subscribe("step:status_changed", invalidate),
      bus.subscribe("plan_verification:status_changed", invalidate),
      bus.subscribe("workspace_review_artifact:created", invalidate),
      bus.subscribe("workspace_review_artifact:updated", invalidate),
    ];

    return () => {
      unsubscribes.forEach((unsubscribe) => unsubscribe());
    };
  }, [bus, enabled, queryClient, queryKey]);

  useEffect(() => {
    if (!enabled || !conversationId || !query.isSuccess) {
      return;
    }

    reconcileAgentConversationRuntimeStatus(conversationId, query.data, {
      storeKey: options.storeKey,
    });
  }, [conversationId, enabled, options.storeKey, query.data, query.isSuccess]);

  return query;
}
