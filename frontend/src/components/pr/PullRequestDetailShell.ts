import type { AgentConversationWorkspace } from "@/api/chat";
import type {
  GitHubBranchRxConversation,
  GitHubBranchTicketLink,
} from "@/api/github";
import type { PullRequestDetailSelector } from "@/hooks/usePullRequestDetail";

export interface PullRequestShell {
  projectId: string;
  prNumber?: number | null | undefined;
  title?: string | null | undefined;
  url?: string | null | undefined;
  status?: string | null | undefined;
  branch?: string | null | undefined;
  conversationId?: string | null | undefined;
  rxConversations?: GitHubBranchRxConversation[] | undefined;
  ticketLinks?: GitHubBranchTicketLink[] | undefined;
}

export function pullRequestShellFromTicket(input: {
  projectId: string;
  prNumber: number;
  prUrl?: string | null | undefined;
  prStatus?: string | null | undefined;
  title?: string | null | undefined;
}): PullRequestShell {
  return {
    projectId: input.projectId,
    prNumber: input.prNumber,
    url: input.prUrl,
    status: input.prStatus,
    title: input.title ?? `PR #${input.prNumber}`,
  };
}

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
  if (workspace.publicationPrUrl) {
    return {
      projectId: workspace.projectId,
      url: workspace.publicationPrUrl,
      status: workspace.publicationPrStatus,
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

export function hasPullRequestShell(shell: PullRequestShell | null): shell is PullRequestShell {
  return Boolean(shell?.projectId && (shell.prNumber != null || shell.branch));
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
