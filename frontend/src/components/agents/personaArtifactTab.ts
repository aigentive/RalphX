import type { ChatConversation } from "@/types/chat-conversation";

export function isPersonaArtifactConversation(
  conversation: Pick<ChatConversation, "agentMode"> | null | undefined,
): boolean {
  return conversation?.agentMode === "persona_builder";
}
