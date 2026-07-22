import type { QueryClient } from "@tanstack/react-query";

import type { ComposerIntegrationReference } from "@/api/chat";
import { ticketingKeys } from "@/hooks/useTicketing";

export function hasClickUpIntegrationReference(
  references: readonly ComposerIntegrationReference[] | null | undefined,
): boolean {
  return Boolean(
    references?.some(
      (reference) =>
        reference.provider === "clickup" && reference.kind === "clickup",
    ),
  );
}

export function invalidateAgentConversationClickUpTicket(
  queryClient: QueryClient,
  conversationId: string,
) {
  return queryClient.invalidateQueries({
    queryKey: ticketingKeys.conversationTicket(conversationId),
  });
}
