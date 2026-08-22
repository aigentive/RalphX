import { QueryClient, QueryObserver } from "@tanstack/react-query";
import { describe, expect, it, vi } from "vitest";

import {
  agentSidebarConversationKeys,
  invalidateAgentSidebarConversations,
} from "./agentSidebarConversationKeys";

const SIDEBAR_FILTERS = { queryKey: agentSidebarConversationKeys.all };
const NON_CANCELLING = { cancelRefetch: false };

function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((r) => {
    resolve = r;
  });
  return { promise, resolve };
}

describe("invalidateAgentSidebarConversations", () => {
  it("invalidates once, non-cancelling, when no sidebar listing is in flight", async () => {
    const queryClient = new QueryClient();
    const invalidateSpy = vi
      .spyOn(queryClient, "invalidateQueries")
      .mockResolvedValue(undefined);

    await invalidateAgentSidebarConversations(queryClient);

    expect(invalidateSpy).toHaveBeenCalledTimes(1);
    expect(invalidateSpy).toHaveBeenCalledWith(SIDEBAR_FILTERS, NON_CANCELLING);
  });

  it("adds exactly one trailing pass when a listing was already in flight", async () => {
    const queryClient = new QueryClient();
    vi.spyOn(queryClient, "isFetching").mockReturnValue(1);
    const invalidateSpy = vi
      .spyOn(queryClient, "invalidateQueries")
      .mockResolvedValue(undefined);

    await invalidateAgentSidebarConversations(queryClient);

    // `cancelRefetch: false` dedupes onto a fetch that started before this
    // invalidation, and that fetch clears `isInvalidated` on success — so its
    // payload can predate the change that triggered us. One trailing pass
    // closes that hole without looping.
    expect(invalidateSpy).toHaveBeenCalledTimes(2);
    expect(invalidateSpy).toHaveBeenNthCalledWith(1, SIDEBAR_FILTERS, NON_CANCELLING);
    expect(invalidateSpy).toHaveBeenNthCalledWith(2, SIDEBAR_FILTERS, NON_CANCELLING);
  });

  it("does not cancel or restart an in-flight sidebar listing", async () => {
    const queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false, gcTime: Infinity } },
    });
    const queryKey = agentSidebarConversationKeys.projectGroup("p1", false);
    const slowListing = deferred<string>();
    let aborted = false;

    // Call 1 seeds a payload, call 2 is the slow listing we hold open, and any
    // later call resolves immediately so the trailing pass can settle.
    const queryFn = vi.fn(({ signal }: { signal: AbortSignal }) => {
      signal.addEventListener("abort", () => {
        aborted = true;
      });
      if (queryFn.mock.calls.length === 1) return Promise.resolve("seed");
      if (queryFn.mock.calls.length === 2) return slowListing.promise;
      return Promise.resolve("fresh");
    });

    // The query must be *active* (have an observer) — `invalidateQueries`
    // defaults to `refetchType: "active"`, so an observerless query is never
    // refetched and therefore never cancelled. The real sidebar queries are
    // rendered by hooks, so this is the shape that actually livelocks.
    const observer = new QueryObserver(queryClient, {
      queryKey,
      queryFn,
      staleTime: Infinity,
      retry: false,
    });
    const unsubscribe = observer.subscribe(() => {});

    // Seed a payload: `Query.fetch` only cancels when `state.data !== undefined`.
    await vi.waitFor(() => expect(queryClient.getQueryData(queryKey)).toBe("seed"));

    // Start the slow listing and invalidate while it is still running.
    void queryClient.refetchQueries({ queryKey, exact: true });
    await vi.waitFor(() => expect(queryFn).toHaveBeenCalledTimes(2));

    const invalidation = invalidateAgentSidebarConversations(queryClient);
    await Promise.resolve();

    // Pre-fix, `cancelRefetch: true` aborted this fetch and started a third.
    expect(aborted).toBe(false);
    expect(queryFn).toHaveBeenCalledTimes(2);

    slowListing.resolve("slow-listing-result");
    await invalidation;

    // The trailing pass ran after the held listing settled, so the cache ends up
    // on the newer payload rather than one that predates the invalidation.
    expect(queryFn).toHaveBeenCalledTimes(3);
    await vi.waitFor(() => expect(queryClient.getQueryData(queryKey)).toBe("fresh"));

    unsubscribe();
    queryClient.clear();
  });
});
