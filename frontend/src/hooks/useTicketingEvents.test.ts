import { QueryClient } from "@tanstack/react-query";
import { describe, expect, it, vi } from "vitest";

import { ticketingKeys } from "./useTicketing";
import { invalidateTicketingQueriesForCacheEvent } from "./useTicketingEvents";

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
});
