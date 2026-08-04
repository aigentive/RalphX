import { useQuery } from "@tanstack/react-query";

import { chatApi } from "@/api/chat";
import type { AgentConversationWorkspace } from "@/api/chat";
import type { ManualServiceTier } from "@/api/manual-role-defaults.types";
import type { AgentRuntimeSelection } from "@/stores/agentSessionStore";
import {
  getProjectWorkspacePublishMode,
  type Project,
} from "@/types/project";

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
  getAgentWorkspacePrAutofixFingerprintSpendPresentation,
  getAgentWorkspaceTerminalPublicationLabel,
  hasPublishedWorkspacePr,
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
  activeProject?: Project | null;
  composerRuntimeOverridesByConversationId?: Record<
    string,
    AgentRuntimeSelection
  >;
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
  activeProject = null,
  composerRuntimeOverridesByConversationId = {},
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
  const activeWorkspace = conversationWorkspaceQuery.isError
    ? null
    : conversationWorkspaceQuery.data ??
      (selectedConversationId
        ? optimisticWorkspacesByConversationId[selectedConversationId] ?? null
        : null);
  const workspaceHydrationStatus = conversationWorkspaceQuery.isError
    ? "error"
    : conversationWorkspaceQuery.isPending
      ? "loading"
      : conversationWorkspaceQuery.data
        ? "resolved"
        : "absent";
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
        composerRuntimeOverridesByConversationId[
          focusedWorkspaceReviewConversationId
        ] ?? null,
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
  const activeWorkspaceFingerprintSpend =
    getAgentWorkspacePrAutofixFingerprintSpendPresentation(activeWorkspace);
  const workspacePublishMode = getProjectWorkspacePublishMode(
    activeProject,
    hasPublishedWorkspacePr(activeWorkspace),
  );
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
  const activeWorkspaceFreshness = canInspectActiveWorkspaceFreshness
    ? activeWorkspaceFreshnessQuery.data
    : undefined;
  const publishShortcutLabel = terminalPublicationLabel
    ? terminalPublicationLabel
    : workspacePublishMode.kind === "unavailable"
      ? "Repository setup required"
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
          : workspacePublishMode.kind === "localCommit"
            ? "Commit locally"
            : "Commit & Publish";
  const activeConversationModeLocked = activeConversation
    ? isConversationModeLocked(activeConversation, activeWorkspace)
    : false;
  const terminalUnavailableReason = conversationWorkspaceQuery.isError
    ? "Workspace details could not be loaded. Retry the connection before continuing."
    : getAgentTerminalUnavailableReason(activeConversation, activeWorkspace);
  const terminalArchivedReason = getAgentTerminalArchivedReason(
    activeConversation,
    activeWorkspace,
  );
  return {
    activeConversationMode,
    activeConversationModeLocked,
    activeWorkspace,
    activeWorkspaceFreshness,
    activeWorkspaceFingerprintSpend,
    focusedWorkspaceReviewServiceTier,
    normalizedActiveRuntime,
    publishShortcutLabel,
    terminalArchivedReason,
    terminalUnavailableReason,
    workspaceHydrationError: conversationWorkspaceQuery.error,
    workspaceHydrationStatus,
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
