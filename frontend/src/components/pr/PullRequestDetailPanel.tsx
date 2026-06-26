import type { AgentConversationWorkspace } from "@/api/chat";
import type { PullRequestDetailSelector } from "@/hooks/usePullRequestDetail";
import { EmptyArtifactState } from "@/components/agents/AgentsArtifactEmptyState";

import {
  hasPullRequestShell,
  PullRequestDetailBody,
  type PullRequestShell,
} from "./PullRequestDetailBody";

export function pullRequestShellFromWorkspace(
  workspace: AgentConversationWorkspace | null | undefined,
): PullRequestShell | null {
  if (!workspace) {
    return null;
  }
  if (workspace.publicationPrNumber != null) {
    return {
      projectId: workspace.projectId,
      prNumber: workspace.publicationPrNumber,
      url: workspace.publicationPrUrl,
      status: workspace.publicationPrStatus,
      title: `PR #${workspace.publicationPrNumber}`,
      branch: workspace.branchName,
      conversationId: workspace.conversationId,
    };
  }
  if (workspace.sourcePullRequest) {
    return {
      projectId: workspace.projectId,
      prNumber: workspace.sourcePullRequest.number,
      url: workspace.sourcePullRequest.url,
      title: workspace.sourcePullRequest.title ?? `PR #${workspace.sourcePullRequest.number}`,
      branch: workspace.sourcePullRequest.headRefName,
      conversationId: workspace.conversationId,
    };
  }
  return null;
}

export function pullRequestSelectorFromShell(
  shell: PullRequestShell | null,
): PullRequestDetailSelector | null {
  if (!hasPullRequestShell(shell)) {
    return null;
  }
  return {
    projectId: shell.projectId,
    ...(shell.prNumber != null ? { prNumber: shell.prNumber } : {}),
    ...(shell.prNumber == null && shell.branch ? { branch: shell.branch } : {}),
  };
}

export function PullRequestDetailPanel({
  workspace,
}: {
  workspace: AgentConversationWorkspace | null;
}) {
  const shell = pullRequestShellFromWorkspace(workspace);
  const selector = pullRequestSelectorFromShell(shell);

  if (!shell || !selector) {
    return <EmptyArtifactState title="No pull request" />;
  }

  return (
    <PullRequestDetailBody
      selector={selector}
      shell={shell}
      className="min-h-full"
    />
  );
}
