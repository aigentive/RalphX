import type { QueryClient } from "@tanstack/react-query";
import type { ComposerIntegrationReference } from "@/api/chat";

export const agentGranolaNoteKeys = {
  all: ["agent-conversation-granola-note"] as const,
  note: (conversationId: string | null | undefined) =>
    [...agentGranolaNoteKeys.all, conversationId ?? "none"] as const,
};

export function hasGranolaIntegrationReference(
  references: readonly ComposerIntegrationReference[] | null | undefined,
): boolean {
  return (
    references?.some(
      (reference) =>
        reference.provider === "granola" && reference.kind === "note",
    ) ?? false
  );
}

export function invalidateAgentConversationGranolaNote(
  queryClient: QueryClient,
  conversationId: string | null | undefined,
) {
  return queryClient.invalidateQueries({
    queryKey: agentGranolaNoteKeys.note(conversationId),
  });
}
