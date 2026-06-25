import { useEffect, useMemo } from "react";

import { chatApi, type AgentConversationRuntimeStatus } from "@/api/chat";
import { type AgentStatus, useChatStore } from "@/stores/chatStore";

import {
  getAgentConversationStoreKey,
  type AgentConversation,
} from "./agentConversations";

const AGENT_SIDEBAR_LIVENESS_POLL_MS = 5_000;

function normalizeRuntimeStatus(status: AgentConversationRuntimeStatus | undefined): {
  isRunning: boolean;
  agentStatus: AgentStatus;
} {
  const isRunning = status?.isRunning ?? false;
  const agentStatus = status?.agentStatus ?? "generating";
  return {
    isRunning,
    agentStatus: isRunning
      ? (agentStatus === "idle" ? "generating" : agentStatus)
      : "idle",
  };
}

export function useAgentSidebarRunningStates(
  conversations: AgentConversation[],
  isVisible: boolean
) {
  const projectConversations = useMemo(() => {
    const seen = new Set<string>();
    const targets: AgentConversation[] = [];

    for (const conversation of conversations) {
      if (conversation.contextType !== "project" || seen.has(conversation.id)) {
        continue;
      }
      seen.add(conversation.id);
      targets.push(conversation);
    }

    return targets;
  }, [conversations]);

  useEffect(() => {
    if (!isVisible || projectConversations.length === 0) {
      return undefined;
    }

    let cancelled = false;
    let inFlight = false;

    const reconcile = () => {
      if (inFlight) return;
      inFlight = true;

      const contextIds = projectConversations.map((conversation) => conversation.id);
      chatApi
        .getAgentConversationRuntimeStatuses(contextIds)
        .then((runtimeStatuses) => {
          if (cancelled) return;

          const chatState = useChatStore.getState();
          for (const conversation of projectConversations) {
            const storeKey = getAgentConversationStoreKey(conversation);
            const { isRunning, agentStatus } = normalizeRuntimeStatus(
              runtimeStatuses[conversation.id]
            );
            const currentStatus = chatState.agentStatus[storeKey] ?? "idle";

            if (isRunning) {
              chatState.setActiveConversation(storeKey, conversation.id);
              if (currentStatus !== agentStatus) {
                chatState.setAgentStatus(storeKey, agentStatus);
              }
              continue;
            }

            if (currentStatus !== "idle") {
              chatState.setAgentRunning(storeKey, false);
            }
          }
        })
        .catch(() => {
          // Best-effort sidebar reconciliation; lifecycle events remain primary.
        })
        .finally(() => {
          inFlight = false;
        });
    };

    reconcile();
    const intervalId = window.setInterval(
      reconcile,
      AGENT_SIDEBAR_LIVENESS_POLL_MS
    );

    return () => {
      cancelled = true;
      window.clearInterval(intervalId);
    };
  }, [isVisible, projectConversations]);
}
