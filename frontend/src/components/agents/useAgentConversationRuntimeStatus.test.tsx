import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { act, renderHook, waitFor } from "@testing-library/react";
import type { ReactNode } from "react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import type { AgentConversationRuntimeStatus } from "@/api/chat";
import { buildStoreKey } from "@/lib/chat-context-registry";
import { useChatStore } from "@/stores/chatStore";
import {
  LOCAL_ENVIRONMENT_ID,
  useEnvironmentStore,
} from "@/stores/environmentStore";

import { useAgentConversationRuntimeStatus } from "./useAgentConversationRuntimeStatus";

const {
  mockGetAgentConversationRuntimeIndex,
  mockGetAgentConversationRuntimeStatuses,
} = vi.hoisted(() => ({
  mockGetAgentConversationRuntimeIndex: vi.fn(),
  mockGetAgentConversationRuntimeStatuses: vi.fn(),
}));

const eventHandlers = new Map<string, Set<(payload: unknown) => void>>();

vi.mock("@/api/chat", () => ({
  chatApi: {
    getAgentConversationRuntimeIndex: mockGetAgentConversationRuntimeIndex,
    getAgentConversationRuntimeStatuses: mockGetAgentConversationRuntimeStatuses,
  },
}));

vi.mock("@/providers/EventProvider", () => ({
  useEventBus: () => ({
    subscribe: vi.fn((event: string, handler: (payload: unknown) => void) => {
      const handlers =
        eventHandlers.get(event) ?? new Set<(payload: unknown) => void>();
      handlers.add(handler);
      eventHandlers.set(event, handlers);
      return () => {
        handlers.delete(handler);
        if (handlers.size === 0) {
          eventHandlers.delete(event);
        }
      };
    }),
  }),
}));

function emitEvent(event: string, payload: unknown) {
  act(() => {
    eventHandlers.get(event)?.forEach((handler) => {
      handler(payload);
    });
  });
}

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
    mockGetAgentConversationRuntimeIndex.mockReset();
    eventHandlers.clear();
    useChatStore.setState({
      activeConversationIds: {},
      activeAgentRunIds: {},
      agentStatus: {},
      agentActivityLabels: {},
      isSending: {},
    });
    useEnvironmentStore.setState({
      activeEnvironmentId: LOCAL_ENVIRONMENT_ID,
      environments: [
        { id: LOCAL_ENVIRONMENT_ID, name: "This Mac", kind: "local" },
      ],
    });
  });

  it("uses the registered runtime index remotely and never invokes the refused status command", async () => {
    useEnvironmentStore.setState({
      activeEnvironmentId: "env-remote",
      environments: [{ id: "env-remote", name: "Remote", kind: "remote" }],
    });
    mockGetAgentConversationRuntimeIndex.mockResolvedValue({
      conversationId: "conversation-1",
      rows: [],
    });

    renderHook(() => useAgentConversationRuntimeStatus("conversation-1"), {
      wrapper: createWrapper(),
    });

    await waitFor(() => {
      expect(mockGetAgentConversationRuntimeIndex).toHaveBeenCalledTimes(1);
    });
    expect(mockGetAgentConversationRuntimeStatuses).not.toHaveBeenCalled();
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

  it("can read child-only aggregate runtime status without mirroring into visible chat state", async () => {
    mockGetAgentConversationRuntimeStatuses.mockResolvedValueOnce({
      "conversation-1": runtimeStatus(),
    });

    const storeKey = buildStoreKey("project", "conversation-1");
    const { result } = renderHook(
      () =>
        useAgentConversationRuntimeStatus("conversation-1", {
          mirrorToVisibleChatStatus: false,
          storeKey,
        }),
      {
        wrapper: createWrapper(),
      },
    );

    await waitFor(() => {
      expect(result.current.data?.conversationId).toBe("conversation-1");
    });

    const state = useChatStore.getState();
    expect(state.agentStatus[storeKey]).toBeUndefined();
    expect(state.agentActivityLabels[storeKey]).toBeUndefined();
    expect(state.activeConversationIds[storeKey]).toBeUndefined();
  });

  it("can mirror only true workspace runtime status for visible workspace chat surfaces", async () => {
    mockGetAgentConversationRuntimeStatuses.mockResolvedValueOnce({
      "conversation-1": runtimeStatus({
        primarySource: "workspace",
        summaryLabel: "Agent running",
        items: [
          {
            source: "workspace",
            contextType: "project",
            contextId: "conversation-1",
            label: "Running",
            title: "Workspace agent",
            agentStatus: "generating",
            taskId: null,
            internalStatus: null,
            runningProcess: null,
            ideationSession: null,
            parentSessionId: null,
            childSessionId: null,
            conversationId: "conversation-1",
          },
        ],
      }),
    });

    const storeKey = buildStoreKey("project", "conversation-1");
    renderHook(
      () =>
        useAgentConversationRuntimeStatus("conversation-1", {
          mirrorToVisibleChatStatus: (status) =>
            status?.items.some((item) => item.source === "workspace") ?? false,
          storeKey,
        }),
      {
        wrapper: createWrapper(),
      },
    );

    await waitFor(() => {
      expect(useChatStore.getState().agentStatus[storeKey]).toBe("generating");
    });
    expect(useChatStore.getState().activeConversationIds[storeKey]).toBe(
      "conversation-1",
    );
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

  it("keeps optimistic start state while the seed message is sending", async () => {
    const storeKey = buildStoreKey("project", "conversation-1");
    useChatStore.getState().setAgentRunning(storeKey, true);
    useChatStore.getState().setSending(storeKey, true);
    useChatStore
      .getState()
      .setAgentActivityLabel(storeKey, "Setup workspace");
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
      expect(useChatStore.getState().agentStatus[storeKey]).toBe("generating");
    });
    expect(useChatStore.getState().isSending[storeKey]).toBe(true);
    expect(useChatStore.getState().agentActivityLabels[storeKey]).toBe(
      "Setup workspace",
    );
  });

  it("reconciles runtime status into the caller store key for non-project conversations", async () => {
    mockGetAgentConversationRuntimeStatuses.mockResolvedValueOnce({
      "review-conversation-1": runtimeStatus({
        conversationId: "review-conversation-1",
        primarySource: "review",
        summaryLabel: "Reviewing",
        items: [
          {
            source: "review",
            contextType: "review",
            contextId: "task-1",
            label: "Reviewing",
            title: "Review task",
            agentStatus: "generating",
            taskId: "task-1",
            internalStatus: "reviewing",
            runningProcess: null,
            ideationSession: null,
            parentSessionId: null,
            childSessionId: null,
            conversationId: "review-conversation-1",
          },
        ],
      }),
    });

    renderHook(
      () =>
        useAgentConversationRuntimeStatus("review-conversation-1", {
          storeKey: buildStoreKey("review", "task-1"),
        }),
      {
        wrapper: createWrapper(),
      },
    );

    const reviewStoreKey = buildStoreKey("review", "task-1");
    const projectStoreKey = buildStoreKey("project", "review-conversation-1");

    await waitFor(() => {
      expect(useChatStore.getState().agentStatus[reviewStoreKey]).toBe(
        "generating",
      );
    });

    const state = useChatStore.getState();
    expect(state.activeConversationIds[reviewStoreKey]).toBe(
      "review-conversation-1",
    );
    expect(state.agentStatus[projectStoreKey]).toBeUndefined();
    expect(state.activeConversationIds[projectStoreKey]).toBeUndefined();
  });

  it("ignores lifecycle invalidations for unrelated conversations", async () => {
    mockGetAgentConversationRuntimeStatuses.mockResolvedValue({
      "conversation-1": runtimeStatus(),
    });

    const { result } = renderHook(
      () => useAgentConversationRuntimeStatus("conversation-1"),
      {
        wrapper: createWrapper(),
      },
    );

    await waitFor(() => {
      expect(result.current.data?.conversationId).toBe("conversation-1");
    });
    expect(mockGetAgentConversationRuntimeStatuses).toHaveBeenCalledTimes(1);

    emitEvent("agent:run_completed", {
      conversation_id: "other-conversation",
      context_type: "project",
      context_id: "other-conversation",
    });

    await new Promise((resolve) => window.setTimeout(resolve, 0));

    expect(mockGetAgentConversationRuntimeStatuses).toHaveBeenCalledTimes(1);
  });

  it("keeps lifecycle invalidations for known runtime child conversations", async () => {
    mockGetAgentConversationRuntimeStatuses.mockResolvedValue({
      "conversation-1": runtimeStatus(),
    });

    const { result } = renderHook(
      () => useAgentConversationRuntimeStatus("conversation-1"),
      {
        wrapper: createWrapper(),
      },
    );

    await waitFor(() => {
      expect(result.current.data?.conversationId).toBe("conversation-1");
    });
    expect(mockGetAgentConversationRuntimeStatuses).toHaveBeenCalledTimes(1);

    emitEvent("agent:run_completed", {
      conversation_id: "review-conversation-1",
      context_type: "project",
      context_id: "review-conversation-1",
    });

    await waitFor(() => {
      expect(mockGetAgentConversationRuntimeStatuses).toHaveBeenCalledTimes(2);
    });
  });

  it("refreshes lifecycle starts that may introduce uncached child conversations", async () => {
    mockGetAgentConversationRuntimeStatuses.mockResolvedValue({
      "conversation-1": runtimeStatus({
        items: [],
      }),
    });

    const { result } = renderHook(
      () => useAgentConversationRuntimeStatus("conversation-1"),
      {
        wrapper: createWrapper(),
      },
    );

    await waitFor(() => {
      expect(result.current.data?.conversationId).toBe("conversation-1");
    });
    expect(mockGetAgentConversationRuntimeStatuses).toHaveBeenCalledTimes(1);

    emitEvent("agent:run_started", {
      run_id: "run-1",
      conversation_id: "review-conversation-2",
      context_type: "project",
      context_id: "review-conversation-2",
    });

    await waitFor(() => {
      expect(mockGetAgentConversationRuntimeStatuses).toHaveBeenCalledTimes(2);
    });
  });

  it("does not fail open for unknown child run starts when unknown runtime discovery is disabled", async () => {
    mockGetAgentConversationRuntimeStatuses.mockResolvedValue({
      "conversation-1": runtimeStatus({
        items: [],
      }),
    });

    const { result } = renderHook(
      () =>
        useAgentConversationRuntimeStatus("conversation-1", {
          invalidateUnknownRuntimeIds: false,
          mirrorToVisibleChatStatus: false,
        }),
      {
        wrapper: createWrapper(),
      },
    );

    await waitFor(() => {
      expect(result.current.data?.conversationId).toBe("conversation-1");
    });
    expect(mockGetAgentConversationRuntimeStatuses).toHaveBeenCalledTimes(1);

    emitEvent("agent:run_started", {
      run_id: "run-1",
      conversation_id: "review-conversation-2",
      context_type: "project",
      context_id: "review-conversation-2",
    });

    await new Promise((resolve) => window.setTimeout(resolve, 0));

    expect(mockGetAgentConversationRuntimeStatuses).toHaveBeenCalledTimes(1);
  });

  it("still refreshes workspace-owned run starts when unknown runtime discovery is disabled", async () => {
    mockGetAgentConversationRuntimeStatuses.mockResolvedValue({
      "conversation-1": runtimeStatus({
        items: [],
      }),
    });

    const { result } = renderHook(
      () =>
        useAgentConversationRuntimeStatus("conversation-1", {
          invalidateUnknownRuntimeIds: false,
          mirrorToVisibleChatStatus: false,
        }),
      {
        wrapper: createWrapper(),
      },
    );

    await waitFor(() => {
      expect(result.current.data?.conversationId).toBe("conversation-1");
    });
    expect(mockGetAgentConversationRuntimeStatuses).toHaveBeenCalledTimes(1);

    emitEvent("agent:run_started", {
      run_id: "run-1",
      conversation_id: "conversation-1",
      context_type: "project",
      context_id: "conversation-1",
    });

    await waitFor(() => {
      expect(mockGetAgentConversationRuntimeStatuses).toHaveBeenCalledTimes(2);
    });
  });

  it("refreshes task status changes that may introduce uncached task runtime", async () => {
    mockGetAgentConversationRuntimeStatuses.mockResolvedValue({
      "conversation-1": runtimeStatus({
        items: [],
      }),
    });

    const { result } = renderHook(
      () => useAgentConversationRuntimeStatus("conversation-1"),
      {
        wrapper: createWrapper(),
      },
    );

    await waitFor(() => {
      expect(result.current.data?.conversationId).toBe("conversation-1");
    });
    expect(mockGetAgentConversationRuntimeStatuses).toHaveBeenCalledTimes(1);

    emitEvent("task:status_changed", {
      task_id: "task-2",
      old_status: "ready",
      new_status: "executing",
    });

    await waitFor(() => {
      expect(mockGetAgentConversationRuntimeStatuses).toHaveBeenCalledTimes(2);
    });
  });
});
