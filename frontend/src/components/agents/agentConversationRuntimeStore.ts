import type { AgentConversationRuntimeStatus } from "@/api/chat";
import { buildStoreKey } from "@/lib/chat-context-registry";
import { type AgentStatus, useChatStore } from "@/stores/chatStore";

export function normalizeAgentConversationRuntimeStatus(
  status: AgentConversationRuntimeStatus | null | undefined,
): {
  isRunning: boolean;
  agentStatus: AgentStatus;
} {
  const isRunning = status?.isRunning ?? false;
  const agentStatus = status?.agentStatus ?? "generating";
  return {
    isRunning,
    agentStatus: isRunning
      ? agentStatus === "idle"
        ? "generating"
        : agentStatus
      : "idle",
  };
}

export function agentConversationRuntimeActivityLabel(
  status: AgentConversationRuntimeStatus | null | undefined,
): string | null {
  if (!status?.isRunning) return null;
  if (
    status.primarySource === "workspace_review" ||
    status.items.some((item) => item.source === "workspace_review")
  ) {
    return "reviewing";
  }
  return null;
}

export function reconcileAgentConversationRuntimeStatus(
  conversationId: string,
  status: AgentConversationRuntimeStatus | null | undefined,
  options: { storeKey?: string | null | undefined } = {},
) {
  const storeKey = options.storeKey ?? buildStoreKey("project", conversationId);
  const chatState = useChatStore.getState();
  const { isRunning, agentStatus } =
    normalizeAgentConversationRuntimeStatus(status);
  const activityLabel = agentConversationRuntimeActivityLabel(status);
  const currentStatus = chatState.agentStatus[storeKey] ?? "idle";

  if (isRunning) {
    chatState.setActiveConversation(storeKey, conversationId);
    if (currentStatus !== agentStatus) {
      chatState.setAgentStatus(storeKey, agentStatus);
    }
    chatState.setAgentActivityLabel(storeKey, activityLabel);
    return;
  }

  if (chatState.isSending[storeKey]) {
    return;
  }

  if (currentStatus !== "idle") {
    chatState.setAgentRunning(storeKey, false);
  } else {
    chatState.setAgentActivityLabel(storeKey, null);
  }
}
