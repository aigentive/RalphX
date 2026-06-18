import type { QueryClient } from "@tanstack/react-query";

import type { ComposerIntegrationReference } from "@/api/chat";

export const agentJiraIssueKeys = {
  all: ["agents", "jira-issue"] as const,
  issue: (conversationId: string | null) =>
    [...agentJiraIssueKeys.all, conversationId] as const,
};

export function hasJiraIntegrationReference(
  references: readonly ComposerIntegrationReference[] | null | undefined,
): boolean {
  return references?.some((reference) => reference.kind === "jira") ?? false;
}

export function invalidateAgentConversationJiraIssue(
  queryClient: QueryClient,
  conversationId: string,
): Promise<void> {
  return queryClient.invalidateQueries({
    queryKey: agentJiraIssueKeys.issue(conversationId),
  });
}
