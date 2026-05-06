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
  getAgentWorkspaceTerminalPublicationLabel,
  hasPublishedWorkspacePr,
  isAgentWorkspacePublishCurrent,
} from "./agentWorkspacePublishState";
import { normalizeRuntimeSelection } from "./agentOptions";
import { useDeferredAgentHydration } from "./useDeferredAgentHydration";
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
    queryKey: ["agents", "conversation-workspace", selectedConversationId],
    queryFn: () => chatApi.getAgentConversationWorkspace(selectedConversationId!),
    enabled:
      !!selectedConversationId &&
      activeConversation?.contextType === "project",
    staleTime: 5_000,
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
  const activeWorkspaceHasPublishedPr = hasPublishedWorkspacePr(activeWorkspace);
  const canInspectActiveWorkspaceFreshness =
    Boolean(activeWorkspace) &&
    !terminalPublicationLabel &&
    (activeWorkspace?.mode === "edit" || activeWorkspaceHasPublishedPr) &&
    (activeWorkspace?.mode !== "edit" || activeWorkspace?.status !== "missing");
  const canHydrateActiveWorkspaceFreshness = useDeferredAgentHydration(
    selectedConversationId && canInspectActiveWorkspaceFreshness
      ? selectedConversationId
      : null,
  );
  const activeWorkspaceFreshnessQuery = useQuery({
    queryKey: ["agents", "conversation-workspace-freshness", selectedConversationId],
    queryFn: () => chatApi.getAgentConversationWorkspaceFreshness(selectedConversationId!),
    enabled:
      canHydrateActiveWorkspaceFreshness &&
      !!selectedConversationId &&
      canInspectActiveWorkspaceFreshness,
    staleTime: 5_000,
  });
  const isPublishShortcutCurrent = isAgentWorkspacePublishCurrent(
    activeWorkspace,
    activeWorkspaceFreshnessQuery.data,
  );
  const publishShortcutLabel = terminalPublicationLabel
    ? terminalPublicationLabel
    : activeWorkspaceFreshnessQuery.data?.isBaseAhead
    ? `Update from ${activeWorkspaceFreshnessQuery.data.baseRef}`
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
    normalizedActiveRuntime,
    publishShortcutLabel,
    terminalUnavailableReason,
  };
}
