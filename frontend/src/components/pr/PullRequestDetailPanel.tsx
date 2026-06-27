import type { AgentConversationWorkspace } from "@/api/chat";
import { EmptyArtifactState } from "@/components/agents/AgentsArtifactEmptyState";

import { PullRequestDetailBody } from "./PullRequestDetailBody";
import {
  pullRequestSelectorFromShell,
  pullRequestShellFromWorkspace,
} from "./PullRequestDetailShell";

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
