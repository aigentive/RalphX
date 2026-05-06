import type {
  AgentConversationWorkspace,
  AgentConversationWorkspaceFreshness,
} from "@/api/chat";

export function hasPublishedWorkspacePr(
  workspace: AgentConversationWorkspace | null
): boolean {
  return Boolean(workspace?.publicationPrNumber ?? workspace?.publicationPrUrl);
}

function normalizePublicationStatus(status: string | null | undefined): string | null {
  const normalized = status?.trim().toLowerCase();
  return normalized || null;
}

export function getAgentWorkspaceTerminalPublicationStatus(
  workspace: AgentConversationWorkspace | null
): "merged" | "closed" | null {
  const status = normalizePublicationStatus(workspace?.publicationPrStatus);
  if (status === "merged") {
    return "merged";
  }
  if (status === "closed") {
    return "closed";
  }
  return null;
}

export function getAgentWorkspaceTerminalPublicationLabel(
  workspace: AgentConversationWorkspace | null
): string | null {
  const status = getAgentWorkspaceTerminalPublicationStatus(workspace);
  if (status === "merged") {
    return "Merged";
  }
  if (status === "closed") {
    return "Closed";
  }
  return null;
}

export function isPipelineOwnedAgentWorkspace(
  workspace: AgentConversationWorkspace | null | undefined
): boolean {
  return Boolean(workspace?.linkedPlanBranchId);
}

export function shouldShowAgentWorkspacePublishSurface(
  workspace: AgentConversationWorkspace | null | undefined
): boolean {
  if (!workspace) {
    return false;
  }

  if (workspace.mode === "edit") {
    return !workspace.linkedIdeationSessionId && !workspace.linkedPlanBranchId;
  }

  if (workspace.mode === "ideation") {
    return isPipelineOwnedAgentWorkspace(workspace);
  }

  return false;
}

export function isAgentWorkspacePublishCurrent(
  workspace: AgentConversationWorkspace | null,
  freshness: AgentConversationWorkspaceFreshness | undefined
): boolean {
  return (
    hasPublishedWorkspacePr(workspace) &&
    workspace?.publicationPushStatus === "pushed" &&
    freshness !== undefined &&
    freshness.baseStatus !== "blocked" &&
    !freshness.isBaseAhead &&
    !freshness.hasUncommittedChanges &&
    freshness.unpublishedCommitCount === 0
  );
}

export function getAgentWorkspaceEffectiveBaseLabel(
  workspace: AgentConversationWorkspace | null,
  freshness: AgentConversationWorkspaceFreshness | undefined
): string {
  if (freshness?.baseStatus === "blocked") {
    return "Base unavailable";
  }
  return (
    freshness?.effectiveBaseDisplayName ??
    freshness?.baseDisplayName ??
    freshness?.effectiveBaseRef ??
    freshness?.baseRef ??
    workspace?.baseDisplayName ??
    workspace?.baseRef ??
    "Base branch"
  );
}
