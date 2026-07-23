/**
 * Gap 2 (frontend): Backfill hydration effect for model chip in execution/review/merge contexts.
 *
 * Tests:
 * (a) agentRunQuery returns modelId → store populated
 * (b) store already populated (live event wins) → no overwrite
 * (c) modelId: null → no write to store
 */

import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, act, waitFor } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { IntegratedChatPanel } from "./IntegratedChatPanel";
import { useChatStore } from "@/stores/chatStore";
import { useUiStore } from "@/stores/uiStore";
import type { AgentRun } from "@/types/chat-conversation";

// ============================================================================
// Hoisted mocks
// ============================================================================

const { useChatMockState } = vi.hoisted(() => {
  const useChatMockState = {
    messages: [] as Array<{ id: string; role: string; content: string; createdAt: string; toolCalls: null; contentBlocks: null }>,
    conversation: null as { contextType: string; contextId: string } | null,
    conversations: [] as Array<{ id: string }>,
  };
  return { useChatMockState };
});

// ============================================================================
// Mocks — same minimal set as IntegratedChatPanel.test.tsx
// ============================================================================

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(() => Promise.resolve(() => {})),
  emit: vi.fn(),
}));

const mockSubscribe = vi.fn(() => () => {});
vi.mock("@/providers/EventProvider", () => ({
  useEventBus: () => ({ subscribe: mockSubscribe, emit: vi.fn() }),
  EventProvider: ({ children }: { children: React.ReactNode }) => children,
}));

vi.mock("@/hooks/useChat", () => ({
  useChat: () => ({
    messages: {
      data: { messages: useChatMockState.messages, conversation: useChatMockState.conversation },
      isLoading: false,
    },
    sendMessage: { mutateAsync: vi.fn(), isPending: false },
    conversations: { data: useChatMockState.conversations, isLoading: false },
    switchConversation: vi.fn(),
    createConversation: vi.fn(),
  }),
  useConversation: () => ({ data: undefined, isLoading: false, error: null }),
  useConversationHistoryWindow: () => ({
    data: undefined,
    isLoading: false,
    isFetchingOlderMessages: false,
    hasOlderMessages: false,
    loadedStartIndex: 0,
    fetchOlderMessages: vi.fn(),
  }),
  useConversationTimelineWindow: () => ({
    data: undefined,
    isLoading: false,
    isFetchingOlderMessages: false,
    hasOlderMessages: false,
    loadedStartIndex: 0,
    fetchOlderMessages: vi.fn(),
  }),
  getCachedConversationMessages: () => useChatMockState.messages,
  isOptimisticConversationId: (conversationId: string | null | undefined) =>
    typeof conversationId === "string" && conversationId.startsWith("optimistic-"),
  chatKeys: {
    all: ["chat"],
    conversationList: (type: string, id: string) => ["chat", "conversations", type, id],
    conversation: (id: string) => ["chat", "conversation", id],
    conversationTimeline: (id: string) => ["chat", "conversation", id, "timeline"],
    agentRun: (id: string) => ["chat", "agentRun", id],
  },
}));

let mockTasks: Array<{ id: string; internalStatus: string }> = [];
vi.mock("@/hooks/useTasks", () => ({
  useTasks: () => ({ data: mockTasks }),
  taskKeys: {
    list: (projectId: string) => ["tasks", projectId],
    detail: (taskId: string) => ["task", taskId],
  },
}));

// mutable mock context — tests override activeConversationId
const mockChatPanelContext = {
  chatContext: { view: "kanban" as const, projectId: "project-1" },
  storeContextKey: "task_execution:task-1",
  currentContextType: "task_execution" as const,
  currentContextId: "task-1",
  activeConversationId: "conv-1" as string | null,
  streamingToolCalls: [],
  setStreamingToolCalls: vi.fn(),
  streamingContentBlocks: [],
  setStreamingContentBlocks: vi.fn(),
  streamingTasks: new Map(),
  setStreamingTasks: vi.fn(),
  isFinalizing: false,
  setIsFinalizing: vi.fn(),
  autoSelectConversation: vi.fn(),
};

vi.mock("@/hooks/useChatPanelContext", () => ({
  useChatPanelContext: () => mockChatPanelContext,
}));

vi.mock("@/hooks/useChatActions", () => ({
  useChatActions: () => ({
    handleSend: vi.fn(),
    handleEditLastQueued: vi.fn(),
    handleDeleteQueuedMessage: vi.fn(),
    handleEditQueuedMessage: vi.fn(),
    handleStopAgent: vi.fn(),
  }),
}));

vi.mock("@/hooks/useChatEvents", () => ({ useChatEvents: vi.fn() }));
vi.mock("@/hooks/useChatRecovery", () => ({ useChatRecovery: vi.fn().mockReturnValue({ isStreamingHydrated: true }) }));
vi.mock("@/hooks/useAgentEvents", () => ({ useAgentEvents: vi.fn() }));

vi.mock("@/hooks/useAskUserQuestion", () => ({
  useAskUserQuestion: () => ({
    activeQuestion: null,
    answeredQuestion: undefined,
    submitAnswer: vi.fn().mockResolvedValue(true),
    dismissQuestion: vi.fn(),
    clearAnswered: vi.fn(),
    isLoading: false,
  }),
}));

vi.mock("@/hooks/useQuestionInput", () => ({
  useQuestionInput: () => ({
    selectedOptions: new Set(),
    questionInputValue: "",
    setQuestionInputValue: vi.fn(),
    handleChipClick: vi.fn(),
    handleMatchedOptions: vi.fn(),
    handleQuestionSend: vi.fn(),
    handleQuestionSkip: vi.fn(),
    handleQuestionOptionSubmit: vi.fn(),
  }),
}));

vi.mock("@/hooks/useChatAttachments", () => ({
  useChatAttachments: () => ({
    attachments: [],
    uploadFiles: vi.fn().mockResolvedValue([]),
    removeAttachment: vi.fn().mockResolvedValue(undefined),
    clearAttachments: vi.fn(),
    uploading: false,
    uploadProgress: [],
  }),
}));

// chatApi mock — getAgentRunStatus is overridden per test
const mockGetAgentRunStatus = vi.fn<() => Promise<AgentRun | null>>();

vi.mock("@/api/chat", () => ({
  chatApi: {
    listConversations: vi.fn().mockResolvedValue([]),
    getAgentRunStatus: (...args: unknown[]) => mockGetAgentRunStatus(...args),
    sendAgentMessage: vi.fn().mockResolvedValue({ conversationId: "conv-1" }),
  },
  stopAgent: vi.fn().mockResolvedValue(true),
}));

vi.mock("@/components/recovery/RecoveryPromptDialog", () => ({
  RecoveryPromptDialog: () => null,
}));

// ============================================================================
// Helpers
// ============================================================================

function buildAgentRun(overrides: Partial<AgentRun> = {}): AgentRun {
  return {
    id: "run-1",
    conversationId: "conv-1",
    status: "completed",
    startedAt: "2024-01-01T00:00:00.000Z",
    completedAt: "2024-01-01T01:00:00.000Z",
    errorMessage: null,
    modelId: "claude-sonnet-4-6",
    modelLabel: "Sonnet 4.6",
    ...overrides,
  };
}

function createTestQueryClient() {
  return new QueryClient({
    defaultOptions: {
      queries: { retry: false, gcTime: 0, staleTime: 0 },
      mutations: { retry: false },
    },
  });
}

function TestWrapper({ children, queryClient }: { children: React.ReactNode; queryClient: QueryClient }) {
  return (
    <QueryClientProvider client={queryClient}>
      {children}
    </QueryClientProvider>
  );
}

const STORE_KEY = "task_execution:task-1";

// ============================================================================
// Tests
// ============================================================================

describe("IntegratedChatPanel — Gap 2 backfill hydration effect", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockTasks = [];
    useChatMockState.messages = [];
    useChatMockState.conversation = null;
    useChatMockState.conversations = [];

    mockChatPanelContext.storeContextKey = STORE_KEY;
    mockChatPanelContext.currentContextType = "task_execution";
    mockChatPanelContext.currentContextId = "task-1";
    mockChatPanelContext.activeConversationId = "conv-1";

    act(() => {
      useChatStore.setState({ effectiveModel: {} });
      useUiStore.setState({ taskHistoryState: null });
    });
  });

  it("(a) populates store from agentRunQuery when modelId is present and store is empty", async () => {
    mockGetAgentRunStatus.mockResolvedValue(buildAgentRun({
      modelId: "claude-sonnet-4-6",
      modelLabel: "Sonnet 4.6",
    }));

    const queryClient = createTestQueryClient();
    render(
      <TestWrapper queryClient={queryClient}>
        <IntegratedChatPanel projectId="project-1" />
      </TestWrapper>
    );

    await waitFor(() => {
      const stored = useChatStore.getState().effectiveModel[STORE_KEY];
      expect(stored).toEqual({ id: "claude-sonnet-4-6", label: "Sonnet 4.6" });
    });
  });

  it("(b) does not overwrite store when live event already populated it (guard: live event wins)", async () => {
    // Pre-populate store as if a live agent:run_started event already fired
    act(() => {
      useChatStore.getState().setEffectiveModel(STORE_KEY, {
        id: "claude-opus-4-6",
        label: "Opus 4.6 (live)",
      });
    });

    mockGetAgentRunStatus.mockResolvedValue(buildAgentRun({
      modelId: "claude-sonnet-4-6",
      modelLabel: "Sonnet 4.6",
    }));

    const queryClient = createTestQueryClient();
    render(
      <TestWrapper queryClient={queryClient}>
        <IntegratedChatPanel projectId="project-1" />
      </TestWrapper>
    );

    // Wait for query to resolve
    await waitFor(() => expect(mockGetAgentRunStatus).toHaveBeenCalled());

    // Store value from live event must remain untouched
    const stored = useChatStore.getState().effectiveModel[STORE_KEY];
    expect(stored).toEqual({ id: "claude-opus-4-6", label: "Opus 4.6 (live)" });
  });

  it("(c) does not write to store when modelId is null", async () => {
    mockGetAgentRunStatus.mockResolvedValue(buildAgentRun({ modelId: null, modelLabel: null }));

    const queryClient = createTestQueryClient();
    render(
      <TestWrapper queryClient={queryClient}>
        <IntegratedChatPanel projectId="project-1" />
      </TestWrapper>
    );

    await waitFor(() => expect(mockGetAgentRunStatus).toHaveBeenCalled());

    const stored = useChatStore.getState().effectiveModel[STORE_KEY];
    expect(stored).toBeUndefined();
  });

  it("(a+label) falls back to getModelLabel when modelLabel is null but modelId present", async () => {
    mockGetAgentRunStatus.mockResolvedValue(buildAgentRun({
      modelId: "claude-sonnet-4-6",
      modelLabel: null,
    }));

    const queryClient = createTestQueryClient();
    render(
      <TestWrapper queryClient={queryClient}>
        <IntegratedChatPanel projectId="project-1" />
      </TestWrapper>
    );

    await waitFor(() => {
      const stored = useChatStore.getState().effectiveModel[STORE_KEY];
      expect(stored).toBeDefined();
      expect(stored?.id).toBe("claude-sonnet-4-6");
      // label should be non-empty (derived from getModelLabel)
      expect(typeof stored?.label).toBe("string");
      expect(stored!.label.length).toBeGreaterThan(0);
    });
  });
});
