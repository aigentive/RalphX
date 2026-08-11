import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, render, renderHook, waitFor } from "@testing-library/react";
import type { ReactNode } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { chatApi, type AgentConversationWorkspaceFreshness } from "@/api/chat";
import { createTestQueryClient } from "@/test/store-utils";

import { agentWorkspaceKeys } from "./agentWorkspaceQueries";
import { useAgentWorkspaceFullFreshness } from "./useAgentWorkspaceFullFreshness";

function freshness(
  conversationId: string,
): AgentConversationWorkspaceFreshness {
  return {
    conversationId,
    freshnessScope: "full",
    baseRef: "main",
    baseDisplayName: "Project default (main)",
    targetRef: "origin/main",
    capturedBaseCommit: "base-sha",
    targetBaseCommit: "base-sha",
    isBaseAhead: false,
    hasUncommittedChanges: false,
    unpublishedCommitCount: 0,
    remoteRefreshed: true,
    worktreeStatusChecked: true,
    baseStatus: "valid",
    effectiveBaseRef: null,
    effectiveBaseDisplayName: null,
    baseBlockReason: null,
  };
}

function wrapper(client: QueryClient) {
  return function TestWrapper({ children }: { children: ReactNode }) {
    return (
      <QueryClientProvider client={client}>{children}</QueryClientProvider>
    );
  };
}

describe("useAgentWorkspaceFullFreshness", () => {
  beforeEach(() => {
    vi.restoreAllMocks();
    vi.useRealTimers();
  });

  it("uses the canonical full-scope key and API arguments only when enabled", async () => {
    const getFreshness = vi
      .spyOn(chatApi, "getAgentConversationWorkspaceFreshness")
      .mockResolvedValue(freshness("conversation-1"));
    const queryClient = createTestQueryClient();
    const { rerender } = renderHook(
      ({ enabled }) =>
        useAgentWorkspaceFullFreshness("conversation-1", { enabled }),
      {
        initialProps: { enabled: false },
        wrapper: wrapper(queryClient),
      },
    );

    expect(getFreshness).not.toHaveBeenCalled();
    expect(
      queryClient.getQueryState(
        agentWorkspaceKeys.scopedFreshness("conversation-1", "full"),
      )?.fetchStatus,
    ).toBe("idle");

    rerender({ enabled: true });

    await waitFor(() =>
      expect(getFreshness).toHaveBeenCalledWith("conversation-1", {
        scope: "full",
      }),
    );
  });

  it("switches conversation keys without carrying the previous result", async () => {
    vi.spyOn(
      chatApi,
      "getAgentConversationWorkspaceFreshness",
    ).mockImplementation(async (conversationId) => freshness(conversationId));
    const queryClient = createTestQueryClient();
    const { result, rerender } = renderHook(
      ({ conversationId }) =>
        useAgentWorkspaceFullFreshness(conversationId, { enabled: true }),
      {
        initialProps: { conversationId: "conversation-1" },
        wrapper: wrapper(queryClient),
      },
    );

    await waitFor(() =>
      expect(result.current.data?.conversationId).toBe("conversation-1"),
    );

    rerender({ conversationId: "conversation-2" });

    expect(result.current.data).toBeUndefined();
    await waitFor(() =>
      expect(result.current.data?.conversationId).toBe("conversation-2"),
    );
  });

  it("deduplicates consumers through the shared full-scope query key", async () => {
    const getFreshness = vi
      .spyOn(chatApi, "getAgentConversationWorkspaceFreshness")
      .mockResolvedValue(freshness("conversation-1"));
    const queryClient = createTestQueryClient();

    function Consumers() {
      useAgentWorkspaceFullFreshness("conversation-1", { enabled: true });
      useAgentWorkspaceFullFreshness("conversation-1", { enabled: true });
      return null;
    }

    render(
      <QueryClientProvider client={queryClient}>
        <Consumers />
      </QueryClientProvider>,
    );

    await waitFor(() => expect(getFreshness).toHaveBeenCalledTimes(1));
  });

  it("polls quickly for an active operation and backs off while idle", async () => {
    vi.useFakeTimers();
    const getFreshness = vi
      .spyOn(chatApi, "getAgentConversationWorkspaceFreshness")
      .mockResolvedValue(freshness("conversation-1"));
    const queryClient = createTestQueryClient();
    const { rerender } = renderHook(
      ({ isOperationActive }) =>
        useAgentWorkspaceFullFreshness("conversation-1", {
          enabled: true,
          isOperationActive,
        }),
      {
        initialProps: { isOperationActive: true },
        wrapper: wrapper(queryClient),
      },
    );

    await act(async () => {});
    expect(getFreshness).toHaveBeenCalledTimes(1);

    await act(async () => {
      await vi.advanceTimersByTimeAsync(5_000);
    });
    expect(getFreshness).toHaveBeenCalledTimes(2);

    rerender({ isOperationActive: false });
    await act(async () => {
      await vi.advanceTimersByTimeAsync(5_000);
    });
    expect(getFreshness).toHaveBeenCalledTimes(2);

    await act(async () => {
      await vi.advanceTimersByTimeAsync(55_000);
    });
    expect(getFreshness).toHaveBeenCalledTimes(3);
  });
});
