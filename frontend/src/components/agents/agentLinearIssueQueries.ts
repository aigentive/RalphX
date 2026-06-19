export const agentLinearIssueKeys = {
  all: ["agents", "linear-issue"] as const,
  issue: (conversationId: string | null) =>
    [...agentLinearIssueKeys.all, conversationId ?? "none"] as const,
};
