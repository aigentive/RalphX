import { useQuery } from "@tanstack/react-query";

import {
  chatApi,
  type AgentConversationRuntimeIndexResponse,
  type AgentConversationRuntimeIndexRow,
  type AgentConversationRuntimeSource,
  type AgentConversationRuntimeStatus,
} from "@/api/chat";

export const agentConversationRuntimeIndexKeys = {
  all: ["agents", "conversation-runtime-index"] as const,
  detail: (conversationId: string | null | undefined) =>
    [...agentConversationRuntimeIndexKeys.all, conversationId ?? "none"] as const,
};

interface UseAgentConversationRuntimeIndexOptions {
  enabled?: boolean;
}

export function isActiveRuntimeLifecycle(
  lifecycle: AgentConversationRuntimeIndexRow["lifecycle"],
): boolean {
  return (
    lifecycle === "running" ||
    lifecycle === "waiting" ||
    lifecycle === "queued"
  );
}

export function hasActiveRuntimeRow(
  index: AgentConversationRuntimeIndexResponse | null | undefined,
) {
  return (
    index?.rows.some((row) => isActiveRuntimeLifecycle(row.lifecycle)) ?? false
  );
}

function runtimeSource(
  row: AgentConversationRuntimeIndexRow,
): AgentConversationRuntimeSource {
  if (row.kind === "task") return "task_execution";
  if (row.kind === "delegation") return "workspace";
  return row.kind;
}

export function runtimeIndexToConversationStatus(
  index: AgentConversationRuntimeIndexResponse,
): AgentConversationRuntimeStatus {
  const activeRows = index.rows.filter((row) =>
    isActiveRuntimeLifecycle(row.lifecycle),
  );
  const activeMainRows = activeRows.filter((row) => row.group === "main");
  const primary = activeMainRows[0] ?? activeRows[0];
  const agentStatus = activeMainRows.some(
    (row) => row.lifecycle === "waiting",
  )
    ? "waiting_for_input"
    : "generating";

  return {
    conversationId: index.conversationId,
    isRunning: activeRows.length > 0,
    agentStatus,
    primarySource: primary ? runtimeSource(primary) : null,
    summaryLabel: primary?.statusLabel ?? null,
    items: activeRows.map((row) => ({
      source: runtimeSource(row),
      contextType: row.contextType ?? "project",
      contextId: row.contextId ?? index.conversationId,
      label: row.statusLabel,
      title: row.title,
      agentStatus:
        row.lifecycle === "waiting" ? "waiting_for_input" : "generating",
      taskId: row.taskId,
      internalStatus: row.lifecycle,
      runningProcess: null,
      ideationSession: null,
      parentSessionId: row.parentSessionId,
      childSessionId: row.childSessionId,
      conversationId: row.conversationId,
    })),
  };
}

export function useAgentConversationRuntimeIndex(
  conversationId: string | null | undefined,
  options: UseAgentConversationRuntimeIndexOptions = {},
) {
  const enabled = Boolean(conversationId) && (options.enabled ?? true);

  return useQuery<AgentConversationRuntimeIndexResponse | null, Error>({
    queryKey: agentConversationRuntimeIndexKeys.detail(conversationId),
    queryFn: async () => {
      if (!conversationId) return null;
      return chatApi.getAgentConversationRuntimeIndex(conversationId);
    },
    enabled,
    staleTime: 2_000,
    refetchInterval: (query) =>
      hasActiveRuntimeRow(query.state.data) ? 5_000 : false,
    refetchOnWindowFocus: enabled,
  });
}
