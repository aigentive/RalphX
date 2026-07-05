import type { AgentConversationRuntimeStatus } from "@/api/chat";
import { buildStoreKey } from "@/lib/chat-context-registry";
import { type AgentStatus, useChatStore } from "@/stores/chatStore";

export type AgentConversationRuntimeStatusMirrorOption =
  | boolean
  | ((
      status: AgentConversationRuntimeStatus | null | undefined,
      context: { conversationId: string; storeKey: string },
    ) => boolean);

export type AgentConversationRuntimeStatusMirrorSelector = (
  status: AgentConversationRuntimeStatus | null | undefined,
  context: { conversationId: string; storeKey: string },
) => AgentConversationRuntimeStatus | null | undefined;

interface ReconcileAgentConversationRuntimeStatusOptions {
  storeKey?: string | null | undefined;
  mirrorToVisibleChatStatus?: AgentConversationRuntimeStatusMirrorOption;
  selectVisibleChatStatus?: AgentConversationRuntimeStatusMirrorSelector;
}

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

function resolveVisibleChatStatusMirrorDecision(
  status: AgentConversationRuntimeStatus | null | undefined,
  options: ReconcileAgentConversationRuntimeStatusOptions,
  context: { conversationId: string; storeKey: string },
): { shouldMirror: boolean; shouldClearSuppressedStatus: boolean } {
  const mirrorOption = options.mirrorToVisibleChatStatus;
  if (typeof mirrorOption === "function") {
    const shouldMirror = mirrorOption(status, context);
    return {
      shouldMirror,
      shouldClearSuppressedStatus: !shouldMirror,
    };
  }
  if (mirrorOption === false) {
    return {
      shouldMirror: false,
      shouldClearSuppressedStatus: false,
    };
  }
  return {
    shouldMirror: true,
    shouldClearSuppressedStatus: false,
  };
}

function selectVisibleChatStatus(
  status: AgentConversationRuntimeStatus | null | undefined,
  options: ReconcileAgentConversationRuntimeStatusOptions,
  context: { conversationId: string; storeKey: string },
): AgentConversationRuntimeStatus | null | undefined {
  return options.selectVisibleChatStatus
    ? options.selectVisibleChatStatus(status, context)
    : status;
}

export function reconcileAgentConversationRuntimeStatus(
  conversationId: string,
  status: AgentConversationRuntimeStatus | null | undefined,
  options: ReconcileAgentConversationRuntimeStatusOptions = {},
) {
  const storeKey = options.storeKey ?? buildStoreKey("project", conversationId);
  const chatState = useChatStore.getState();
  const mirrorDecision = resolveVisibleChatStatusMirrorDecision(
    status,
    options,
    {
      conversationId,
      storeKey,
    },
  );
  if (!mirrorDecision.shouldMirror) {
    if (
      mirrorDecision.shouldClearSuppressedStatus &&
      !chatState.isSending[storeKey]
    ) {
      chatState.setAgentStatus(storeKey, "idle");
    }
    return;
  }

  const visibleChatStatus = selectVisibleChatStatus(status, options, {
    conversationId,
    storeKey,
  });
  const { isRunning, agentStatus } =
    normalizeAgentConversationRuntimeStatus(visibleChatStatus);
  const activityLabel = agentConversationRuntimeActivityLabel(visibleChatStatus);
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
