import { useQuery } from "@tanstack/react-query";

import { chatApi } from "@/api/chat";
import type { AgentConversationWorkspace } from "@/api/chat";
import type { AgentRuntimeSelection } from "@/stores/agentSessionStore";

import type { AgentConversation } from "./agentConversations";
import {
  isWorkspaceModeLocked,
  resolveConversationAgentMode,
} from "./agentConversationMode";
import {
  getAgentTerminalUnavailableReason,
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
import { normalizeRuntimeSelection } from "./agentOptions";
import type { AgentModelRegistry } from "@/lib/agent-models";

interface UseAgentsWorkspaceModelArgs {
  activeConversation: AgentConversation | null;
  optimisticWorkspacesByConversationId: Record<string, AgentConversationWorkspace>;
  modelRegistry: AgentModelRegistry;
  runtimeByConversationId: Record<string, AgentRuntimeSelection>;
  selectedConversationId: string | null;
}

export function useAgentsWorkspaceModel({
  activeConversation,
  optimisticWorkspacesByConversationId,
  modelRegistry,
  runtimeByConversationId,
  selectedConversationId,
}: UseAgentsWorkspaceModelArgs) {
  const conversationWorkspaceQuery = useQuery({
    queryKey: agentWorkspaceKeys.workspace(selectedConversationId),
    queryFn: () => chatApi.getAgentConversationWorkspace(selectedConversationId!),
    enabled:
      !!selectedConversationId &&
      activeConversation?.contextType === "project",
    staleTime: AGENT_WORKSPACE_STALE_MS,
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
  const activeRuntime = selectedConversationId
    ? runtimeByConversationId[selectedConversationId] ??
      runtimeFromConversation(activeConversation) ??
      null
    : null;
  const normalizedActiveRuntime = normalizeRuntimeSelection(activeRuntime, modelRegistry);
  const terminalPublicationLabel =
    getAgentWorkspaceTerminalPublicationLabel(activeWorkspace);
  const canInspectActiveWorkspaceFreshness =
    canInspectAgentWorkspaceFreshness(activeWorkspace);
  const activeWorkspaceFreshnessQuery = useQuery({
    queryKey: agentWorkspaceKeys.freshness(selectedConversationId),
    queryFn: () => chatApi.getAgentConversationWorkspaceFreshness(selectedConversationId!),
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
  const activeConversationModeLocked =
    activeConversationMode === "ideation" || isWorkspaceModeLocked(activeWorkspace);
  const terminalUnavailableReason = getAgentTerminalUnavailableReason(
    activeConversation,
    activeWorkspace,
  );
  return {
    activeConversationMode,
    activeConversationModeLocked,
    activeWorkspace,
    activeWorkspaceFreshness,
    normalizedActiveRuntime,
    publishShortcutLabel,
    terminalUnavailableReason,
  };
}
