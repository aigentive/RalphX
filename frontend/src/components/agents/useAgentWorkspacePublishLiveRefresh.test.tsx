import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, renderHook } from "@testing-library/react";
import type { ReactNode } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { useChatStore } from "@/stores/chatStore";

import { agentWorkspaceKeys } from "./agentWorkspaceQueries";
import {
  PUBLISH_LIVE_REFRESH_INTERVAL_MS,
  useAgentWorkspacePublishLiveRefresh,
} from "./useAgentWorkspacePublishLiveRefresh";

const CONVERSATION_ID = "run-conversation-1";
const STORE_KEY = `project:${CONVERSATION_ID}`;

function createWrapper(queryClient: QueryClient) {
  return function Wrapper({ children }: { children: ReactNode }) {
    return (
      <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
    );
  };
}

function invalidatedKeys(spy: ReturnType<typeof vi.spyOn>) {
  return spy.mock.calls.map(
    ([filters]) => (filters as { queryKey: readonly unknown[] }).queryKey,
  );
}

describe("useAgentWorkspacePublishLiveRefresh", () => {
  let queryClient: QueryClient;

  beforeEach(() => {
    vi.useFakeTimers();
    queryClient = new QueryClient({
      defaultOptions: { queries: { retry: false } },
    });
    useChatStore.setState({ agentStatus: {} });
  });

  afterEach(() => {
    vi.useRealTimers();
    useChatStore.setState({ agentStatus: {} });
  });

  it("polls publish queries while the workspace conversation is generating and stops when idle", () => {
    const invalidateSpy = vi.spyOn(queryClient, "invalidateQueries");
    useChatStore.getState().setAgentStatus(STORE_KEY, "generating");

    const { rerender } = renderHook(
      () => useAgentWorkspacePublishLiveRefresh(CONVERSATION_ID),
      { wrapper: createWrapper(queryClient) },
    );

    expect(invalidateSpy).not.toHaveBeenCalled();

    act(() => {
      vi.advanceTimersByTime(PUBLISH_LIVE_REFRESH_INTERVAL_MS);
    });
    expect(invalidatedKeys(invalidateSpy)).toEqual(
      expect.arrayContaining([
        agentWorkspaceKeys.review(CONVERSATION_ID),
        agentWorkspaceKeys.changeSummary(CONVERSATION_ID),
        agentWorkspaceKeys.diff(CONVERSATION_ID),
        agentWorkspaceKeys.commits(CONVERSATION_ID),
      ]),
    );

    act(() => {
      vi.advanceTimersByTime(PUBLISH_LIVE_REFRESH_INTERVAL_MS);
    });
    const pollingCallCount = invalidateSpy.mock.calls.length;
    expect(pollingCallCount).toBe(8);

    // Terminal transition: one final refresh, then no further polling.
    act(() => {
      useChatStore.getState().setAgentStatus(STORE_KEY, "idle");
    });
    rerender();
    expect(invalidateSpy.mock.calls.length).toBe(pollingCallCount + 4);

    act(() => {
      vi.advanceTimersByTime(PUBLISH_LIVE_REFRESH_INTERVAL_MS * 4);
    });
    expect(invalidateSpy.mock.calls.length).toBe(pollingCallCount + 4);
  });

  it("does not run a settle refresh for a different conversation", () => {
    const invalidateSpy = vi.spyOn(queryClient, "invalidateQueries");
    useChatStore.getState().setAgentStatus(STORE_KEY, "generating");

    const { rerender } = renderHook(
      ({ conversationId }: { conversationId: string | null }) =>
        useAgentWorkspacePublishLiveRefresh(conversationId),
      {
        wrapper: createWrapper(queryClient),
        initialProps: { conversationId: CONVERSATION_ID as string | null },
      },
    );

    // Rebinding the surface to another idle conversation must not fire the
    // settle invalidation against the new conversation.
    rerender({ conversationId: "other-conversation" });
    expect(
      invalidatedKeys(invalidateSpy).some(
        (key) => key[key.length - 1] === "other-conversation",
      ),
    ).toBe(false);
  });

  it("stays inert without a conversation id", () => {
    const invalidateSpy = vi.spyOn(queryClient, "invalidateQueries");

    renderHook(() => useAgentWorkspacePublishLiveRefresh(null), {
      wrapper: createWrapper(queryClient),
    });

    act(() => {
      vi.advanceTimersByTime(PUBLISH_LIVE_REFRESH_INTERVAL_MS * 4);
    });
    expect(invalidateSpy).not.toHaveBeenCalled();
  });
});
