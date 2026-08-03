import { useEffect, useMemo, useRef } from "react";

import { chatApi } from "@/api/chat";
import { isRemoteEnvironmentActive } from "@/hooks/useActiveEnvironment";
import { useEnvironmentStore } from "@/stores/environmentStore";

import {
  getAgentConversationStoreKey,
  type AgentConversation,
} from "./agentConversations";
import { reconcileAgentConversationRuntimeStatus } from "./agentConversationRuntimeStore";
import { reconcileAgentConversationRuntimeIndexes } from "./agentConversationRuntimeReconcile";

const AGENT_SIDEBAR_LIVENESS_POLL_MS = 5_000;

const knownConversationSets = new Map<symbol, AgentConversation[]>();

export function getKnownAgentSidebarConversations(): AgentConversation[] {
  const seen = new Set<string>();
  return [...knownConversationSets.values()].flatMap((conversations) =>
    conversations.filter((conversation) => {
      if (seen.has(conversation.id)) return false;
      seen.add(conversation.id);
      return true;
    }),
  );
}

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
    if (!isVisible) return undefined;
    const owner = Symbol("agent-sidebar-runtime-conversations");
    knownConversationSets.set(owner, agentConversations);
    return () => {
      knownConversationSets.delete(owner);
    };
  }, [agentConversations, isVisible]);

  const conversationIds = agentConversations
    .map((conversation) => conversation.id)
    .join("\u0000");
  const lastRemoteReconcileIds = useRef<string | null>(null);

  useEffect(() => {
    if (
      !isVisible ||
      agentConversations.length === 0 ||
      !isRemoteEnvironmentActive()
    ) {
      lastRemoteReconcileIds.current = null;
      return;
    }
    if (lastRemoteReconcileIds.current === conversationIds) return;
    lastRemoteReconcileIds.current = conversationIds;

    const environmentId = useEnvironmentStore.getState().activeEnvironmentId;
    void reconcileAgentConversationRuntimeIndexes(
      environmentId,
      agentConversations,
    );
    // `agentConversations` is a dependency for correctness, but the id-set guard above is
    // what decides whether a fan-out actually runs: a re-created array with the same ids
    // returns early, so this never re-fetches on identity churn alone.
  }, [agentConversations, conversationIds, isVisible]);

  useEffect(() => {
    if (!isVisible || agentConversations.length === 0) {
      return undefined;
    }
    if (isRemoteEnvironmentActive()) {
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
