import type { QueryClient } from "@tanstack/react-query";

import { chatApi } from "@/api/chat";
import { logger } from "@/lib/logger";
import type {
  AgentConversationWorkspace,
  AgentConversationWorkspaceFreshnessScope,
  AgentConversationWorkspaceMode,
  AgentWorkspacePrReviewContext,
  AgentWorkspaceReviewContext,
  StartAgentWorkspaceReviewFixerResult,
  StartAgentWorkspaceReviewResult,
} from "@/api/chat";

import {
  blocksAgentWorkspaceGitInspection,
  getAgentWorkspaceTerminalPublicationLabel,
  hasPublishedWorkspacePr,
} from "./agentWorkspacePublishState";

export const AGENT_WORKSPACE_STALE_MS = 5_000;
export const AGENT_WORKSPACE_FRESHNESS_STALE_MS = 60_000;

export type WorkspaceReviewRefreshMode = "status" | "full_target";

type WorkspaceReviewContextFetcher = (
  conversationId: string,
  options: { signal?: AbortSignal; refreshTarget?: boolean },
) => Promise<AgentWorkspaceReviewContext>;

interface WorkspaceReviewRefreshState {
  requestedMode: WorkspaceReviewRefreshMode | null;
  running: Promise<void> | null;
}

const workspaceReviewRefreshes = new WeakMap<
  QueryClient,
  Map<string, WorkspaceReviewRefreshState>
>();

function mergeWorkspaceReviewRefreshMode(
  current: WorkspaceReviewRefreshMode | null,
  requested: WorkspaceReviewRefreshMode,
): WorkspaceReviewRefreshMode {
  return current === "full_target" || requested === "full_target"
    ? "full_target"
    : "status";
}

export function refreshWorkspaceReviewContext(
  queryClient: QueryClient,
  conversationId: string,
  mode: WorkspaceReviewRefreshMode = "status",
  fetchContext: WorkspaceReviewContextFetcher =
    chatApi.getAgentWorkspaceReviewContext,
): Promise<void> {
  let clientRefreshes = workspaceReviewRefreshes.get(queryClient);
  if (!clientRefreshes) {
    clientRefreshes = new Map();
    workspaceReviewRefreshes.set(queryClient, clientRefreshes);
  }
  let state = clientRefreshes.get(conversationId);
  if (!state) {
    state = { requestedMode: null, running: null };
    clientRefreshes.set(conversationId, state);
  }
  state.requestedMode = mergeWorkspaceReviewRefreshMode(
    state.requestedMode,
    mode,
  );
  if (state.running) {
    return state.running;
  }

  const refreshState = state;
  const refreshes = clientRefreshes;
  refreshState.running = (async () => {
    try {
      while (refreshState.requestedMode) {
        const requestedMode = refreshState.requestedMode;
        refreshState.requestedMode = null;
        const queryKey = agentWorkspaceKeys.workspaceReview(conversationId);
        const activeQuery = queryClient.getQueryCache().find({
          queryKey,
          exact: true,
        });
        if (activeQuery?.state.fetchStatus === "fetching") {
          await queryClient.refetchQueries(
            { queryKey, exact: true },
            { cancelRefetch: false },
          );
          if (requestedMode === "status") {
            continue;
          }
        }
        await queryClient.fetchQuery({
          queryKey,
          queryFn: ({ signal }) =>
            fetchContext(conversationId, {
              signal,
              refreshTarget: requestedMode === "full_target",
            }),
          staleTime: 0,
        });
      }
    } finally {
      refreshState.running = null;
      refreshState.requestedMode = null;
      refreshes.delete(conversationId);
    }
  })();
  return refreshState.running;
}

export async function refreshWorkspaceReviewAfterSignal(
  queryClient: QueryClient,
  conversationId: string,
): Promise<void> {
  await refreshWorkspaceReviewContext(queryClient, conversationId, "status");
  await refreshWorkspaceReviewContext(
    queryClient,
    conversationId,
    "full_target",
  );
}

const agentTaskScopeKey = (
  contextType: string | null | undefined,
  contextId: string | null | undefined,
) => ["agents", "agent-task-scope", contextType, contextId] as const;

export const agentWorkspaceKeys = {
  workspace: (conversationId: string | null | undefined) =>
    ["agents", "conversation-workspace", conversationId] as const,
  freshness: (conversationId: string | null | undefined) =>
    ["agents", "conversation-workspace-freshness", conversationId] as const,
  scopedFreshness: (
    conversationId: string | null | undefined,
    scope: AgentConversationWorkspaceFreshnessScope,
  ) =>
    [
      "agents",
      "conversation-workspace-freshness",
      conversationId,
      scope,
    ] as const,
  publicationEvents: (conversationId: string | null | undefined) =>
    [
      "agents",
      "conversation-workspace-publication-events",
      conversationId,
    ] as const,
  review: (conversationId: string | null | undefined) =>
    ["agents", "workspace-review", conversationId] as const,
  changeSummary: (conversationId: string | null | undefined) =>
    ["agents", "workspace-change-summary", conversationId] as const,
  agentTasks: (conversationId: string | null | undefined) =>
    agentTaskScopeKey("conversation", conversationId),
  agentTaskLists: (conversationId: string | null | undefined) =>
    [...agentTaskScopeKey("conversation", conversationId), "lists"] as const,
  agentTaskListTasks: (
    conversationId: string | null | undefined,
    taskListId: string | null | undefined,
  ) =>
    [
      ...agentTaskScopeKey("conversation", conversationId),
      "list-tasks",
      taskListId,
    ] as const,
  agentTasksForScope: agentTaskScopeKey,
  agentTaskListsForScope: (
    contextType: string | null | undefined,
    contextId: string | null | undefined,
  ) => [...agentTaskScopeKey(contextType, contextId), "lists"] as const,
  agentTaskListTasksForScope: (
    contextType: string | null | undefined,
    contextId: string | null | undefined,
    taskListId: string | null | undefined,
  ) =>
    [
      ...agentTaskScopeKey(contextType, contextId),
      "list-tasks",
      taskListId,
    ] as const,
  prAnnotations: (conversationId: string | null | undefined) =>
    ["agents", "workspace-pr-annotations", conversationId] as const,
  workspaceReviewHunkAnnotations: (conversationId: string | null | undefined) =>
    ["agents", "workspace-review-hunk-annotations", conversationId] as const,
  prReview: (conversationId: string | null | undefined) =>
    ["agents", "workspace-pr-review", conversationId] as const,
  workspaceReview: (conversationId: string | null | undefined) =>
    ["agents", "workspace-review-context", conversationId] as const,
  diff: (conversationId: string | null | undefined) =>
    ["agents", "workspace-diff", conversationId] as const,
  commits: (conversationId: string | null | undefined) =>
    ["agents", "workspace-commits", conversationId] as const,
};

export function prReviewContextForConversation(
  context: AgentWorkspacePrReviewContext | null | undefined,
  conversationId: string | null | undefined,
): AgentWorkspacePrReviewContext | null {
  if (!context || !conversationId) {
    return null;
  }

  if (context.workspace.conversationId !== conversationId) {
    return null;
  }
  if (
    context.monitor?.conversationId &&
    context.monitor.conversationId !== conversationId
  ) {
    return null;
  }
  if (
    context.pendingAction?.conversationId &&
    context.pendingAction.conversationId !== conversationId
  ) {
    return null;
  }
  if (
    context.recentActions.some(
      (action) => action.conversationId !== conversationId,
    )
  ) {
    return null;
  }

  return context;
}

type WorkspaceReviewContextLike =
  | AgentWorkspaceReviewContext
  | StartAgentWorkspaceReviewFixerResult
  | StartAgentWorkspaceReviewResult;

const LOCAL_WORKSPACE_REVIEW_OWNER_MODES: ReadonlySet<AgentConversationWorkspaceMode> =
  new Set(["edit", "ideation"]);

export function isLocalWorkspaceReviewModeEligible(
  mode: AgentConversationWorkspaceMode | null | undefined,
): boolean {
  return mode ? LOCAL_WORKSPACE_REVIEW_OWNER_MODES.has(mode) : false;
}

export interface WorkspaceReviewOwnerConversationInput {
  activeConversationContextType: string | null | undefined;
  activeConversationId: string | null | undefined;
  activeConversationParentId: string | null | undefined;
  activeConversationMode: AgentConversationWorkspaceMode | null | undefined;
  activeWorkspaceConversationId: string | null | undefined;
}

export function resolveWorkspaceReviewOwnerConversationId({
  activeConversationContextType,
  activeConversationId,
  activeConversationParentId,
  activeConversationMode,
  activeWorkspaceConversationId,
}: WorkspaceReviewOwnerConversationInput): string | null {
  if (activeConversationContextType !== "project" || !activeConversationId) {
    return null;
  }

  const selectedConversationOwnsWorkspace =
    activeWorkspaceConversationId === activeConversationId;
  const hasReviewableWorkspaceMode = isLocalWorkspaceReviewModeEligible(
    activeConversationMode,
  );
  if (selectedConversationOwnsWorkspace) {
    return hasReviewableWorkspaceMode ? activeConversationId : null;
  }

  if (activeConversationParentId) {
    return activeConversationParentId;
  }

  return hasReviewableWorkspaceMode ? activeConversationId : null;
}

export function workspaceReviewContextForConversation<
  T extends WorkspaceReviewContextLike,
>(
  context: T | null | undefined,
  conversationId: string | null | undefined,
): T | null {
  if (!context || !conversationId) {
    return null;
  }

  if (
    "workspace" in context &&
    context.workspace.conversationId !== conversationId
  ) {
    return null;
  }
  if (context.monitor.conversationId !== conversationId) {
    return null;
  }

  return context;
}

const pendingFreshnessPreflights = new Set<string>();

export function canInspectAgentWorkspaceFreshness(
  workspace: AgentConversationWorkspace | null | undefined,
): workspace is AgentConversationWorkspace {
  if (blocksAgentWorkspaceGitInspection(workspace)) {
    return false;
  }
  const terminalPublicationLabel = getAgentWorkspaceTerminalPublicationLabel(
    workspace ?? null,
  );
  const hasPublishedPr = hasPublishedWorkspacePr(workspace ?? null);
  return (
    Boolean(workspace) &&
    !terminalPublicationLabel &&
    (workspace?.mode === "edit" || workspace?.mode === "plan" || hasPublishedPr) &&
    ((workspace?.mode !== "edit" && workspace?.mode !== "plan") ||
      workspace?.status !== "missing")
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
    let workspace: AgentConversationWorkspace | null;
    try {
      workspace = await queryClient.fetchQuery({
        queryKey: agentWorkspaceKeys.workspace(conversationId),
        queryFn: () => chatApi.getAgentConversationWorkspace(conversationId),
        staleTime: AGENT_WORKSPACE_STALE_MS,
      });
    } catch (error) {
      logger.error("Failed to preload agent workspace", {
        conversationId,
        error,
      });
      return;
    }

    if (!canInspectAgentWorkspaceFreshness(workspace)) {
      return;
    }

    try {
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
    }
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
      queryKey: agentWorkspaceKeys.changeSummary(conversationId),
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
      queryKey:
        agentWorkspaceKeys.workspaceReviewHunkAnnotations(conversationId),
    }),
    queryClient.invalidateQueries({
      queryKey: agentWorkspaceKeys.prReview(conversationId),
    }),
    queryClient.invalidateQueries({
      queryKey: agentWorkspaceKeys.diff(conversationId),
    }),
    queryClient.invalidateQueries({
      queryKey: agentWorkspaceKeys.commits(conversationId),
    }),
  ]);
}
