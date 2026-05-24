import type { QueryClient } from "@tanstack/react-query";

import { chatApi } from "@/api/chat";
import type {
  AgentConversationWorkspace,
  AgentConversationWorkspaceFreshnessScope,
} from "@/api/chat";

import {
  getAgentWorkspaceTerminalPublicationLabel,
  hasPublishedWorkspacePr,
} from "./agentWorkspacePublishState";

export const AGENT_WORKSPACE_STALE_MS = 5_000;
export const AGENT_WORKSPACE_FRESHNESS_STALE_MS = 60_000;

export const agentWorkspaceKeys = {
  workspace: (conversationId: string | null | undefined) => [
    "agents",
    "conversation-workspace",
    conversationId,
  ] as const,
  freshness: (conversationId: string | null | undefined) => [
    "agents",
    "conversation-workspace-freshness",
    conversationId,
  ] as const,
  scopedFreshness: (
    conversationId: string | null | undefined,
    scope: AgentConversationWorkspaceFreshnessScope,
  ) => [
    "agents",
    "conversation-workspace-freshness",
    conversationId,
    scope,
  ] as const,
  publicationEvents: (conversationId: string | null | undefined) => [
    "agents",
    "conversation-workspace-publication-events",
    conversationId,
  ] as const,
  review: (conversationId: string | null | undefined) => [
    "agents",
    "workspace-review",
    conversationId,
  ] as const,
  agentTasks: (conversationId: string | null | undefined) => [
    "agents",
    "conversation-agent-tasks",
    conversationId,
  ] as const,
  agentTaskLists: (conversationId: string | null | undefined) => [
    "agents",
    "conversation-agent-task-lists",
    conversationId,
  ] as const,
  agentTaskListTasks: (
    conversationId: string | null | undefined,
    taskListId: string | null | undefined,
  ) => [
    "agents",
    "conversation-agent-task-list-tasks",
    conversationId,
    taskListId,
  ] as const,
  prAnnotations: (conversationId: string | null | undefined) => [
    "agents",
    "workspace-pr-annotations",
    conversationId,
  ] as const,
  diff: (conversationId: string | null | undefined) => [
    "agents",
    "workspace-diff",
    conversationId,
  ] as const,
  commits: (conversationId: string | null | undefined) => [
    "agents",
    "workspace-commits",
    conversationId,
  ] as const,
};

const pendingFreshnessPreflights = new Set<string>();

export function canInspectAgentWorkspaceFreshness(
  workspace: AgentConversationWorkspace | null | undefined,
): workspace is AgentConversationWorkspace {
  const terminalPublicationLabel =
    getAgentWorkspaceTerminalPublicationLabel(workspace ?? null);
  const hasPublishedPr = hasPublishedWorkspacePr(workspace ?? null);
  return (
    Boolean(workspace) &&
    !terminalPublicationLabel &&
    (workspace?.mode === "edit" || hasPublishedPr) &&
    (workspace?.mode !== "edit" || workspace?.status !== "missing")
  );
}

export async function preflightAgentWorkspaceFreshness(
  queryClient: QueryClient,
  conversationId: string,
): Promise<void> {
  if (pendingFreshnessPreflights.has(conversationId)) {
    return;
  }

  pendingFreshnessPreflights.add(conversationId);
  try {
    const workspace = await queryClient.fetchQuery({
      queryKey: agentWorkspaceKeys.workspace(conversationId),
      queryFn: () => chatApi.getAgentConversationWorkspace(conversationId),
      staleTime: AGENT_WORKSPACE_STALE_MS,
    });

    if (!canInspectAgentWorkspaceFreshness(workspace)) {
      return;
    }

    await queryClient.prefetchQuery({
      queryKey: agentWorkspaceKeys.scopedFreshness(conversationId, "local"),
      queryFn: () =>
        chatApi.getAgentConversationWorkspaceFreshness(conversationId, {
          scope: "local",
        }),
      staleTime: AGENT_WORKSPACE_FRESHNESS_STALE_MS,
    });
  } catch {
    // Mounted workspace views handle user-visible freshness errors.
  } finally {
    pendingFreshnessPreflights.delete(conversationId);
  }
}

export function invalidateWorkspaceQueries(
  queryClient: QueryClient,
  conversationId: string,
): Promise<unknown[]> {
  return Promise.all([
    queryClient.invalidateQueries({
      queryKey: agentWorkspaceKeys.workspace(conversationId),
    }),
    queryClient.invalidateQueries({
      queryKey: agentWorkspaceKeys.freshness(conversationId),
    }),
    queryClient.invalidateQueries({
      queryKey: agentWorkspaceKeys.publicationEvents(conversationId),
    }),
    queryClient.invalidateQueries({
      queryKey: agentWorkspaceKeys.review(conversationId),
    }),
    queryClient.invalidateQueries({
      queryKey: agentWorkspaceKeys.agentTasks(conversationId),
    }),
    queryClient.invalidateQueries({
      queryKey: agentWorkspaceKeys.agentTaskLists(conversationId),
    }),
    queryClient.invalidateQueries({
      queryKey: agentWorkspaceKeys.prAnnotations(conversationId),
    }),
    queryClient.invalidateQueries({
      queryKey: agentWorkspaceKeys.diff(conversationId),
    }),
    queryClient.invalidateQueries({
      queryKey: agentWorkspaceKeys.commits(conversationId),
    }),
  ]);
}
