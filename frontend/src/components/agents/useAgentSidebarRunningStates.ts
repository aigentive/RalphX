import { useEffect, useMemo } from "react";

import { chatApi } from "@/api/chat";
import { useChatStore } from "@/stores/chatStore";

import {
  getAgentConversationStoreKey,
  type AgentConversation,
} from "./agentConversations";

const AGENT_SIDEBAR_LIVENESS_POLL_MS = 5_000;

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
        .getAgentRunningStates("project", contextIds)
        .then((runningStates) => {
          if (cancelled) return;

          const chatState = useChatStore.getState();
          for (const conversation of projectConversations) {
            const storeKey = getAgentConversationStoreKey(conversation);
            const isRunning = runningStates[conversation.id] === true;
            const currentStatus = chatState.agentStatus[storeKey] ?? "idle";

            if (isRunning) {
              chatState.setActiveConversation(storeKey, conversation.id);
              if (currentStatus === "idle") {
                chatState.setAgentRunning(storeKey, true);
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
