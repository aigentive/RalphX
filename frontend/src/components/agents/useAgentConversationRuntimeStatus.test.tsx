import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { renderHook, waitFor } from "@testing-library/react";
import type { ReactNode } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import type { AgentConversationRuntimeStatus } from "@/api/chat";
import { buildStoreKey } from "@/lib/chat-context-registry";
import { useChatStore } from "@/stores/chatStore";

import { useAgentConversationRuntimeStatus } from "./useAgentConversationRuntimeStatus";

const { mockGetAgentConversationRuntimeStatuses } = vi.hoisted(() => ({
  mockGetAgentConversationRuntimeStatuses: vi.fn(),
}));

vi.mock("@/api/chat", () => ({
  chatApi: {
    getAgentConversationRuntimeStatuses: mockGetAgentConversationRuntimeStatuses,
  },
}));

vi.mock("@/providers/EventProvider", () => ({
  useEventBus: () => ({
    subscribe: vi.fn(() => vi.fn()),
  }),
}));

function runtimeStatus(
  overrides: Partial<AgentConversationRuntimeStatus> = {},
): AgentConversationRuntimeStatus {
  return {
    conversationId: "conversation-1",
    isRunning: true,
    agentStatus: "generating",
    primarySource: "workspace_review",
    summaryLabel: "Reviewing",
    items: [
      {
        source: "workspace_review",
        contextType: "project",
        contextId: "review-conversation-1",
        label: "Reviewing",
        title: "Review workspace changes",
        agentStatus: "generating",
        taskId: null,
        internalStatus: "reviewing",
        runningProcess: null,
        ideationSession: null,
        parentSessionId: null,
        childSessionId: null,
        conversationId: "review-conversation-1",
      },
    ],
    ...overrides,
  };
}

function createWrapper() {
  const queryClient = new QueryClient({
    defaultOptions: {
      queries: {
        retry: false,
      },
    },
  });

  return function Wrapper({ children }: { children: ReactNode }) {
    return (
      <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
    );
  };
}

describe("useAgentConversationRuntimeStatus", () => {
  beforeEach(() => {
    mockGetAgentConversationRuntimeStatuses.mockReset();
    useChatStore.setState({
      activeConversationIds: {},
      activeAgentRunIds: {},
      agentStatus: {},
      agentActivityLabels: {},
    });
  });

  it("mirrors workspace Review runtime status into the parent sidebar store key", async () => {
    mockGetAgentConversationRuntimeStatuses.mockResolvedValueOnce({
      "conversation-1": runtimeStatus(),
    });

    renderHook(() => useAgentConversationRuntimeStatus("conversation-1"), {
      wrapper: createWrapper(),
    });

    const storeKey = buildStoreKey("project", "conversation-1");
    await waitFor(() => {
      expect(useChatStore.getState().agentActivityLabels[storeKey]).toBe(
        "reviewing",
      );
    });

    const state = useChatStore.getState();
    expect(state.agentStatus[storeKey]).toBe("generating");
    expect(state.activeConversationIds[storeKey]).toBe("conversation-1");
  });

  it("clears stale parent sidebar runtime state after an idle lookup", async () => {
    const storeKey = buildStoreKey("project", "conversation-1");
    useChatStore.getState().setAgentStatus(storeKey, "generating");
    useChatStore.getState().setAgentActivityLabel(storeKey, "reviewing");
    mockGetAgentConversationRuntimeStatuses.mockResolvedValueOnce({
      "conversation-1": runtimeStatus({
        isRunning: false,
        agentStatus: "idle",
        primarySource: null,
        summaryLabel: null,
        items: [],
      }),
    });

    renderHook(() => useAgentConversationRuntimeStatus("conversation-1"), {
      wrapper: createWrapper(),
    });

    await waitFor(() => {
      expect(useChatStore.getState().agentStatus[storeKey]).toBeUndefined();
    });
    expect(
      useChatStore.getState().agentActivityLabels[storeKey],
    ).toBeUndefined();
  });
});
