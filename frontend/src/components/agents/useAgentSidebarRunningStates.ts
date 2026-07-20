import { useEffect, useMemo } from "react";

import { chatApi } from "@/api/chat";

import {
  getAgentConversationStoreKey,
  type AgentConversation,
} from "./agentConversations";
import { reconcileAgentConversationRuntimeStatus } from "./agentConversationRuntimeStore";

const AGENT_SIDEBAR_LIVENESS_POLL_MS = 5_000;

export function useAgentSidebarRunningStates(
  conversations: AgentConversation[],
  isVisible: boolean
) {
  const agentConversations = useMemo(() => {
    const seen = new Set<string>();
    const targets: AgentConversation[] = [];

    for (const conversation of conversations) {
      if (
        (conversation.contextType !== "project" &&
          conversation.contextType !== "standalone") ||
        seen.has(conversation.id)
      ) {
        continue;
      }
      seen.add(conversation.id);
      targets.push(conversation);
    }

    return targets;
  }, [conversations]);

  useEffect(() => {
    if (!isVisible || agentConversations.length === 0) {
      return undefined;
    }

    let cancelled = false;
    let inFlight = false;

    const reconcile = () => {
      if (inFlight) return;
      inFlight = true;

      const contextIds = agentConversations.map((conversation) => conversation.id);
      chatApi
        .getAgentConversationRuntimeStatuses(contextIds)
        .then((runtimeStatuses) => {
          if (cancelled) return;

          for (const conversation of agentConversations) {
            reconcileAgentConversationRuntimeStatus(
              conversation.id,
              runtimeStatuses[conversation.id],
              { storeKey: getAgentConversationStoreKey(conversation) }
            );
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
  }, [agentConversations, isVisible]);
}
