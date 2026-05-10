import { act, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { useChatStore } from "@/stores/chatStore";

import {
  getAgentConversationStoreKey,
  type AgentConversation,
} from "./agentConversations";
import { useAgentSidebarRunningStates } from "./useAgentSidebarRunningStates";

const { mockGetAgentRunningStates } = vi.hoisted(() => ({
  mockGetAgentRunningStates: vi.fn(),
}));

vi.mock("@/api/chat", () => ({
  chatApi: {
    getAgentRunningStates: mockGetAgentRunningStates,
  },
}));

function conversation(id: string): AgentConversation {
  const now = "2026-05-10T12:00:00.000Z";
  return {
    id,
    contextType: "project",
    contextId: "project-1",
    projectId: "project-1",
    ideationSessionId: null,
    providerSessionId: null,
    providerHarness: null,
    title: id,
    messageCount: 0,
    lastMessageAt: null,
    createdAt: now,
    updatedAt: now,
    archivedAt: null,
  };
}

describe("useAgentSidebarRunningStates", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    mockGetAgentRunningStates.mockReset();
    mockGetAgentRunningStates.mockResolvedValue({});
    useChatStore.setState({
      activeConversationIds: {},
      activeAgentRunIds: {},
      agentStatus: {},
    });
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("rehydrates idle sidebar rows from bulk running state", async () => {
    const runningConversation = conversation("conv-running");
    const idleConversation = conversation("conv-idle");
    mockGetAgentRunningStates.mockResolvedValueOnce({
      "conv-running": true,
      "conv-idle": false,
    });

    renderHook(() =>
      useAgentSidebarRunningStates(
        [runningConversation, idleConversation],
        true
      )
    );

    await act(async () => {});

    const runningStoreKey = getAgentConversationStoreKey(runningConversation);
    const idleStoreKey = getAgentConversationStoreKey(idleConversation);
    const state = useChatStore.getState();

    expect(mockGetAgentRunningStates).toHaveBeenCalledWith("project", [
      "conv-running",
      "conv-idle",
    ]);
    expect(state.agentStatus[runningStoreKey]).toBe("generating");
    expect(state.activeConversationIds[runningStoreKey]).toBe("conv-running");
    expect(state.agentStatus[idleStoreKey]).toBeUndefined();
  });

  it("clears stale sidebar status when bulk state says not running", async () => {
    const staleConversation = conversation("conv-stale");
    const storeKey = getAgentConversationStoreKey(staleConversation);
    useChatStore.getState().setAgentRunning(storeKey, true);
    mockGetAgentRunningStates.mockResolvedValueOnce({ "conv-stale": false });

    renderHook(() =>
      useAgentSidebarRunningStates([staleConversation], true)
    );

    await act(async () => {});

    expect(useChatStore.getState().agentStatus[storeKey]).toBeUndefined();
  });

  it("does not poll while the sidebar is hidden", async () => {
    renderHook(() =>
      useAgentSidebarRunningStates([conversation("conv-hidden")], false)
    );

    await act(async () => {
      vi.advanceTimersByTime(10_000);
    });

    expect(mockGetAgentRunningStates).not.toHaveBeenCalled();
  });
});
