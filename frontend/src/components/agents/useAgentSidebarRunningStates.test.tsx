import { act, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { useChatStore } from "@/stores/chatStore";

import {
  getAgentConversationStoreKey,
  type AgentConversation,
} from "./agentConversations";
import { useAgentSidebarRunningStates } from "./useAgentSidebarRunningStates";

const { mockGetAgentConversationRuntimeStatuses } = vi.hoisted(() => ({
  mockGetAgentConversationRuntimeStatuses: vi.fn(),
}));

vi.mock("@/api/chat", () => ({
  chatApi: {
    getAgentConversationRuntimeStatuses: mockGetAgentConversationRuntimeStatuses,
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
    mockGetAgentConversationRuntimeStatuses.mockReset();
    mockGetAgentConversationRuntimeStatuses.mockResolvedValue({});
    useChatStore.setState({
      activeConversationIds: {},
      activeAgentRunIds: {},
      agentStatus: {},
      agentActivityLabels: {},
    });
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("rehydrates idle sidebar rows from runtime status", async () => {
    const runningConversation = conversation("conv-running");
    const idleConversation = conversation("conv-idle");
    mockGetAgentConversationRuntimeStatuses.mockResolvedValueOnce({
      "conv-running": {
        conversationId: "conv-running",
        isRunning: true,
        agentStatus: "generating",
        primarySource: "workspace",
        summaryLabel: "Agent running",
        items: [],
      },
      "conv-idle": {
        conversationId: "conv-idle",
        isRunning: false,
        agentStatus: "idle",
        primarySource: null,
        summaryLabel: null,
        items: [],
      },
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

    expect(mockGetAgentConversationRuntimeStatuses).toHaveBeenCalledWith([
      "conv-running",
      "conv-idle",
    ]);
    expect(state.agentStatus[runningStoreKey]).toBe("generating");
    expect(state.activeConversationIds[runningStoreKey]).toBe("conv-running");
    expect(state.agentStatus[idleStoreKey]).toBeUndefined();
  });

  it("rehydrates standalone rows into the standalone store-key family", async () => {
    const standaloneConversation = conversation("standalone-1", {
      contextType: "standalone",
      contextId: "standalone-1",
      projectId: null,
    });
    mockGetAgentConversationRuntimeStatuses.mockResolvedValueOnce({
      "standalone-1": {
        conversationId: "standalone-1",
        isRunning: true,
        agentStatus: "generating",
        primarySource: "workspace",
        summaryLabel: "Agent running",
        items: [],
      },
    });

    renderHook(() =>
      useAgentSidebarRunningStates([standaloneConversation], true),
    );
    await act(async () => {});

    expect(mockGetAgentConversationRuntimeStatuses).toHaveBeenCalledWith([
      "standalone-1",
    ]);
    expect(
      useChatStore.getState().activeConversationIds["standalone:standalone-1"],
    ).toBe("standalone-1");
  });

  it("rehydrates retained idle sidebar rows as waiting for input", async () => {
    const waitingConversation = conversation("conv-waiting");
    mockGetAgentConversationRuntimeStatuses.mockResolvedValueOnce({
      "conv-waiting": {
        conversationId: "conv-waiting",
        isRunning: true,
        agentStatus: "waiting_for_input",
        primarySource: "ideation",
        summaryLabel: "Ideation running",
        items: [],
      },
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

  it("rehydrates sidebar rows from associated verification runtime status", async () => {
    const runningConversation = conversation("conv-verifying");
    mockGetAgentConversationRuntimeStatuses.mockResolvedValueOnce({
      "conv-verifying": {
        conversationId: "conv-verifying",
        isRunning: true,
        agentStatus: "generating",
        primarySource: "verification",
        summaryLabel: "Verifying",
        items: [
          {
            source: "verification",
            contextType: "ideation",
            contextId: "verification-session",
            label: "Verifying",
            title: "Verification run",
            agentStatus: "generating",
            taskId: null,
            internalStatus: null,
            runningProcess: null,
            ideationSession: null,
            parentSessionId: "plan-session",
            childSessionId: "verification-session",
            conversationId: null,
          },
        ],
      },
    });

    renderHook(() =>
      useAgentSidebarRunningStates([runningConversation], true)
    );

    await act(async () => {});

    const runningStoreKey = getAgentConversationStoreKey(runningConversation);
    const state = useChatStore.getState();
    expect(state.agentStatus[runningStoreKey]).toBe("generating");
    expect(state.activeConversationIds[runningStoreKey]).toBe("conv-verifying");
  });

  it("labels associated workspace Review runtime as reviewing", async () => {
    const runningConversation = conversation("conv-reviewing");
    mockGetAgentConversationRuntimeStatuses.mockResolvedValueOnce({
      "conv-reviewing": {
        conversationId: "conv-reviewing",
        isRunning: true,
        agentStatus: "generating",
        primarySource: "workspace_review",
        summaryLabel: "Reviewing",
        items: [
          {
            source: "workspace_review",
            contextType: "project",
            contextId: "review-child",
            label: "Reviewing",
            title: "Review workspace changes",
            agentStatus: "generating",
            taskId: null,
            internalStatus: "reviewing",
            runningProcess: null,
            ideationSession: null,
            parentSessionId: null,
            childSessionId: null,
            conversationId: "review-child",
          },
        ],
      },
    });

    renderHook(() =>
      useAgentSidebarRunningStates([runningConversation], true)
    );

    await act(async () => {});

    const runningStoreKey = getAgentConversationStoreKey(runningConversation);
    const state = useChatStore.getState();
    expect(state.agentStatus[runningStoreKey]).toBe("generating");
    expect(state.activeConversationIds[runningStoreKey]).toBe("conv-reviewing");
    expect(state.agentActivityLabels[runningStoreKey]).toBe("reviewing");
  });

  it("clears stale sidebar status when runtime status says not running", async () => {
    const staleConversation = conversation("conv-stale");
    const storeKey = getAgentConversationStoreKey(staleConversation);
    useChatStore.getState().setAgentRunning(storeKey, true);
    useChatStore.getState().setAgentActivityLabel(storeKey, "reviewing");
    mockGetAgentConversationRuntimeStatuses.mockResolvedValueOnce({
      "conv-stale": {
        conversationId: "conv-stale",
        isRunning: false,
        agentStatus: "idle",
        primarySource: null,
        summaryLabel: null,
        items: [],
      },
    });

    renderHook(() =>
      useAgentSidebarRunningStates([staleConversation], true)
    );

    await act(async () => {});

    expect(useChatStore.getState().agentStatus[storeKey]).toBeUndefined();
    expect(useChatStore.getState().agentActivityLabels[storeKey]).toBeUndefined();
  });

  it("does not poll while the sidebar is hidden", async () => {
    renderHook(() =>
      useAgentSidebarRunningStates([conversation("conv-hidden")], false)
    );

    await act(async () => {
      vi.advanceTimersByTime(10_000);
    });

    expect(mockGetAgentConversationRuntimeStatuses).not.toHaveBeenCalled();
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

    expect(mockGetAgentConversationRuntimeStatuses).toHaveBeenCalledWith([
      "conv-project",
    ]);
  });

  it("does not start a second poll while a previous poll is in flight", async () => {
    let resolvePoll!: (states: Record<string, unknown>) => void;
    mockGetAgentConversationRuntimeStatuses.mockImplementationOnce(
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

    expect(mockGetAgentConversationRuntimeStatuses).toHaveBeenCalledTimes(1);

    await act(async () => {
      resolvePoll({
        "conv-pending": {
          conversationId: "conv-pending",
          isRunning: false,
          agentStatus: "idle",
          primarySource: null,
          summaryLabel: null,
          items: [],
        },
      });
    });

    await act(async () => {
      vi.advanceTimersByTime(5_000);
    });

    expect(mockGetAgentConversationRuntimeStatuses).toHaveBeenCalledTimes(2);
  });

  it("ignores bulk polling errors", async () => {
    mockGetAgentConversationRuntimeStatuses.mockRejectedValueOnce(
      new Error("liveness failed")
    );

    renderHook(() =>
      useAgentSidebarRunningStates([conversation("conv-error")], true)
    );

    await act(async () => {});

    expect(useChatStore.getState().agentStatus).toEqual({});
  });
});
