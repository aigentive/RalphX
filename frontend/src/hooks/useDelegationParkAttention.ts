/**
 * useDelegationParkAttention - Global alert for coordinators that will never be resumed.
 *
 * A parked coordinator normally wakes on its own once its delegates settle. When wake delivery
 * fails the backend settles the park as failed and emits `delegation_park:needs_attention`.
 * Without this hook that outcome is silent: the row simply leaves the parked lane and the
 * conversation sits terminal with no explanation of why the resume never arrived.
 */

import { useEffect, useRef } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";
import { useEventBus } from "@/providers/EventProvider";
import { invalidateAgentSidebarConversations } from "@/hooks/agentSidebarConversationKeys";
import { DELEGATION_PARK_NEEDS_ATTENTION } from "@/lib/events";
import type { DelegationParkAttentionPayload } from "@/types/events";
import type { Unsubscribe } from "@/lib/event-bus";

const TOAST_DURATION_MS = 30_000;

function coordinatorLabel(payload: DelegationParkAttentionPayload): string {
  const title = payload.conversation_title?.trim();
  return title ? `“${title}”` : payload.parent_conversation_id;
}

function delegateSummary(delegateCount: number | null | undefined): string {
  const count = typeof delegateCount === "number" && delegateCount > 0 ? delegateCount : 0;
  return count === 1 ? "1 delegate settled" : `${count} delegates settled`;
}

export function useDelegationParkAttention() {
  const bus = useEventBus();
  const queryClient = useQueryClient();
  // Park ids already alerted on — the backend can emit once per failed dispatcher attempt.
  const alertedRef = useRef(new Set<string>());

  useEffect(() => {
    const unsubscribes: Unsubscribe[] = [];

    unsubscribes.push(
      bus.subscribe<DelegationParkAttentionPayload>(
        DELEGATION_PARK_NEEDS_ATTENTION,
        (payload) => {
          const parkId = payload?.park_id;
          if (!parkId || alertedRef.current.has(parkId)) return;
          alertedRef.current.add(parkId);

          // The row is leaving the parked working lane; refresh so it stops advertising
          // delegates it is no longer waiting on.
          void invalidateAgentSidebarConversations(queryClient);

          toast.error(
            `Delegates finished but ${coordinatorLabel(payload)} could not be resumed`,
            {
              description: `${delegateSummary(payload.delegate_count)}. Send a message in that conversation to continue. (${payload.error})`,
              duration: TOAST_DURATION_MS,
            }
          );
        }
      )
    );

    return () => {
      unsubscribes.forEach((unsub) => unsub());
    };
  }, [bus, queryClient]);
}
