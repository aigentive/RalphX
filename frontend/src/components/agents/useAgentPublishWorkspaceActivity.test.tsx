import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, renderHook, waitFor } from "@testing-library/react";
import type { ReactNode } from "react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import type { AgentConversationRuntimeStatus } from "@/api/chat";

import { agentConversationRuntimeStatusKeys } from "./useAgentConversationRuntimeStatus";
import { useAgentPublishWorkspaceActivity } from "./useAgentPublishWorkspaceActivity";

const {
  getRuntimeStatusesMock,
  getWorkspaceChangeSummaryMock,
  getWorkspaceReviewMock,
} = vi.hoisted(() => ({
  getRuntimeStatusesMock: vi.fn(),
  getWorkspaceChangeSummaryMock: vi.fn(),
  getWorkspaceReviewMock: vi.fn(),
}));

vi.mock("@/api/chat", () => ({
  chatApi: {
    getAgentConversationRuntimeStatuses: getRuntimeStatusesMock,
  },
}));

vi.mock("@/api/diff", () => ({
  diffApi: {
    getAgentConversationWorkspaceChangeSummary: getWorkspaceChangeSummaryMock,
    getAgentConversationWorkspaceReview: getWorkspaceReviewMock,
  },
}));

vi.mock("@/providers/EventProvider", () => ({
  useEventBus: () => ({
    subscribe: vi.fn(() => vi.fn()),
  }),
}));

function runtimeStatus(
  conversationId: string,
  isRunning: boolean,
): AgentConversationRuntimeStatus {
  return {
    conversationId,
    isRunning,
    agentStatus: isRunning ? "generating" : "idle",
    primarySource: isRunning ? "workspace" : null,
    summaryLabel: isRunning ? "Agent running" : null,
    items: [],
  };
}

function createWrapper() {
  const queryClient = new QueryClient({
    defaultOptions: {
      queries: { retry: false, gcTime: Number.POSITIVE_INFINITY },
    },
  });
  const wrapper = ({ children }: { children: ReactNode }) => (
    <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
  );
  return { queryClient, wrapper };
}

describe("useAgentPublishWorkspaceActivity", () => {
  beforeEach(() => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    getRuntimeStatusesMock.mockReset();
    getWorkspaceChangeSummaryMock.mockReset();
    getWorkspaceReviewMock.mockReset();
    getWorkspaceChangeSummaryMock.mockResolvedValue({
      supportsWorktreeModes: true,
      staged: { fileCount: 0, additions: 0, deletions: 0 },
      unstaged: { fileCount: 1, additions: 3, deletions: 1 },
    });
    getWorkspaceReviewMock.mockResolvedValue({
      changes: [],
      commits: [],
      baseRef: "main",
      headRef: "HEAD",
      supportsWorktreeModes: true,
    });
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("polls review and change facts for the exact active run conversation", async () => {
    getRuntimeStatusesMock.mockImplementation(async (conversationIds: string[]) => {
      const conversationId = conversationIds[0] ?? "missing";
      return {
        [conversationId]: runtimeStatus(conversationId, true),
      };
    });
    const { wrapper } = createWrapper();

    const { result } = renderHook(
      () =>
        useAgentPublishWorkspaceActivity({
          conversationId: "run-conversation-7",
          reviewEnabled: true,
          liveRefreshEnabled: true,
        }),
      { wrapper },
    );

    await waitFor(() => {
      expect(result.current.isRunActive).toBe(true);
      expect(getWorkspaceReviewMock).toHaveBeenCalledWith("run-conversation-7");
      expect(getWorkspaceChangeSummaryMock).toHaveBeenCalledWith(
        "run-conversation-7",
      );
    });
    const reviewCalls = getWorkspaceReviewMock.mock.calls.length;
    const summaryCalls = getWorkspaceChangeSummaryMock.mock.calls.length;

    await act(async () => {
      await vi.advanceTimersByTimeAsync(2_500);
    });

    expect(getWorkspaceReviewMock.mock.calls.length).toBeGreaterThan(reviewCalls);
    expect(getWorkspaceChangeSummaryMock.mock.calls.length).toBeGreaterThan(
      summaryCalls,
    );
    expect(getWorkspaceReviewMock).not.toHaveBeenCalledWith("setup-conversation");
  });

  it("runs one trailing refresh when the focused run settles, then stops polling", async () => {
    getRuntimeStatusesMock.mockResolvedValue({
      "run-conversation-7": runtimeStatus("run-conversation-7", true),
    });
    const { queryClient, wrapper } = createWrapper();

    const { result } = renderHook(
      () =>
        useAgentPublishWorkspaceActivity({
          conversationId: "run-conversation-7",
          reviewEnabled: true,
          liveRefreshEnabled: true,
        }),
      { wrapper },
    );

    await waitFor(() => expect(result.current.isRunActive).toBe(true));
    const reviewCallsBeforeSettlement = getWorkspaceReviewMock.mock.calls.length;
    const summaryCallsBeforeSettlement =
      getWorkspaceChangeSummaryMock.mock.calls.length;
    getRuntimeStatusesMock.mockResolvedValue({
      "run-conversation-7": runtimeStatus("run-conversation-7", false),
    });

    act(() => {
      queryClient.setQueryData(
        agentConversationRuntimeStatusKeys.detail("run-conversation-7"),
        runtimeStatus("run-conversation-7", false),
      );
    });

    await waitFor(() => {
      expect(result.current.isRunActive).toBe(false);
      expect(getWorkspaceReviewMock.mock.calls.length).toBeGreaterThan(
        reviewCallsBeforeSettlement,
      );
      expect(getWorkspaceChangeSummaryMock.mock.calls.length).toBeGreaterThan(
        summaryCallsBeforeSettlement,
      );
    });
    const settledReviewCalls = getWorkspaceReviewMock.mock.calls.length;
    const settledSummaryCalls = getWorkspaceChangeSummaryMock.mock.calls.length;

    await act(async () => {
      await vi.advanceTimersByTimeAsync(5_000);
    });

    expect(getWorkspaceReviewMock).toHaveBeenCalledTimes(settledReviewCalls);
    expect(getWorkspaceChangeSummaryMock).toHaveBeenCalledTimes(
      settledSummaryCalls,
    );
  });
});
