import type { IdeationSessionResponse } from "@/api/ideation";
import { buildAgentEventStoreKey } from "@/lib/agent-store-key";
import { formatHumanTimestampTitle } from "@/lib/formatters";
import type { ChatConversation } from "@/types/chat-conversation";

export type AgentIdeationSession = Pick<
  IdeationSessionResponse,
  "id" | "projectId" | "title" | "status" | "updatedAt" | "archivedAt"
>;

export type AgentConversation = ChatConversation & {
  projectId: string;
  ideationSessionId: string | null;
};

export function toProjectAgentConversation(
  conversation: ChatConversation
): AgentConversation {
  return {
    ...conversation,
    projectId: conversation.contextId,
    ideationSessionId: null,
  };
}

export function toIdeationAgentConversation(
  session: AgentIdeationSession,
  conversation: ChatConversation
): AgentConversation {
  return {
    ...conversation,
    contextType: "ideation",
    contextId: session.id,
    projectId: session.projectId,
    ideationSessionId: session.id,
    title: session.title ?? conversation.title,
    updatedAt: newestTimestamp(conversation.updatedAt, session.updatedAt) ?? conversation.updatedAt,
    archivedAt:
      session.archivedAt ??
      conversation.archivedAt ??
      (session.status === "archived" ? session.updatedAt : null),
  };
}

export function sortAgentConversations(
  conversations: AgentConversation[]
): AgentConversation[] {
  return [...conversations].sort((a, b) => {
    return new Date(b.createdAt).getTime() - new Date(a.createdAt).getTime();
  });
}

export function getAgentConversationStoreKey(
  conversation: Pick<AgentConversation, "contextType" | "contextId" | "id">
): string {
  return buildAgentEventStoreKey(
    conversation.contextType,
    conversation.contextId,
    conversation.id
  );
}

export function formatAgentConversationCreatedAt(
  input: string | number | Date
): string {
  return formatHumanTimestampLabel(input);
}

export function formatAgentConversationCreatedAtTitle(
  input: string | number | Date
): string {
  return formatHumanTimestampTitle(input);
}

function newestTimestamp(
  left: string | null | undefined,
  right: string | null | undefined
): string | null {
  if (!left) return right ?? null;
  if (!right) return left;
  return new Date(right).getTime() > new Date(left).getTime() ? right : left;
}
