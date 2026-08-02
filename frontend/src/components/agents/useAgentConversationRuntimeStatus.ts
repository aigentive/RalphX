import { useEffect, useMemo } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";

import { chatApi, type AgentConversationRuntimeStatus } from "@/api/chat";
import {
  isRemoteEnvironmentActive,
  useIsRemoteEnvironment,
} from "@/hooks/useActiveEnvironment";
import { isRemotelyAvailable } from "@/lib/remote/agent-gate";
import type { Unsubscribe } from "@/lib/event-bus";
import { useEventBus } from "@/providers/EventProvider";

import {
  reconcileAgentConversationRuntimeStatus,
  type AgentConversationRuntimeStatusMirrorOption,
  type AgentConversationRuntimeStatusMirrorSelector,
} from "./agentConversationRuntimeStore";
import { runtimeIndexToConversationStatus } from "./useAgentConversationRuntimeIndex";

export const agentConversationRuntimeStatusKeys = {
  all: ["agents", "conversation-runtime-status"] as const,
  detail: (conversationId: string | null | undefined) =>
    [...agentConversationRuntimeStatusKeys.all, conversationId ?? "none"] as const,
};

interface UseAgentConversationRuntimeStatusOptions {
  enabled?: boolean;
  invalidateUnknownRuntimeIds?: boolean;
  mirrorToVisibleChatStatus?: AgentConversationRuntimeStatusMirrorOption;
  selectVisibleChatStatus?: AgentConversationRuntimeStatusMirrorSelector;
  storeKey?: string | null | undefined;
}

const RUNTIME_STATUS_EVENT_ID_KEYS = [
  "conversation_id",
  "conversationId",
  "parent_conversation_id",
  "parentConversationId",
  "child_conversation_id",
  "childConversationId",
  "context_id",
  "contextId",
  "task_id",
  "taskId",
  "parent_session_id",
  "parentSessionId",
  "child_session_id",
  "childSessionId",
  "session_id",
  "sessionId",
] as const;

interface RuntimeStatusInvalidationOptions {
  invalidateUnknownRuntimeIds?: boolean;
}

function addStringId(ids: Set<string>, value: unknown) {
  if (typeof value !== "string") {
    return;
  }
  const trimmed = value.trim();
  if (trimmed.length > 0) {
    ids.add(trimmed);
  }
}

function runtimeStatusIds(
  conversationId: string | null | undefined,
  status: AgentConversationRuntimeStatus | null | undefined,
): Set<string> {
  const ids = new Set<string>();
  addStringId(ids, conversationId);
  addStringId(ids, status?.conversationId);

  for (const item of status?.items ?? []) {
    addStringId(ids, item.contextId);
    addStringId(ids, item.taskId);
    addStringId(ids, item.parentSessionId);
    addStringId(ids, item.childSessionId);
    addStringId(ids, item.conversationId);
  }

  return ids;
}

function payloadRuntimeIds(payload: unknown): Set<string> | null {
  if (!payload || typeof payload !== "object") {
    return null;
  }

  const record = payload as Record<string, unknown>;
  const ids = new Set<string>();
  for (const key of RUNTIME_STATUS_EVENT_ID_KEYS) {
    addStringId(ids, record[key]);
  }

  return ids.size > 0 ? ids : null;
}

function shouldInvalidateRuntimeStatusForPayload(
  conversationId: string | null | undefined,
  status: AgentConversationRuntimeStatus | null | undefined,
  payload: unknown,
  options: RuntimeStatusInvalidationOptions = {},
): boolean {
  const payloadIds = payloadRuntimeIds(payload);
  if (!payloadIds) {
    return options.invalidateUnknownRuntimeIds !== false;
  }

  const knownIds = runtimeStatusIds(conversationId, status);
  for (const id of payloadIds) {
    if (knownIds.has(id)) {
      return true;
    }
  }

  return options.invalidateUnknownRuntimeIds === true;
}

export function useAgentConversationRuntimeStatus(
  conversationId: string | null | undefined,
  options: UseAgentConversationRuntimeStatusOptions = {},
) {
  const queryClient = useQueryClient();
  const bus = useEventBus();
  const isRemoteEnvironment = useIsRemoteEnvironment();
  const enabled = Boolean(conversationId) && (options.enabled ?? true);
  const queryKey = useMemo(
    () => agentConversationRuntimeStatusKeys.detail(conversationId),
    [conversationId],
  );

  const query = useQuery<AgentConversationRuntimeStatus | null, Error>({
    queryKey,
    queryFn: async () => {
      if (!conversationId) return null;
      if (
        isRemoteEnvironmentActive() &&
        isRemotelyAvailable("get_agent_conversation_runtime_index")
      ) {
        return runtimeIndexToConversationStatus(
          await chatApi.getAgentConversationRuntimeIndex(conversationId),
        );
      }
      const statuses = await chatApi.getAgentConversationRuntimeStatuses([
        conversationId,
      ]);
      return statuses[conversationId] ?? null;
    },
    enabled,
    staleTime: 2_000,
    // Keep polling while the query is FAILING. On an error with no prior success
    // `state.data` is undefined, which used to read as "not running" and stopped the poll
    // permanently — so one transient backend failure froze the pane until something else
    // invalidated the key. A failing liveness probe is exactly when polling must continue.
    refetchInterval: (query) => {
      if (isRemoteEnvironment) {
        return query.state.data?.isRunning ? 5_000 : false;
      }
      return query.state.data?.isRunning || query.state.status === "error"
        ? 5_000
        : false;
    },
    refetchOnWindowFocus: enabled,
  });

  useEffect(() => {
    if (!enabled) return;

    const invalidate = (
      payload: unknown,
      options?: RuntimeStatusInvalidationOptions,
    ) => {
      if (
        !shouldInvalidateRuntimeStatusForPayload(
          conversationId,
          query.data,
          payload,
          options,
        )
      ) {
        return;
      }
      void queryClient.invalidateQueries({ queryKey });
    };
    const invalidateKnownRuntime = (payload: unknown) => {
      invalidate(payload);
    };
    const invalidatePossibleNewRuntime = (payload: unknown) => {
      invalidate(payload, {
        invalidateUnknownRuntimeIds:
          options.invalidateUnknownRuntimeIds ?? true,
      });
    };
    const unsubscribes: Unsubscribe[] = [
      bus.subscribe("agent:run_started", invalidatePossibleNewRuntime),
      bus.subscribe("agent:run_completed", invalidateKnownRuntime),
      bus.subscribe("agent:turn_completed", invalidateKnownRuntime),
      bus.subscribe("agent:stopped", invalidateKnownRuntime),
      bus.subscribe("agent:error", invalidateKnownRuntime),
      bus.subscribe("task:status_changed", invalidatePossibleNewRuntime),
      bus.subscribe("execution:status_changed", invalidateKnownRuntime),
      bus.subscribe("step:status_changed", invalidateKnownRuntime),
      bus.subscribe("plan_verification:status_changed", invalidateKnownRuntime),
      bus.subscribe("workspace_review_artifact:created", invalidateKnownRuntime),
      bus.subscribe("workspace_review_artifact:updated", invalidateKnownRuntime),
    ];

    return () => {
      unsubscribes.forEach((unsubscribe) => unsubscribe());
    };
  }, [
    bus,
    conversationId,
    enabled,
    options.invalidateUnknownRuntimeIds,
    query.data,
    queryClient,
    queryKey,
  ]);

  useEffect(() => {
    if (!enabled || !conversationId || !query.isSuccess) {
      return;
    }

    reconcileAgentConversationRuntimeStatus(conversationId, query.data, {
      ...(options.mirrorToVisibleChatStatus !== undefined
        ? { mirrorToVisibleChatStatus: options.mirrorToVisibleChatStatus }
        : {}),
      ...(options.selectVisibleChatStatus !== undefined
        ? { selectVisibleChatStatus: options.selectVisibleChatStatus }
        : {}),
      ...(options.storeKey !== undefined ? { storeKey: options.storeKey } : {}),
    });
  }, [
    conversationId,
    enabled,
    options.mirrorToVisibleChatStatus,
    options.selectVisibleChatStatus,
    options.storeKey,
    query.data,
    query.isSuccess,
  ]);

  return query;
}
