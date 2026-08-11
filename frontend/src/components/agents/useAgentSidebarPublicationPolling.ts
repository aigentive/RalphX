import { useEffect, useMemo, useRef } from "react";
import { useQueryClient } from "@tanstack/react-query";

import { chatApi } from "@/api/chat";

import type { AgentConversation } from "./agentConversations";
import { invalidateWorkspaceQueries } from "./agentWorkspaceQueries";
import { agentSidebarConversationKeys } from "./useAgentSidebarPublicationGroup";

const PUBLICATION_POLL_MS = 5_000;

export function workspacePublicationFingerprint(
  state: string,
  label: string | null | undefined,
): string {
  return `${state}\u0000${label?.trim().toLowerCase() ?? ""}`;
}

export function useAgentSidebarPublicationPolling(
  conversations: AgentConversation[],
  isVisible: boolean,
  currentFingerprints: Map<string, string>,
) {
  const queryClient = useQueryClient();
  const currentStatesRef = useRef(currentFingerprints);
  useEffect(() => {
    currentStatesRef.current = currentFingerprints;
  }, [currentFingerprints]);

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

          const changedConversationIds: string[] = [];
          const cached = currentStatesRef.current;
          for (const [id, entry] of Object.entries(states)) {
            const cachedState = cached.get(id);
            const nextFingerprint = workspacePublicationFingerprint(
              entry.publication_state,
              entry.publication_label,
            );
            if (cachedState !== undefined && cachedState !== nextFingerprint) {
              changedConversationIds.push(id);
            }
          }

          if (changedConversationIds.length > 0) {
            queryClient.invalidateQueries({
              queryKey: agentSidebarConversationKeys.all,
            });
            for (const conversationId of changedConversationIds) {
              void invalidateWorkspaceQueries(queryClient, conversationId);
            }
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
