import { useQuery } from "@tanstack/react-query";
import { useMemo } from "react";

import { chatApi, type AgentConversationWorkspace } from "@/api/chat";

export const agentConversationWorkspaceKeys = {
  all: ["agents", "conversation-workspaces-by-project"] as const,
  project: (projectId: string | null | undefined) =>
    [...agentConversationWorkspaceKeys.all, projectId ?? ""] as const,
};

export function useProjectAgentConversationWorkspaces(
  projectId: string | null | undefined,
  options?: { enabled?: boolean }
) {
  const query = useQuery({
    queryKey: agentConversationWorkspaceKeys.project(projectId),
    queryFn: () => chatApi.listAgentConversationWorkspacesByProject(projectId!),
    enabled: Boolean(projectId) && (options?.enabled ?? true),
    staleTime: 5_000,
  });

  const byConversationId = useMemo(() => {
    const entries = (query.data ?? []).map((workspace) => [
      workspace.conversationId,
      workspace,
    ] as const);
    return new Map<string, AgentConversationWorkspace>(entries);
  }, [query.data]);

  return {
    ...query,
    data: query.data ?? [],
    byConversationId,
  };
}
