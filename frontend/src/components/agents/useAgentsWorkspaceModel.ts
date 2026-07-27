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
  focusedAutomationRunConversationId?: string | null;
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
  focusedAutomationRunConversationId = null,
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
  const activeWorkspaceFreshness = activeWorkspaceFreshnessQuery.data;
  // While an automation run is focused, the publish shortcut must describe the
  // run conversation's own workspace, never the automation setup workspace.
  const focusedRunWorkspaceQuery = useQuery({
    queryKey: agentWorkspaceKeys.workspace(focusedAutomationRunConversationId),
    queryFn: () =>
      chatApi.getAgentConversationWorkspace(focusedAutomationRunConversationId!),
    enabled: !!focusedAutomationRunConversationId,
    staleTime: AGENT_WORKSPACE_STALE_MS,
  });
  const focusedRunWorkspace = focusedAutomationRunConversationId
    ? focusedRunWorkspaceQuery.data ?? null
    : null;
  const canInspectFocusedRunFreshness =
    canInspectAgentWorkspaceFreshness(focusedRunWorkspace);
  const focusedRunFreshnessQuery = useQuery({
    queryKey: agentWorkspaceKeys.scopedFreshness(
      focusedAutomationRunConversationId,
      "local",
    ),
    queryFn: () =>
      chatApi.getAgentConversationWorkspaceFreshness(
        focusedAutomationRunConversationId!,
        { scope: "local" },
      ),
    enabled:
      !!focusedAutomationRunConversationId && canInspectFocusedRunFreshness,
    staleTime: AGENT_WORKSPACE_FRESHNESS_STALE_MS,
  });
  const publishShortcutSourceWorkspace = focusedAutomationRunConversationId
    ? focusedRunWorkspace
    : activeWorkspace;
  const publishShortcutFreshness = focusedAutomationRunConversationId
    ? focusedRunFreshnessQuery.data
    : activeWorkspaceFreshness;
  const terminalPublicationLabel = getAgentWorkspaceTerminalPublicationLabel(
    publishShortcutSourceWorkspace,
  );
  const workspacePublishMode = getProjectWorkspacePublishMode(
    activeProject,
    hasPublishedWorkspacePr(publishShortcutSourceWorkspace),
  );
  const isPublishShortcutCurrent = isAgentWorkspacePublishCurrent(
    publishShortcutSourceWorkspace,
    publishShortcutFreshness,
  );
  const publishShortcutLabel = terminalPublicationLabel
    ? terminalPublicationLabel
    : workspacePublishMode.kind === "unavailable"
      ? "Repository setup required"
    : publishShortcutFreshness?.baseStatus === "blocked"
      ? "Base unavailable"
      : publishShortcutFreshness?.isBaseAhead
        ? `Update from ${
            publishShortcutFreshness.effectiveBaseRef ??
            publishShortcutFreshness.baseRef ??
            publishShortcutSourceWorkspace?.baseRef ??
            getAgentWorkspaceEffectiveBaseLabel(
              publishShortcutSourceWorkspace,
              publishShortcutFreshness,
            )
          }`
        : isPublishShortcutCurrent
          ? "Published"
          : workspacePublishMode.kind === "localCommit"
            ? "Commit locally"
            : "Commit & Publish";
  // Fail closed: without a resolved run workspace the shortcut is suppressed
  // instead of silently falling back to the automation setup workspace.
  const suppressPublishShortcut =
    !!focusedAutomationRunConversationId && !focusedRunWorkspace;
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
    publishShortcutWorkspace: focusedRunWorkspace,
    suppressPublishShortcut,
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
