import { useEffect } from "react";
import { type QueryClient, type QueryKey, useQueryClient } from "@tanstack/react-query";

import { useEventBus } from "@/providers/EventProvider";
import type { TicketingProvider } from "@/api/ticketing";

import { ticketingKeys } from "./useTicketing";

export const TICKETING_CACHE_INVALIDATED_EVENT = "ticketing:cache_invalidated";

export interface TicketingCacheInvalidatedPayload {
  provider: TicketingProvider;
  ticketId?: string | null | undefined;
  ticketKey?: string | null | undefined;
  containerId?: string | null | undefined;
  projectId?: string | null | undefined;
  reason?: string | undefined;
  invalidatedAt?: string | undefined;
}

function isLinearTicketDetailFamilyKey(queryKey: QueryKey, ticketId: string): boolean {
  return queryKey.length >= 5
    && queryKey[0] === ticketingKeys.all[0]
    && queryKey[1] === "detail"
    && queryKey[2] === "linear"
    && queryKey[3] === ticketId;
}

export function invalidateTicketingQueriesForCacheEvent(
  queryClient: QueryClient,
  payload: TicketingCacheInvalidatedPayload,
) {
  if (payload.provider !== "linear") {
    return;
  }

  void queryClient.invalidateQueries({
    queryKey: [...ticketingKeys.all, "tickets", "linear"],
  });
  void queryClient.invalidateQueries({
    queryKey: [...ticketingKeys.all, "containers", "linear"],
  });

  const ticketId = payload.ticketId;
  if (!ticketId) {
    return;
  }

  const ticketInput = payload.ticketKey
    ? {
        provider: "linear" as const,
        ticketRef: {
          provider: "linear" as const,
          id: ticketId,
          key: payload.ticketKey,
        },
      }
    : null;
  if (ticketInput) {
    void queryClient.invalidateQueries({
      queryKey: ticketingKeys.detail(ticketInput),
    });
    void queryClient.invalidateQueries({
      queryKey: ticketingKeys.transitions(ticketInput),
    });
  }
  void queryClient.invalidateQueries({
    predicate: (query) => isLinearTicketDetailFamilyKey(query.queryKey, ticketId),
  });
}

export function useTicketingCacheEvents() {
  const bus = useEventBus();
  const queryClient = useQueryClient();

  useEffect(() => {
    return bus.subscribe<TicketingCacheInvalidatedPayload>(
      TICKETING_CACHE_INVALIDATED_EVENT,
      (payload) => {
        invalidateTicketingQueriesForCacheEvent(queryClient, payload);
      },
    );
  }, [bus, queryClient]);
}
