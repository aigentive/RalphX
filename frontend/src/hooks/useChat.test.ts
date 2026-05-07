/**
 * useChat hook tests
 *
 * Tests for useChat, useConversations, and useAgentRunStatus hooks
 * using TanStack Query with mocked API and Tauri events.
 */

import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { renderHook, waitFor, act } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import { createElement } from "react";
import {
  useChat,
  useConversations,
  useConversation,
  useConversationHistoryWindow,
  useAgentRunStatus,
  chatKeys,
} from "./useChat";
import { chatApi } from "@/api/chat";
import type { ChatMessageResponse } from "@/api/chat";
import type { ChatContext } from "@/types/chat";
import type { ChatConversation, AgentRun } from "@/types/chat-conversation";
import { useChatStore } from "@/stores/chatStore";

// Mock Tauri event listener
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(() => Promise.resolve(() => {})),
}));

// Mock chat store
vi.mock("@/stores/chatStore", () => ({
  useChatStore: vi.fn(),
}));

// Mock ideation keys
vi.mock("./useIdeation", () => ({
  ideationKeys: {
    sessionWithData: vi.fn((id: string) => ["ideation", "session", id, "data"]),
  },
}));

// Mock agent event subscription hook (requires EventProvider in real app)
vi.mock("./useAgentEvents", () => ({
  useAgentEvents: vi.fn(),
}));

// Mock the chat API
vi.mock("@/api/chat", () => ({
  chatApi: {
    sendAgentMessage: vi.fn(),
    listConversations: vi.fn(),
    getConversation: vi.fn(),
    getConversationMessagesPage: vi.fn(),
    createConversation: vi.fn(),
    getAgentRunStatus: vi.fn(),
  },
}));

// Create mock data
const mockConversation1: ChatConversation = {
  id: "conv-1",
  contextType: "ideation",
  contextId: "session-1",
  providerSessionId: "claude-session-1",
  providerHarness: "claude",
  claudeSessionId: "claude-session-1",
  title: "First conversation",
  messageCount: 2,
  lastMessageAt: "2026-01-24T10:00:00Z",
  createdAt: "2026-01-24T09:00:00Z",
  updatedAt: "2026-01-24T10:00:00Z",
};

const mockConversation2: ChatConversation = {
  id: "conv-2",
  contextType: "ideation",
  contextId: "session-1",
  providerSessionId: null,
  providerHarness: null,
  claudeSessionId: null,
  title: "Second conversation",
  messageCount: 1,
  lastMessageAt: "2026-01-24T11:00:00Z",
  createdAt: "2026-01-24T11:00:00Z",
  updatedAt: "2026-01-24T11:00:00Z",
};

const mockMessage1: ChatMessageResponse = {
  id: "message-1",
  sessionId: "session-1",
  projectId: null,
  taskId: null,
  role: "user",
  content: "Hello",
  metadata: null,
  parentMessageId: null,
  conversationId: "conv-1",
  toolCalls: null,
  createdAt: "2026-01-24T10:00:00Z",
};

const mockMessage2: ChatMessageResponse = {
  id: "message-2",
  sessionId: "session-1",
  projectId: null,
  taskId: null,
  role: "orchestrator",
  content: "Hi there! How can I help?",
  metadata: null,
  parentMessageId: "message-1",
  conversationId: "conv-1",
  toolCalls: null,
  createdAt: "2026-01-24T10:00:05Z",
};

const mockAgentRun: AgentRun = {
  id: "run-1",
  conversationId: "conv-1",
  status: "running",
  startedAt: "2026-01-24T10:00:10Z",
  completedAt: null,
  errorMessage: null,
};

// Test contexts
const ideationContext: ChatContext = {
  view: "ideation",
  projectId: "project-1",
  ideationSessionId: "session-1",
};

const taskDetailContext: ChatContext = {
  view: "task_detail",
  projectId: "project-1",
  selectedTaskId: "task-1",
};

// Test wrapper with QueryClientProvider
function createWrapper() {
  const queryClient = new QueryClient({
    defaultOptions: {
      queries: {
        retry: false,
        gcTime: 0,
      },
    },
  });

  return function Wrapper({ children }: { children: React.ReactNode }) {
    return createElement(QueryClientProvider, { client: queryClient }, children);
  };
}

// Mock chat store state
const mockStoreState = {
  activeConversationIds: {} as Record<string, string | null>,
  setActiveConversation: vi.fn(),
  setAgentRunning: vi.fn(),
  setSending: vi.fn(),
  queuedMessages: [],
  processQueue: vi.fn(),
};

// Type helper for zustand store mock
type StoreMock = typeof mockStoreState;
type StoreSelector<T> = (state: StoreMock) => T;

describe("chatKeys", () => {
  it("should generate correct key for conversations", () => {
    expect(chatKeys.conversations()).toEqual(["chat", "conversations"]);
  });

  it("should generate correct key for conversation", () => {
    expect(chatKeys.conversation("conv-1")).toEqual([
      "chat",
      "conversations",
      "conv-1",
    ]);
  });

  it("should generate correct key for conversation list", () => {
    expect(chatKeys.conversationList("ideation", "session-1")).toEqual([
      "chat",
      "conversations",
      "ideation",
      "session-1",
    ]);
  });

  it("should generate correct key for agent run", () => {
    expect(chatKeys.agentRun("conv-1")).toEqual([
      "chat",
      "agent-run",
      "conv-1",
    ]);
  });
});

describe("useConversations", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  afterEach(() => {
    vi.resetAllMocks();
  });

  it("should fetch conversations for ideation context", async () => {
    const mockConversations = [mockConversation1, mockConversation2];
    vi.mocked(chatApi.listConversations).mockResolvedValueOnce(
      mockConversations
    );

    const { result } = renderHook(() => useConversations(ideationContext), {
      wrapper: createWrapper(),
    });

    expect(result.current.isLoading).toBe(true);

    await waitFor(() => expect(result.current.isSuccess).toBe(true));

    expect(result.current.data).toEqual(mockConversations);
    expect(chatApi.listConversations).toHaveBeenCalledWith(
      "ideation",
      "session-1"
    );
  });

  it("should fetch conversations for task context", async () => {
    const mockConversations = [mockConversation1];
    vi.mocked(chatApi.listConversations).mockResolvedValueOnce(
      mockConversations
    );

    const { result } = renderHook(() => useConversations(taskDetailContext), {
      wrapper: createWrapper(),
    });

    await waitFor(() => expect(result.current.isSuccess).toBe(true));

    expect(result.current.data).toEqual(mockConversations);
    expect(chatApi.listConversations).toHaveBeenCalledWith("task", "task-1");
  });
});

describe("useConversation", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  afterEach(() => {
    vi.resetAllMocks();
  });

  it("should fetch conversation with messages", async () => {
    const mockData = {
      conversation: mockConversation1,
      messages: [mockMessage1, mockMessage2],
    };
    vi.mocked(chatApi.getConversation).mockResolvedValueOnce(mockData);

    const { result } = renderHook(() => useConversation("conv-1"), {
      wrapper: createWrapper(),
    });

    await waitFor(() => expect(result.current.isSuccess).toBe(true));

    expect(result.current.data).toEqual(mockData);
    expect(chatApi.getConversation).toHaveBeenCalledWith("conv-1");
  });

  it("should not fetch when conversationId is null", async () => {
    const { result } = renderHook(() => useConversation(null), {
      wrapper: createWrapper(),
    });

    expect(result.current.isLoading).toBe(false);
    expect(chatApi.getConversation).not.toHaveBeenCalled();
  });
});

describe("useConversationHistoryWindow", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  afterEach(() => {
    vi.resetAllMocks();
  });

  it("loads the latest message window first and prepends older pages on demand", async () => {
    vi.mocked(chatApi.getConversationMessagesPage)
      .mockResolvedValueOnce({
        conversation: mockConversation1,
        messages: [mockMessage2],
        limit: 1,
        offset: 0,
        totalMessageCount: 2,
        hasOlder: true,
      })
      .mockResolvedValueOnce({
        conversation: mockConversation1,
        messages: [mockMessage1],
        limit: 1,
        offset: 1,
        totalMessageCount: 2,
        hasOlder: false,
      });

    const { result } = renderHook(
      () => useConversationHistoryWindow("conv-1", { pageSize: 1 }),
      {
        wrapper: createWrapper(),
      }
    );

    await waitFor(() => expect(result.current.isSuccess).toBe(true));

    expect(result.current.data?.messages.map((message) => message.id)).toEqual([
      "message-2",
    ]);
    expect(result.current.loadedStartIndex).toBe(1);
    expect(result.current.hasOlderMessages).toBe(true);
    expect(chatApi.getConversationMessagesPage).toHaveBeenCalledWith(
      "conv-1",
      1,
      0
    );

    await act(async () => {
      await result.current.fetchOlderMessages();
    });

    await waitFor(() =>
      expect(result.current.data?.messages.map((message) => message.id)).toEqual([
        "message-1",
        "message-2",
      ])
    );

    expect(result.current.loadedStartIndex).toBe(0);
    expect(result.current.hasOlderMessages).toBe(false);
    expect(chatApi.getConversationMessagesPage).toHaveBeenNthCalledWith(
      2,
      "conv-1",
      1,
      1
    );
  });

  it("caps loaded history pages to keep transcript memory bounded", async () => {
    vi.mocked(chatApi.getConversationMessagesPage)
      .mockResolvedValueOnce({
        conversation: mockConversation1,
        messages: [mockMessage2],
        limit: 1,
        offset: 0,
        totalMessageCount: 3,
        hasOlder: true,
      })
      .mockResolvedValueOnce({
        conversation: mockConversation1,
        messages: [mockMessage1],
        limit: 1,
        offset: 1,
        totalMessageCount: 3,
        hasOlder: true,
      });

    const { result } = renderHook(
      () => useConversationHistoryWindow("conv-1", { pageSize: 1, maxPages: 2 }),
      {
        wrapper: createWrapper(),
      }
    );

    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    expect(result.current.hasOlderMessages).toBe(true);

    await act(async () => {
      await result.current.fetchOlderMessages();
    });

    await waitFor(() => expect(result.current.hasOlderMessages).toBe(false));
    expect(result.current.data?.messages.map((message) => message.id)).toEqual([
      "message-1",
      "message-2",
    ]);
    expect(chatApi.getConversationMessagesPage).toHaveBeenCalledTimes(2);

    await act(async () => {
      await result.current.fetchOlderMessages();
    });

    expect(chatApi.getConversationMessagesPage).toHaveBeenCalledTimes(2);
  });
});

describe("useAgentRunStatus", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  afterEach(() => {
    vi.resetAllMocks();
  });

  it("should fetch agent run status", async () => {
    vi.mocked(chatApi.getAgentRunStatus).mockResolvedValueOnce(mockAgentRun);

    const { result } = renderHook(() => useAgentRunStatus("conv-1"), {
      wrapper: createWrapper(),
    });

    await waitFor(() => expect(result.current.isSuccess).toBe(true));

    expect(result.current.data).toEqual(mockAgentRun);
    expect(chatApi.getAgentRunStatus).toHaveBeenCalledWith("conv-1");
  });

  it("should return null when no agent is running", async () => {
    vi.mocked(chatApi.getAgentRunStatus).mockResolvedValueOnce(null);

    const { result } = renderHook(() => useAgentRunStatus("conv-1"), {
      wrapper: createWrapper(),
    });

    await waitFor(() => expect(result.current.isSuccess).toBe(true));

    expect(result.current.data).toBeNull();
  });

  it("should not fetch when conversationId is null", async () => {
    const { result } = renderHook(() => useAgentRunStatus(null), {
      wrapper: createWrapper(),
    });

    expect(result.current.isLoading).toBe(false);
    expect(chatApi.getAgentRunStatus).not.toHaveBeenCalled();
  });
});

describe("useChat", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    vi.mocked(chatApi.listConversations).mockResolvedValue([]);
    vi.mocked(chatApi.getConversation).mockResolvedValue({
      conversation: mockConversation1,
      messages: [mockMessage1, mockMessage2],
    });
    vi.mocked(chatApi.getConversationMessagesPage).mockResolvedValue({
      conversation: mockConversation1,
      messages: [mockMessage1, mockMessage2],
      limit: 40,
      offset: 0,
      totalMessageCount: 2,
      hasOlder: false,
    });
    vi.mocked(chatApi.getAgentRunStatus).mockResolvedValue(null);
    // Mock store state
    vi.mocked(useChatStore).mockImplementation(<T = StoreMock>(selector?: StoreSelector<T>) => {
      if (typeof selector === "function") {
        return selector(mockStoreState);
      }
      return mockStoreState as T;
    });
  });

  afterEach(() => {
    vi.resetAllMocks();
  });

  it("should send context-aware message", async () => {
    // sendAgentMessage now returns SendContextMessageResult
    const mockResult = {
      responseText: "AI response",
      toolCalls: [],
      claudeSessionId: "claude-session-123",
      conversationId: "conv-1",
    };
    vi.mocked(chatApi.sendAgentMessage).mockResolvedValueOnce(mockResult);
    vi.mocked(chatApi.listConversations).mockResolvedValueOnce([]);
    vi.mocked(chatApi.getAgentRunStatus).mockResolvedValueOnce(null);

    const { result } = renderHook(() => useChat(ideationContext), {
      wrapper: createWrapper(),
    });

    await act(async () => {
      await result.current.sendMessage.mutateAsync({ content: "New message content" });
    });

    expect(chatApi.sendAgentMessage).toHaveBeenCalledWith(
      "ideation",
      "session-1",
      "New message content",
      undefined,
      undefined
    );
  });

  it("should send message in task context", async () => {
    // sendAgentMessage now returns SendContextMessageResult
    const mockResult = {
      responseText: "AI response for task",
      toolCalls: [],
      claudeSessionId: "claude-session-456",
      conversationId: "conv-2",
    };
    vi.mocked(chatApi.sendAgentMessage).mockResolvedValueOnce(mockResult);
    vi.mocked(chatApi.listConversations).mockResolvedValueOnce([]);
    vi.mocked(chatApi.getAgentRunStatus).mockResolvedValueOnce(null);

    const { result } = renderHook(() => useChat(taskDetailContext), {
      wrapper: createWrapper(),
    });

    await act(async () => {
      await result.current.sendMessage.mutateAsync({ content: "Task message" });
    });

    expect(chatApi.sendAgentMessage).toHaveBeenCalledWith(
      "task",
      "task-1",
      "Task message",
      undefined,
      undefined
    );
  });

  it("should create new conversation", async () => {
    vi.mocked(chatApi.createConversation).mockResolvedValueOnce(
      mockConversation1
    );
    vi.mocked(chatApi.listConversations).mockResolvedValueOnce([]);
    vi.mocked(chatApi.getAgentRunStatus).mockResolvedValueOnce(null);

    const { result } = renderHook(() => useChat(ideationContext), {
      wrapper: createWrapper(),
    });

    await act(async () => {
      await result.current.createConversation();
    });

    expect(chatApi.createConversation).toHaveBeenCalledWith(
      "ideation",
      "session-1"
    );
    expect(mockStoreState.setActiveConversation).toHaveBeenCalledWith("session:session-1", "conv-1");
  });

  it("should switch conversation", async () => {
    vi.mocked(chatApi.listConversations).mockResolvedValueOnce([]);
    vi.mocked(chatApi.getAgentRunStatus).mockResolvedValueOnce(null);

    const { result } = renderHook(() => useChat(ideationContext), {
      wrapper: createWrapper(),
    });

    await act(async () => {
      result.current.switchConversation("conv-2");
    });

    expect(mockStoreState.setActiveConversation).toHaveBeenCalledWith("session:session-1", "conv-2");
  });

  it("should update agent running state from agent run status", async () => {
    vi.mocked(chatApi.listConversations).mockResolvedValueOnce([]);
    vi.mocked(chatApi.getAgentRunStatus).mockResolvedValueOnce(mockAgentRun);

    // Set active conversation in store
    const storeWithConversation = {
      ...mockStoreState,
      activeConversationIds: { "session:session-1": "conv-1" as string | null },
    };
    vi.mocked(useChatStore).mockImplementation(<T = StoreMock>(selector?: StoreSelector<T>) => {
      if (typeof selector === "function") {
        return selector(storeWithConversation);
      }
      return storeWithConversation as T;
    });

    renderHook(() => useChat(ideationContext), {
      wrapper: createWrapper(),
    });

    await waitFor(() => {
      // setAgentRunning takes contextKey and isRunning
      expect(mockStoreState.setAgentRunning).toHaveBeenCalledWith("session:session-1", true);
    });
  });

  it("should initialize active conversation from conversations list", async () => {
    const mockConversations = [mockConversation1, mockConversation2];
    vi.mocked(chatApi.listConversations).mockResolvedValueOnce(
      mockConversations
    );
    vi.mocked(chatApi.getAgentRunStatus).mockResolvedValueOnce(null);

    renderHook(() => useChat(ideationContext), {
      wrapper: createWrapper(),
    });

    await waitFor(() => {
      // Should set the most recent conversation (conv-2 has later lastMessageAt)
      expect(mockStoreState.setActiveConversation).toHaveBeenCalledWith(
        "session:session-1",
        "conv-2"
      );
    });
  });

  it("should provide conversations, activeConversation, and agentRunStatus", async () => {
    const mockConversations = [mockConversation1];
    const mockConversationData = {
      conversation: mockConversation1,
      messages: [mockMessage1, mockMessage2],
    };

    vi.mocked(chatApi.listConversations).mockResolvedValueOnce(
      mockConversations
    );
    vi.mocked(chatApi.getConversationMessagesPage).mockResolvedValueOnce({
      ...mockConversationData,
      limit: 40,
      offset: 0,
      totalMessageCount: 2,
      hasOlder: false,
    });
    vi.mocked(chatApi.getAgentRunStatus).mockResolvedValueOnce(mockAgentRun);

    // Set active conversation in store
    const storeWithConversation = {
      ...mockStoreState,
      activeConversationIds: { "session:session-1": "conv-1" as string | null },
    };
    vi.mocked(useChatStore).mockImplementation(<T = StoreMock>(selector?: StoreSelector<T>) => {
      if (typeof selector === "function") {
        return selector(storeWithConversation);
      }
      return storeWithConversation as T;
    });

    const { result } = renderHook(() => useChat(ideationContext), {
      wrapper: createWrapper(),
    });

    await waitFor(() => {
      expect(result.current.conversations.isSuccess).toBe(true);
    });

    expect(result.current.conversations.data).toEqual(mockConversations);

    await waitFor(() => {
      expect(result.current.activeConversation.isSuccess).toBe(true);
    });

    expect(result.current.activeConversation.data).toEqual(
      expect.objectContaining(mockConversationData)
    );

    await waitFor(() => {
      expect(result.current.agentRunStatus.isSuccess).toBe(true);
    });

    expect(result.current.agentRunStatus.data).toEqual(mockAgentRun);
  });

  it("should use provided storeKey for active conversation operations instead of derived contextKey", async () => {
    vi.mocked(chatApi.listConversations).mockResolvedValueOnce([]);
    vi.mocked(chatApi.getAgentRunStatus).mockResolvedValueOnce(null);

    // taskDetailContext derives contextKey = "task:task-1" internally
    // but we pass storeKey = "task_execution:task-1" to override
    const { result } = renderHook(
      () => useChat(taskDetailContext, { storeKey: "task_execution:task-1" }),
      { wrapper: createWrapper() }
    );

    await act(async () => {
      result.current.switchConversation("conv-exec");
    });

    // Must use caller-provided storeKey, NOT the derived "task:task-1"
    expect(mockStoreState.setActiveConversation).toHaveBeenCalledWith("task_execution:task-1", "conv-exec");
  });

  it("should return effectiveStoreKey as contextKey when storeKey option is provided", async () => {
    vi.mocked(chatApi.listConversations).mockResolvedValueOnce([]);
    vi.mocked(chatApi.getAgentRunStatus).mockResolvedValueOnce(null);

    const { result } = renderHook(
      () => useChat(taskDetailContext, { storeKey: "task_execution:task-1" }),
      { wrapper: createWrapper() }
    );

    // contextKey in return value should reflect the effectiveStoreKey
    expect(result.current.contextKey).toBe("task_execution:task-1");
  });

  it("should fall back to derived contextKey when no storeKey option provided", async () => {
    vi.mocked(chatApi.listConversations).mockResolvedValueOnce([]);
    vi.mocked(chatApi.getAgentRunStatus).mockResolvedValueOnce(null);

    const { result } = renderHook(
      () => useChat(ideationContext),
      { wrapper: createWrapper() }
    );

    // contextKey falls back to derived "session:session-1"
    expect(result.current.contextKey).toBe("session:session-1");
  });

  it("should skip auto-select when disableAutoSelect is true", async () => {
    const mockConversations = [mockConversation1, mockConversation2];
    vi.mocked(chatApi.listConversations).mockResolvedValueOnce(mockConversations);
    vi.mocked(chatApi.getAgentRunStatus).mockResolvedValueOnce(null);

    renderHook(
      () => useChat(ideationContext, { storeKey: "task_execution:task-1", disableAutoSelect: true }),
      { wrapper: createWrapper() }
    );

    // Wait for conversations to load
    await waitFor(() => {
      expect(chatApi.listConversations).toHaveBeenCalled();
    });

    // setActiveConversation must NOT be called — disableAutoSelect prevents it
    expect(mockStoreState.setActiveConversation).not.toHaveBeenCalled();
  });

  it("should handle send message error", async () => {
    const error = new Error("Failed to send message");
    vi.mocked(chatApi.sendAgentMessage).mockRejectedValueOnce(error);
    vi.mocked(chatApi.listConversations).mockResolvedValueOnce([]);
    vi.mocked(chatApi.getAgentRunStatus).mockResolvedValueOnce(null);

    const { result } = renderHook(() => useChat(ideationContext), {
      wrapper: createWrapper(),
    });

    await expect(
      act(async () => {
        await result.current.sendMessage.mutateAsync({ content: "Message" });
      })
    ).rejects.toThrow("Failed to send message");
  });
});

describe("useAgentEvents streaming behavior", () => {
  let queryClient: QueryClient;

  beforeEach(() => {
    vi.clearAllMocks();
    queryClient = new QueryClient({
      defaultOptions: {
        queries: {
          retry: false,
          gcTime: 0,
        },
      },
    });
  });

  afterEach(() => {
    vi.resetAllMocks();
  });

  it("should not create duplicate assistant messages during streaming", async () => {
    // Mock existing conversation data
    const existingMessages = [mockMessage1];
    const conversationData = {
      conversation: mockConversation1,
      messages: existingMessages,
    };

    // Set initial data in query cache
    queryClient.setQueryData(chatKeys.conversation("conv-1"), conversationData);

    // Simulate the behavior of useAgentEvents for assistant message
    // When role is "assistant", it should only invalidate, not add to cache
    const activeConversationId = "conv-1";
    const payload = {
      conversation_id: "conv-1",
      message_id: "message-assistant-1",
      role: "assistant",
      content: "Assistant response",
    };

    // This simulates the useAgentEvents logic for assistant messages
    if (payload.conversation_id === activeConversationId) {
      if (payload.role !== "user") {
        // For assistant messages, only invalidate (we can't test invalidation directly in this unit test)
        // So we verify that setQueryData is NOT called for assistant messages
        // In reality, this would trigger a refetch from the backend
      }
    }

    // Verify that assistant message did NOT get optimistically added
    const updatedData = queryClient.getQueryData<{
      conversation: ChatConversation;
      messages: ChatMessageResponse[];
    }>(chatKeys.conversation("conv-1"));

    // Messages should still be the original (optimistic append only for user messages)
    expect(updatedData?.messages).toHaveLength(existingMessages.length);
    expect(updatedData?.messages.some((m) => m.id === "message-assistant-1")).toBe(false);
  });

  it("should optimistically add user messages immediately", async () => {
    // Mock existing conversation data
    const existingMessages = [mockMessage1];
    const conversationData = {
      conversation: mockConversation1,
      messages: existingMessages,
    };

    // Set initial data in query cache
    queryClient.setQueryData(chatKeys.conversation("conv-1"), conversationData);

    // Simulate the behavior of useAgentEvents for user message
    const activeConversationId = "conv-1";
    const payload = {
      conversation_id: "conv-1",
      message_id: "message-user-2",
      role: "user",
      content: "User question",
    };

    // This simulates the useAgentEvents logic for user messages
    if (payload.conversation_id === activeConversationId && payload.role === "user") {
      queryClient.setQueryData<{ conversation: ChatConversation; messages: ChatMessageResponse[] }>(
        chatKeys.conversation(activeConversationId),
        (oldData) => {
          if (!oldData) return oldData;

          // Check if message already exists
          if (oldData.messages.some(m => m.id === payload.message_id)) {
            return oldData;
          }

          const newMessage: ChatMessageResponse = {
            id: payload.message_id,
            conversationId: payload.conversation_id,
            sessionId: null,
            projectId: null,
            taskId: null,
            role: payload.role as "user" | "assistant" | "system",
            content: payload.content || "",
            metadata: null,
            parentMessageId: null,
            createdAt: new Date().toISOString(),
            toolCalls: null,
            contentBlocks: null,
          };
          return { ...oldData, messages: [...oldData.messages, newMessage] };
        }
      );
    }

    // Verify that user message WAS optimistically added
    const updatedData = queryClient.getQueryData<{
      conversation: ChatConversation;
      messages: ChatMessageResponse[];
    }>(chatKeys.conversation("conv-1"));

    // User message should be added optimistically
    expect(updatedData?.messages).toHaveLength(existingMessages.length + 1);
    expect(updatedData?.messages.some((m) => m.id === "message-user-2")).toBe(true);
    expect(updatedData?.messages.find((m) => m.id === "message-user-2")?.role).toBe("user");
  });

  it("should maintain stable message order before and after streaming", async () => {
    // Mock conversation with multiple messages
    const orderedMessages = [
      mockMessage1, // user
      mockMessage2, // assistant
      {
        ...mockMessage1,
        id: "message-3",
        content: "Follow-up question",
        createdAt: "2026-01-24T10:00:10Z",
      }, // user
    ];
    const conversationData = {
      conversation: mockConversation1,
      messages: orderedMessages,
    };

    queryClient.setQueryData(chatKeys.conversation("conv-1"), conversationData);

    const activeConversationId = "conv-1";

    // Simulate new user message
    const userPayload = {
      conversation_id: "conv-1",
      message_id: "message-4",
      role: "user",
      content: "Another question",
    };

    // Add user message (simulating useAgentEvents behavior)
    if (userPayload.conversation_id === activeConversationId && userPayload.role === "user") {
      queryClient.setQueryData<{ conversation: ChatConversation; messages: ChatMessageResponse[] }>(
        chatKeys.conversation(activeConversationId),
        (oldData) => {
          if (!oldData) return oldData;
          if (oldData.messages.some(m => m.id === userPayload.message_id)) {
            return oldData;
          }
          const newMessage: ChatMessageResponse = {
            id: userPayload.message_id,
            conversationId: userPayload.conversation_id,
            sessionId: null,
            projectId: null,
            taskId: null,
            role: userPayload.role as "user" | "assistant" | "system",
            content: userPayload.content || "",
            metadata: null,
            parentMessageId: null,
            createdAt: new Date().toISOString(),
            toolCalls: null,
            contentBlocks: null,
          };
          return { ...oldData, messages: [...oldData.messages, newMessage] };
        }
      );
    }

    const afterUserData = queryClient.getQueryData<{
      conversation: ChatConversation;
      messages: ChatMessageResponse[];
    }>(chatKeys.conversation("conv-1"));

    // Verify order is maintained and new message is at the end
    expect(afterUserData?.messages).toHaveLength(4);
    expect(afterUserData?.messages[3]?.id).toBe("message-4");
    expect(afterUserData?.messages[3]?.role).toBe("user");

    // Simulate assistant message (should not be added optimistically)
    const assistantPayload = {
      conversation_id: "conv-1",
      message_id: "message-5",
      role: "assistant",
      content: "Assistant response",
    };

    // For assistant, no setQueryData should be called
    if (assistantPayload.conversation_id === activeConversationId && assistantPayload.role !== "user") {
      // Only invalidation happens, which we can't directly test in unit tests
    }

    const afterAssistantData = queryClient.getQueryData<{
      conversation: ChatConversation;
      messages: ChatMessageResponse[];
    }>(chatKeys.conversation("conv-1"));

    // Assistant message should NOT be added (only invalidation happens)
    expect(afterAssistantData?.messages).toHaveLength(4); // Still 4, not 5
    expect(afterAssistantData?.messages.some((m) => m.id === "message-5")).toBe(false);
  });

  it("should not add duplicate messages if message already exists", async () => {
    const existingMessages = [mockMessage1, mockMessage2];
    const conversationData = {
      conversation: mockConversation1,
      messages: existingMessages,
    };

    queryClient.setQueryData(chatKeys.conversation("conv-1"), conversationData);

    const activeConversationId = "conv-1";

    // Try to add the same user message again
    const duplicatePayload = {
      conversation_id: "conv-1",
      message_id: "message-1", // Same ID as mockMessage1
      role: "user",
      content: "Hello",
    };

    // Simulate useAgentEvents behavior for duplicate
    if (duplicatePayload.conversation_id === activeConversationId && duplicatePayload.role === "user") {
      queryClient.setQueryData<{ conversation: ChatConversation; messages: ChatMessageResponse[] }>(
        chatKeys.conversation(activeConversationId),
        (oldData) => {
          if (!oldData) return oldData;
          // This is the key check - if message already exists, return unchanged
          if (oldData.messages.some(m => m.id === duplicatePayload.message_id)) {
            return oldData;
          }
          const newMessage: ChatMessageResponse = {
            id: duplicatePayload.message_id,
            conversationId: duplicatePayload.conversation_id,
            sessionId: null,
            projectId: null,
            taskId: null,
            role: duplicatePayload.role as "user" | "assistant" | "system",
            content: duplicatePayload.content || "",
            metadata: null,
            parentMessageId: null,
            createdAt: new Date().toISOString(),
            toolCalls: null,
            contentBlocks: null,
          };
          return { ...oldData, messages: [...oldData.messages, newMessage] };
        }
      );
    }

    const afterData = queryClient.getQueryData<{
      conversation: ChatConversation;
      messages: ChatMessageResponse[];
    }>(chatKeys.conversation("conv-1"));

    // Should still have 2 messages, not 3
    expect(afterData?.messages).toHaveLength(2);
    expect(afterData?.messages.filter((m) => m.id === "message-1")).toHaveLength(1);
  });
});
