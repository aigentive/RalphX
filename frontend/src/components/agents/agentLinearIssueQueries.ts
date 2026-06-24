import type { QueryClient } from "@tanstack/react-query";

import type { ComposerIntegrationReference } from "@/api/chat";

export const agentLinearIssueKeys = {
  all: ["agents", "linear-issue"] as const,
  issue: (conversationId: string | null) =>
    [...agentLinearIssueKeys.all, conversationId ?? "none"] as const,
};

export function hasLinearIntegrationReference(
  references: readonly ComposerIntegrationReference[] | null | undefined,
): boolean {
  return references?.some((reference) => reference.kind === "linear") ?? false;
}

export function invalidateAgentConversationLinearIssue(
  queryClient: QueryClient,
  conversationId: string,
): Promise<void> {
  return queryClient.invalidateQueries({
    queryKey: agentLinearIssueKeys.issue(conversationId),
  });
}
