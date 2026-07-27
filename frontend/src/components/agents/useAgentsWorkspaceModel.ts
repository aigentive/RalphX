import { useQuery } from "@tanstack/react-query";

import { chatApi } from "@/api/chat";
import type { AgentConversationWorkspace } from "@/api/chat";
import type { ManualServiceTier } from "@/api/manual-role-defaults.types";
import type { AgentRuntimeSelection } from "@/stores/agentSessionStore";

import type { AgentConversation } from "./agentConversations";
import {
  isConversationModeLocked,
  resolveConversationAgentMode,
} from "./agentConversationMode";
import {
  getAgentTerminalArchivedReason,
  getAgentTerminalUnavailableReason,
  runtimeForWorkspaceReviewFocus,
  runtimeFromConversation,
} from "./agentConversationRuntime";
import {
  getAgentWorkspaceEffectiveBaseLabel,
  getAgentWorkspaceTerminalPublicationLabel,
  isAgentWorkspacePublishCurrent,
} from "./agentWorkspacePublishState";
import {
  AGENT_WORKSPACE_FRESHNESS_STALE_MS,
  AGENT_WORKSPACE_STALE_MS,
  agentWorkspaceKeys,
  canInspectAgentWorkspaceFreshness,
} from "./agentWorkspaceQueries";
import { normalizeRuntimeForPersistence } from "./agentOptions";
import type { AgentModelRegistry } from "@/lib/agent-models";

interface UseAgentsWorkspaceModelArgs {
  activeConversation: AgentConversation | null;
  focusedWorkspaceReviewConversation?: AgentConversation | null;
  optimisticWorkspacesByConversationId: Record<string, AgentConversationWorkspace>;
  modelRegistry: AgentModelRegistry;
  focusedWorkspaceReviewConversationId?: string | null;
  focusedWorkspaceReviewRuntimeHint?: AgentRuntimeSelection | null;
  runtimeByConversationId: Record<string, AgentRuntimeSelection>;
  selectedConversationId: string | null;
  workspaceReviewerRuntime: AgentRuntimeSelection | null;
}

export function useAgentsWorkspaceModel({
  activeConversation,
  focusedWorkspaceReviewConversation = null,
  focusedWorkspaceReviewConversationId = null,
  focusedWorkspaceReviewRuntimeHint = null,
  optimisticWorkspacesByConversationId,
  modelRegistry,
  runtimeByConversationId,
  selectedConversationId,
  workspaceReviewerRuntime,
}: UseAgentsWorkspaceModelArgs) {
  const conversationWorkspaceQuery = useQuery({
    queryKey: agentWorkspaceKeys.workspace(selectedConversationId),
    queryFn: () => chatApi.getAgentConversationWorkspace(selectedConversationId!),
    enabled:
      !!selectedConversationId &&
      activeConversation?.contextType === "project",
    staleTime: AGENT_WORKSPACE_STALE_MS,
    refetchInterval: (query) =>
      query.state.data?.maintenanceOperation?.status === "active" ? 1_500 : false,
    refetchIntervalInBackground: false,
  });
  const activeWorkspace =
    conversationWorkspaceQuery.data ??
    (selectedConversationId
      ? optimisticWorkspacesByConversationId[selectedConversationId] ?? null
      : null);
  const activeConversationMode =
    activeConversation?.contextType === "project"
      ? resolveConversationAgentMode(activeConversation, activeWorkspace)
      : null;
  const workspaceRuntime = selectedConversationId
    ? runtimeByConversationId[selectedConversationId] ??
      runtimeFromConversation(activeConversation) ??
      null
    : null;
  const activeRuntime = focusedWorkspaceReviewConversationId
    ? runtimeForWorkspaceReviewFocus(
        workspaceRuntime,
        runtimeFromConversation(focusedWorkspaceReviewConversation) ??
          runtimeByConversationId[focusedWorkspaceReviewConversationId] ??
          null,
        workspaceReviewerRuntime,
        focusedWorkspaceReviewRuntimeHint,
      )
    : workspaceRuntime;
  const focusedWorkspaceReviewServiceTier = serviceTierFromConversation(
    focusedWorkspaceReviewConversation,
  );
  const normalizedActiveRuntime = normalizeRuntimeForPersistence(
    activeRuntime,
    modelRegistry,
  );
  const terminalPublicationLabel =
    getAgentWorkspaceTerminalPublicationLabel(activeWorkspace);
  const canInspectActiveWorkspaceFreshness =
    canInspectAgentWorkspaceFreshness(activeWorkspace);
  const activeWorkspaceFreshnessQuery = useQuery({
    queryKey: agentWorkspaceKeys.scopedFreshness(selectedConversationId, "local"),
    queryFn: () =>
      chatApi.getAgentConversationWorkspaceFreshness(selectedConversationId!, {
        scope: "local",
      }),
    enabled:
      !!selectedConversationId &&
      canInspectActiveWorkspaceFreshness,
    staleTime: AGENT_WORKSPACE_FRESHNESS_STALE_MS,
  });
  const isPublishShortcutCurrent = isAgentWorkspacePublishCurrent(
    activeWorkspace,
    activeWorkspaceFreshnessQuery.data,
  );
  const activeWorkspaceFreshness = activeWorkspaceFreshnessQuery.data;
  const publishShortcutLabel = terminalPublicationLabel
    ? terminalPublicationLabel
    : activeWorkspaceFreshness?.baseStatus === "blocked"
      ? "Base unavailable"
      : activeWorkspaceFreshness?.isBaseAhead
        ? `Update from ${
            activeWorkspaceFreshness.effectiveBaseRef ??
            activeWorkspaceFreshness.baseRef ??
            activeWorkspace?.baseRef ??
            getAgentWorkspaceEffectiveBaseLabel(activeWorkspace, activeWorkspaceFreshness)
          }`
        : isPublishShortcutCurrent
          ? "Published"
          : "Commit & Publish";
  const activeConversationModeLocked = activeConversation
    ? isConversationModeLocked(activeConversation, activeWorkspace)
    : false;
  const terminalUnavailableReason = getAgentTerminalUnavailableReason(
    activeConversation,
    activeWorkspace,
  );
  const terminalArchivedReason = getAgentTerminalArchivedReason(
    activeConversation,
    activeWorkspace,
  );
  return {
    activeConversationMode,
    activeConversationModeLocked,
    activeWorkspace,
    activeWorkspaceFreshness,
    focusedWorkspaceReviewServiceTier,
    normalizedActiveRuntime,
    publishShortcutLabel,
    terminalArchivedReason,
    terminalUnavailableReason,
  };
}

function serviceTierFromConversation(
  conversation: AgentConversation | null,
): ManualServiceTier | null {
  const serviceTier = conversation?.serviceTier?.trim().toLowerCase();
  if (
    serviceTier === "provider_default" ||
    serviceTier === "standard" ||
    serviceTier === "fast"
  ) {
    return serviceTier;
  }
  return null;
}
