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

function conversation(
  id: string,
  overrides: Partial<AgentConversation> = {}
): AgentConversation {
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
    ...overrides,
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
      "conv-running": { isRunning: true, agentStatus: "generating" },
      "conv-idle": { isRunning: false, agentStatus: "idle" },
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

  it("rehydrates retained idle sidebar rows as waiting for input", async () => {
    const waitingConversation = conversation("conv-waiting");
    mockGetAgentRunningStates.mockResolvedValueOnce({
      "conv-waiting": { isRunning: true, agentStatus: "waiting_for_input" },
    });

    renderHook(() =>
      useAgentSidebarRunningStates([waitingConversation], true)
    );

    await act(async () => {});

    const waitingStoreKey = getAgentConversationStoreKey(waitingConversation);
    const state = useChatStore.getState();
    expect(state.agentStatus[waitingStoreKey]).toBe("waiting_for_input");
    expect(state.activeConversationIds[waitingStoreKey]).toBe("conv-waiting");
  });

  it("keeps legacy boolean bulk states compatible", async () => {
    const runningConversation = conversation("conv-legacy");
    mockGetAgentRunningStates.mockResolvedValueOnce({
      "conv-legacy": true,
    });

    renderHook(() =>
      useAgentSidebarRunningStates([runningConversation], true)
    );

    await act(async () => {});

    const runningStoreKey = getAgentConversationStoreKey(runningConversation);
    const state = useChatStore.getState();
    expect(state.agentStatus[runningStoreKey]).toBe("generating");
    expect(state.activeConversationIds[runningStoreKey]).toBe("conv-legacy");
  });

  it("clears stale sidebar status when bulk state says not running", async () => {
    const staleConversation = conversation("conv-stale");
    const storeKey = getAgentConversationStoreKey(staleConversation);
    useChatStore.getState().setAgentRunning(storeKey, true);
    mockGetAgentRunningStates.mockResolvedValueOnce({
      "conv-stale": { isRunning: false, agentStatus: "idle" },
    });

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

  it("deduplicates project conversations and ignores non-project conversations", async () => {
    const projectConversation = conversation("conv-project");
    const duplicateProjectConversation = conversation("conv-project");
    const ideationConversation = conversation("conv-ideation", {
      contextType: "ideation",
      contextId: "session-1",
      ideationSessionId: "session-1",
    });

    renderHook(() =>
      useAgentSidebarRunningStates(
        [projectConversation, duplicateProjectConversation, ideationConversation],
        true
      )
    );

    await act(async () => {});

    expect(mockGetAgentRunningStates).toHaveBeenCalledWith("project", [
      "conv-project",
    ]);
  });

  it("does not start a second poll while a previous poll is in flight", async () => {
    let resolvePoll!: (
      states: Record<string, { isRunning: boolean; agentStatus: string }>
    ) => void;
    mockGetAgentRunningStates.mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          resolvePoll = resolve;
        })
    );

    renderHook(() =>
      useAgentSidebarRunningStates([conversation("conv-pending")], true)
    );

    await act(async () => {
      vi.advanceTimersByTime(5_000);
    });

    expect(mockGetAgentRunningStates).toHaveBeenCalledTimes(1);

    await act(async () => {
      resolvePoll({
        "conv-pending": { isRunning: false, agentStatus: "idle" },
      });
    });

    await act(async () => {
      vi.advanceTimersByTime(5_000);
    });

    expect(mockGetAgentRunningStates).toHaveBeenCalledTimes(2);
  });

  it("ignores bulk polling errors", async () => {
    mockGetAgentRunningStates.mockRejectedValueOnce(new Error("liveness failed"));

    renderHook(() =>
      useAgentSidebarRunningStates([conversation("conv-error")], true)
    );

    await act(async () => {});

    expect(useChatStore.getState().agentStatus).toEqual({});
  });
});
