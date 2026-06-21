import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { renderHook } from "@testing-library/react";
import { createElement, type ReactNode } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { EventBus, Unsubscribe } from "@/lib/event-bus";

import { ticketingKeys } from "./useTicketing";
import {
  invalidateTicketingQueriesForCacheEvent,
  TICKETING_CACHE_INVALIDATED_EVENT,
  useTicketingCacheEvents,
  type TicketingCacheInvalidatedPayload,
} from "./useTicketingEvents";

const subscribeMock = vi.fn();
const unsubscribeMock = vi.fn<Unsubscribe>();

vi.mock("@/providers/EventProvider", () => ({
  useEventBus: (): EventBus => ({
    subscribe: subscribeMock,
    emit: vi.fn(),
  }),
}));

describe("ticketing cache events", () => {
  it("invalidates Linear ticket list and detail keys from webhook events", () => {
    const queryClient = new QueryClient();
    const invalidateSpy = vi.spyOn(queryClient, "invalidateQueries");

    invalidateTicketingQueriesForCacheEvent(queryClient, {
      provider: "linear",
      ticketId: "issue-1",
      ticketKey: "LIN-1",
      reason: "recorded_issue_activity",
    });

    expect(invalidateSpy).toHaveBeenCalledWith({
      queryKey: [...ticketingKeys.all, "tickets", "linear"],
    });
    expect(invalidateSpy).toHaveBeenCalledWith({
      queryKey: [...ticketingKeys.all, "containers", "linear"],
    });
    expect(invalidateSpy).toHaveBeenCalledWith({
      queryKey: ticketingKeys.detail({
        provider: "linear",
        ticketRef: { provider: "linear", id: "issue-1", key: "LIN-1" },
      }),
    });
    expect(invalidateSpy).toHaveBeenCalledWith({
      queryKey: ticketingKeys.transitions({
        provider: "linear",
        ticketRef: { provider: "linear", id: "issue-1", key: "LIN-1" },
      }),
    });
  });

  it("invalidates cached Linear detail and transition keys when activity only includes issue id", () => {
    const queryClient = new QueryClient();
    const keyedInput = {
      provider: "linear" as const,
      ticketRef: { provider: "linear" as const, id: "issue-1", key: "LIN-1" },
    };
    const detailKey = ticketingKeys.detail(keyedInput);
    const transitionsKey = ticketingKeys.transitions(keyedInput);
    queryClient.setQueryData(detailKey, { title: "Cached ticket" });
    queryClient.setQueryData(transitionsKey, [{ name: "Started" }]);

    invalidateTicketingQueriesForCacheEvent(queryClient, {
      provider: "linear",
      ticketId: "issue-1",
      reason: "recorded_issue_activity",
    });

    expect(queryClient.getQueryState(detailKey)?.isInvalidated).toBe(true);
    expect(queryClient.getQueryState(transitionsKey)?.isInvalidated).toBe(true);
  });

  it("does not invalidate Jira caches for manual-refresh providers", () => {
    const queryClient = new QueryClient();
    const invalidateSpy = vi.spyOn(queryClient, "invalidateQueries");

    invalidateTicketingQueriesForCacheEvent(queryClient, {
      provider: "jira",
      ticketId: "10001",
      ticketKey: "JRA-1",
    });

    expect(invalidateSpy).not.toHaveBeenCalled();
  });

  it("invalidates Linear list and container keys but skips detail keys without a ticket id", () => {
    const queryClient = new QueryClient();
    const invalidateSpy = vi.spyOn(queryClient, "invalidateQueries");

    invalidateTicketingQueriesForCacheEvent(queryClient, {
      provider: "linear",
      reason: "recorded_issue_activity",
    });

    expect(invalidateSpy).toHaveBeenCalledWith({
      queryKey: [...ticketingKeys.all, "tickets", "linear"],
    });
    expect(invalidateSpy).toHaveBeenCalledWith({
      queryKey: [...ticketingKeys.all, "containers", "linear"],
    });
    // No ticket id means no detail/transition invalidation runs.
    expect(invalidateSpy).toHaveBeenCalledTimes(2);
  });

  it("invalidates detail-family caches by predicate even when the ticket key is absent", () => {
    const queryClient = new QueryClient();
    const familyKey = [...ticketingKeys.all, "detail", "linear", "issue-1", "extra"];
    queryClient.setQueryData(familyKey, { title: "Cached family member" });

    invalidateTicketingQueriesForCacheEvent(queryClient, {
      provider: "linear",
      ticketId: "issue-1",
      reason: "recorded_issue_activity",
    });

    expect(queryClient.getQueryState(familyKey)?.isInvalidated).toBe(true);
  });
});

describe("useTicketingCacheEvents", () => {
  function wrapper({ children }: { children: ReactNode }) {
    const queryClient = new QueryClient();
    return createElement(QueryClientProvider, { client: queryClient }, children);
  }

  beforeEach(() => {
    subscribeMock.mockReset();
    unsubscribeMock.mockReset();
    subscribeMock.mockReturnValue(unsubscribeMock);
  });

  afterEach(() => {
    vi.clearAllMocks();
  });

  it("subscribes to the ticketing cache invalidation event on mount", () => {
    renderHook(() => useTicketingCacheEvents(), { wrapper });

    expect(subscribeMock).toHaveBeenCalledTimes(1);
    expect(subscribeMock).toHaveBeenCalledWith(
      TICKETING_CACHE_INVALIDATED_EVENT,
      expect.any(Function),
    );
  });

  it("unsubscribes from the event bus on unmount", () => {
    const { unmount } = renderHook(() => useTicketingCacheEvents(), { wrapper });
    expect(unsubscribeMock).not.toHaveBeenCalled();
    unmount();
    expect(unsubscribeMock).toHaveBeenCalledTimes(1);
  });

  it("routes received payloads through the Linear-only invalidation gate", () => {
    const handlers: Array<(payload: TicketingCacheInvalidatedPayload) => void> = [];
    subscribeMock.mockImplementation(
      (_event: string, handler: (payload: TicketingCacheInvalidatedPayload) => void) => {
        handlers.push(handler);
        return unsubscribeMock;
      },
    );

    const queryClient = new QueryClient();
    const invalidateSpy = vi.spyOn(queryClient, "invalidateQueries");
    function clientWrapper({ children }: { children: ReactNode }) {
      return createElement(QueryClientProvider, { client: queryClient }, children);
    }

    renderHook(() => useTicketingCacheEvents(), { wrapper: clientWrapper });
    expect(handlers).toHaveLength(1);

    // A Jira event is gated out and must not invalidate anything.
    handlers[0]?.({ provider: "jira", ticketId: "10001", ticketKey: "JRA-1" });
    expect(invalidateSpy).not.toHaveBeenCalled();

    // A Linear event flows through to invalidation.
    handlers[0]?.({ provider: "linear", ticketId: "issue-1", ticketKey: "LIN-1" });
    expect(invalidateSpy).toHaveBeenCalledWith({
      queryKey: [...ticketingKeys.all, "tickets", "linear"],
    });
  });
});
