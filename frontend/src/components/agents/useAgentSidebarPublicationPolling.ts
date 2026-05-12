import { useEffect, useMemo, useRef } from "react";
import { useQueryClient } from "@tanstack/react-query";

import { chatApi } from "@/api/chat";

import type { AgentConversation } from "./agentConversations";
import { agentSidebarConversationKeys } from "./useAgentSidebarPublicationGroup";

const PUBLICATION_POLL_MS = 5_000;

export function useAgentSidebarPublicationPolling(
  conversations: AgentConversation[],
  isVisible: boolean,
  currentStates: Map<string, string>
) {
  const queryClient = useQueryClient();
  const currentStatesRef = useRef(currentStates);
  useEffect(() => {
    currentStatesRef.current = currentStates;
  }, [currentStates]);

  const conversationIds = useMemo(() => {
    const seen = new Set<string>();
    const ids: string[] = [];
    for (const c of conversations) {
      if (c.contextType !== "project" || seen.has(c.id)) continue;
      seen.add(c.id);
      ids.push(c.id);
    }
    return ids;
  }, [conversations]);

  useEffect(() => {
    if (!isVisible || conversationIds.length === 0) return undefined;

    let cancelled = false;
    let inFlight = false;

    const poll = () => {
      if (inFlight) return;
      inFlight = true;

      chatApi
        .getBulkWorkspacePublicationStates(conversationIds)
        .then((states) => {
          if (cancelled) return;

          let changed = false;
          const cached = currentStatesRef.current;
          for (const [id, entry] of Object.entries(states)) {
            const cachedState = cached.get(id);
            if (cachedState !== undefined && cachedState !== entry.publication_state) {
              changed = true;
              break;
            }
          }

          if (changed) {
            queryClient.invalidateQueries({
              queryKey: agentSidebarConversationKeys.all,
            });
          }
        })
        .catch(() => {})
        .finally(() => {
          inFlight = false;
        });
    };

    poll();
    const intervalId = window.setInterval(poll, PUBLICATION_POLL_MS);
    return () => {
      cancelled = true;
      window.clearInterval(intervalId);
    };
  }, [isVisible, conversationIds, queryClient]);
}
