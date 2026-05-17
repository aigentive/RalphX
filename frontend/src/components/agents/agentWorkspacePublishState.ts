import type {
  AgentConversationWorkspace,
  AgentConversationWorkspaceFreshness,
} from "@/api/chat";

export function hasPublishedWorkspacePr(
  workspace: AgentConversationWorkspace | null
): boolean {
  return Boolean(workspace?.publicationPrNumber ?? workspace?.publicationPrUrl);
}

export function isAgentWorkspacePublishCurrent(
  workspace: AgentConversationWorkspace | null,
  freshness: AgentConversationWorkspaceFreshness | undefined
): boolean {
  const freshnessScope = freshness?.freshnessScope ?? "full";
  const remoteRefreshed = freshness?.remoteRefreshed ?? true;
  const worktreeStatusChecked = freshness?.worktreeStatusChecked ?? true;
  return (
    hasPublishedWorkspacePr(workspace) &&
    workspace?.publicationPushStatus === "pushed" &&
    freshness !== undefined &&
    !freshness.isBaseAhead &&
    !freshness.hasUncommittedChanges &&
    freshness.unpublishedCommitCount === 0
  );
}
