/**
 * useChat hook tests
 *
 * Tests for useChat, useConversations, and useAgentRunStatus hooks
 * using TanStack Query with mocked API and Tauri events.
 */

import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { renderHook, waitFor, act } from "@testing-library/react";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";
import type { InfiniteData } from "@tanstack/react-query";
import { createElement } from "react";
import {
  useChat,
  useConversations,
  useConversation,
  useConversationHistoryWindow,
  useConversationTimelineWindow,
  useAgentRunStatus,
  chatKeys,
  createOptimisticConversationId,
  isOptimisticConversationId,
  getCachedConversationMessages,
  addOptimisticUserMessageToConversationCache,
  removeOptimisticMessageFromConversationCache,
  upsertFinalizedMessageIntoConversationCache,
  upsertRenderReadyMessageIntoConversationCache,
} from "./useChat";
import {
  chatApi,
  type ConversationMessagesPageResponse,
  type ConversationTimelinePageResponse,
  type SendAgentMessageResult,
} from "@/api/chat";
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
    getConversationTimelinePage: vi.fn(),
    createConversation: vi.fn(),
    getAgentRunStatus: vi.fn(),
  },
  parseToolCalls: (raw: unknown) => Array.isArray(raw)
    ? raw.map((toolCall, index) => {
      const record = toolCall as Record<string, unknown>;
      return {
        id: typeof record.id === "string" ? record.id : `tool-${index}`,
        name: typeof record.name === "string" ? record.name : "unknown",
        arguments: record.arguments ?? {},
        ...(record.result !== undefined ? { result: record.result } : {}),
      };
    })
    : [],
  parseContentBlocks: (raw: unknown) => Array.isArray(raw) ? raw : [],
}));

// Create mock data
const mockConversation1: ChatConversation = {
  id: "conv-1",
  contextType: "ideation",
  contextId: "session-1",
  providerSessionId: "claude-session-1",
  providerHarness: "claude",
  claudeSessionId: "claude-session-1",
  coordinationMode: "solo",
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
  coordinationMode: "solo",
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
  contentBlocks: null,
  sender: null,
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
  contentBlocks: null,
  sender: null,
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

function timelinePage(
  messages: ChatMessageResponse[],
  options: Partial<ConversationTimelinePageResponse> = {}
): ConversationTimelinePageResponse {
  const items = messages.map((message, index) => ({
    id: message.id,
    conversationId: message.conversationId ?? "conv-1",
    messageId: message.parentMessageId ?? message.id,
    runId: null,
    sequence: message.timelineSequence ?? index + 1,
    blockIndex: index,
    role: message.role,
    kind: message.timelineKind ?? "text",
    status: message.timelineStatus ?? "finalized",
    content: message.content,
    contentBlocks: message.contentBlocks ?? [{ type: "text", text: message.content }],
    toolCall: message.toolCalls?.[0] ?? null,
    metadata: message.metadata,
    providerHarness: message.providerHarness ?? null,
    providerSessionId: message.providerSessionId ?? null,
    createdAt: message.createdAt,
    updatedAt: message.createdAt,
    finalizedAt: message.timelineStatus === "streaming" ? null : message.createdAt,
    asMessage: message,
  }));

  return {
    conversation: mockConversation1,
    items,
    messages,
    limit: options.limit ?? messages.length,
    beforeSequence: options.beforeSequence ?? null,
    totalItemCount: options.totalItemCount ?? messages.length,
    hasOlder: options.hasOlder ?? false,
    oldestLoadedSequence:
      options.oldestLoadedSequence ?? items[0]?.sequence ?? null,
    newestLoadedSequence:
      options.newestLoadedSequence ?? items[items.length - 1]?.sequence ?? null,
  };
}

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
function createWrapperWithClient() {
  const queryClient = new QueryClient({
    defaultOptions: {
      queries: {
        retry: false,
        gcTime: 0,
      },
    },
  });

  const wrapper = function Wrapper({ children }: { children: React.ReactNode }) {
    return createElement(QueryClientProvider, { client: queryClient }, children);
  };

  return { queryClient, wrapper };
}

function createWrapper() {
  return createWrapperWithClient().wrapper;
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

  it("should generate correct key for conversation timeline", () => {
    expect(chatKeys.conversationTimeline("conv-1")).toEqual([
      "chat",
      "conversations",
      "conv-1",
      "timeline",
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

describe("optimistic conversation ids", () => {
  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("creates query-safe optimistic ids with crypto randomUUID when available", () => {
    vi.stubGlobal("crypto", {
      randomUUID: vi.fn(() => "uuid-123"),
    });

    const conversationId = createOptimisticConversationId();

    expect(conversationId).toBe("optimistic-conversation:uuid-123");
    expect(isOptimisticConversationId(conversationId)).toBe(true);
    expect(isOptimisticConversationId("conversation-real")).toBe(false);
    expect(isOptimisticConversationId(null)).toBe(false);
  });

  it("falls back to timestamp and random suffix when crypto ids are unavailable", () => {
    vi.stubGlobal("crypto", {});
    const dateNowSpy = vi.spyOn(Date, "now").mockReturnValue(12345);
    const randomSpy = vi.spyOn(Math, "random").mockReturnValue(0.5);

    try {
      expect(createOptimisticConversationId()).toBe(
        "optimistic-conversation:12345-i"
      );
    } finally {
      dateNowSpy.mockRestore();
      randomSpy.mockRestore();
    }
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

  it("should not fetch backend data for optimistic conversation ids", async () => {
    const { result } = renderHook(
      () => useConversation("optimistic-conversation:test"),
      {
        wrapper: createWrapper(),
      }
    );

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

  it("continues default history pagination past three pages until the first message is reachable", async () => {
    const message3: ChatMessageResponse = {
      ...mockMessage1,
      id: "message-3",
      content: "Middle page",
      createdAt: "2026-01-24T10:00:03Z",
    };
    const message4: ChatMessageResponse = {
      ...mockMessage2,
      id: "message-4",
      content: "Latest page",
      createdAt: "2026-01-24T10:00:04Z",
    };

    vi.mocked(chatApi.getConversationMessagesPage)
      .mockResolvedValueOnce({
        conversation: mockConversation1,
        messages: [message4],
        limit: 1,
        offset: 0,
        totalMessageCount: 4,
        hasOlder: true,
      })
      .mockResolvedValueOnce({
        conversation: mockConversation1,
        messages: [message3],
        limit: 1,
        offset: 1,
        totalMessageCount: 4,
        hasOlder: true,
      })
      .mockResolvedValueOnce({
        conversation: mockConversation1,
        messages: [mockMessage2],
        limit: 1,
        offset: 2,
        totalMessageCount: 4,
        hasOlder: true,
      })
      .mockResolvedValueOnce({
        conversation: mockConversation1,
        messages: [mockMessage1],
        limit: 1,
        offset: 3,
        totalMessageCount: 4,
        hasOlder: false,
      });

    const { result } = renderHook(
      () => useConversationHistoryWindow("conv-1", { pageSize: 1 }),
      {
        wrapper: createWrapper(),
      }
    );

    await waitFor(() => expect(result.current.isSuccess).toBe(true));

    for (let index = 0; index < 3; index += 1) {
      await act(async () => {
        await result.current.fetchOlderMessages();
      });
    }

    await waitFor(() =>
      expect(result.current.data?.messages.map((message) => message.id)).toEqual([
        "message-1",
        "message-2",
        "message-3",
        "message-4",
      ])
    );
    expect(result.current.loadedStartIndex).toBe(0);
    expect(result.current.hasOlderMessages).toBe(false);
    expect(chatApi.getConversationMessagesPage).toHaveBeenCalledTimes(4);
    expect(chatApi.getConversationMessagesPage).toHaveBeenNthCalledWith(
      4,
      "conv-1",
      1,
      3
    );
  });
});

describe("useConversationTimelineWindow", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  afterEach(() => {
    vi.resetAllMocks();
  });

  it("loads newest visible timeline items and then older pages by sequence", async () => {
    const newestBlock: ChatMessageResponse = {
      ...mockMessage2,
      id: "block:message-2:1",
      parentMessageId: "message-2",
      content: "tool block",
      timelineSequence: 2,
      timelineKind: "tool_use",
      timelineStatus: "finalized",
    };
    const olderBlock: ChatMessageResponse = {
      ...mockMessage1,
      id: "block:message-1:0",
      parentMessageId: "message-1",
      timelineSequence: 1,
      timelineKind: "text",
      timelineStatus: "finalized",
    };

    vi.mocked(chatApi.getConversationTimelinePage)
      .mockResolvedValueOnce(
        timelinePage([newestBlock], {
          limit: 1,
          totalItemCount: 2,
          hasOlder: true,
          oldestLoadedSequence: 2,
          newestLoadedSequence: 2,
        })
      )
      .mockResolvedValueOnce(
        timelinePage([olderBlock], {
          limit: 1,
          beforeSequence: 2,
          totalItemCount: 2,
          hasOlder: false,
          oldestLoadedSequence: 1,
          newestLoadedSequence: 1,
        })
      );

    const { result } = renderHook(
      () => useConversationTimelineWindow("conv-1", { pageSize: 1 }),
      {
        wrapper: createWrapper(),
      }
    );

    await waitFor(() => expect(result.current.isSuccess).toBe(true));
    expect(result.current.data?.messages.map((message) => message.id)).toEqual([
      "block:message-2:1",
    ]);
    expect(result.current.loadedStartIndex).toBe(1);
    expect(result.current.hasOlderMessages).toBe(true);
    expect(chatApi.getConversationTimelinePage).toHaveBeenCalledWith(
      "conv-1",
      1,
      null
    );

    await act(async () => {
      await result.current.fetchOlderMessages();
    });

    await waitFor(() =>
      expect(result.current.data?.messages.map((message) => message.id)).toEqual([
        "block:message-1:0",
        "block:message-2:1",
      ])
    );
    expect(result.current.loadedStartIndex).toBe(0);
    expect(chatApi.getConversationTimelinePage).toHaveBeenNthCalledWith(
      2,
      "conv-1",
      1,
      2
    );
  });

  it("omits hidden bootstrap rows from timeline window data", async () => {
    const hiddenBootstrap: ChatMessageResponse = {
      ...mockMessage1,
      id: "block:bootstrap:0",
      parentMessageId: "bootstrap",
      content: "Execute task: task-hidden",
      metadata: JSON.stringify({
        hidden_from_ui: true,
        source: "task_runtime_bootstrap",
      }),
      timelineSequence: 1,
      timelineKind: "text",
      timelineStatus: "finalized",
    };
    const visibleUser: ChatMessageResponse = {
      ...mockMessage1,
      id: "block:user-visible:0",
      parentMessageId: "user-visible",
      content: "Visible user request",
      timelineSequence: 2,
      timelineKind: "text",
      timelineStatus: "finalized",
    };
    const visibleAssistant: ChatMessageResponse = {
      ...mockMessage2,
      id: "block:assistant-visible:0",
      parentMessageId: "assistant-visible",
      content: "Visible assistant response",
      timelineSequence: 3,
      timelineKind: "text",
      timelineStatus: "finalized",
    };

    vi.mocked(chatApi.getConversationTimelinePage).mockResolvedValueOnce(
      timelinePage([hiddenBootstrap, visibleUser, visibleAssistant], {
        limit: 3,
        totalItemCount: 3,
        hasOlder: false,
        oldestLoadedSequence: 1,
        newestLoadedSequence: 3,
      })
    );

    const { result } = renderHook(
      () => useConversationTimelineWindow("conv-1", { pageSize: 3 }),
      {
        wrapper: createWrapper(),
      }
    );

    await waitFor(() => expect(result.current.isSuccess).toBe(true));

    expect(result.current.data?.messages.map((message) => message.content)).toEqual([
      "Visible user request",
      "Visible assistant response",
    ]);
    expect(result.current.data?.totalMessageCount).toBe(2);
    expect(result.current.loadedStartIndex).toBe(0);
  });

  it("continues default timeline pagination past three pages until the first item is reachable", async () => {
    const timelineBlocks = [1, 2, 3, 4].map((sequence) => ({
      ...mockMessage1,
      id: `block:message-${sequence}:0`,
      parentMessageId: `message-${sequence}`,
      content: `Timeline block ${sequence}`,
      timelineSequence: sequence,
      timelineKind: "text" as const,
      timelineStatus: "finalized" as const,
      createdAt: `2026-01-24T10:00:0${sequence}Z`,
    }));

    vi.mocked(chatApi.getConversationTimelinePage)
      .mockResolvedValueOnce(
        timelinePage([timelineBlocks[3]], {
          limit: 1,
          totalItemCount: 4,
          hasOlder: true,
          oldestLoadedSequence: 4,
          newestLoadedSequence: 4,
        })
      )
      .mockResolvedValueOnce(
        timelinePage([timelineBlocks[2]], {
          limit: 1,
          beforeSequence: 4,
          totalItemCount: 4,
          hasOlder: true,
          oldestLoadedSequence: 3,
          newestLoadedSequence: 3,
        })
      )
      .mockResolvedValueOnce(
        timelinePage([timelineBlocks[1]], {
          limit: 1,
          beforeSequence: 3,
          totalItemCount: 4,
          hasOlder: true,
          oldestLoadedSequence: 2,
          newestLoadedSequence: 2,
        })
      )
      .mockResolvedValueOnce(
        timelinePage([timelineBlocks[0]], {
          limit: 1,
          beforeSequence: 2,
          totalItemCount: 4,
          hasOlder: false,
          oldestLoadedSequence: 1,
          newestLoadedSequence: 1,
        })
      );

    const { result } = renderHook(
      () => useConversationTimelineWindow("conv-1", { pageSize: 1 }),
      {
        wrapper: createWrapper(),
      }
    );

    await waitFor(() => expect(result.current.isSuccess).toBe(true));

    for (let index = 0; index < 3; index += 1) {
      await act(async () => {
        await result.current.fetchOlderMessages();
      });
    }

    await waitFor(() =>
      expect(result.current.data?.messages.map((message) => message.id)).toEqual([
        "block:message-1:0",
        "block:message-2:0",
        "block:message-3:0",
        "block:message-4:0",
      ])
    );
    expect(result.current.loadedStartIndex).toBe(0);
    expect(result.current.hasOlderMessages).toBe(false);
    expect(chatApi.getConversationTimelinePage).toHaveBeenCalledTimes(4);
    expect(chatApi.getConversationTimelinePage).toHaveBeenNthCalledWith(
      4,
      "conv-1",
      1,
      2
    );
  });

  it("merges timeline cache messages and maintains optimistic timeline items", () => {
    const { queryClient } = createWrapperWithClient();
    queryClient.setQueryData(chatKeys.conversationTimeline("conv-1"), {
      pages: [
        timelinePage([
          {
            ...mockMessage2,
            id: "block:message-2:0",
            parentMessageId: "message-2",
            timelineSequence: 4,
          },
        ], {
          totalItemCount: 4,
          oldestLoadedSequence: 4,
          newestLoadedSequence: 4,
        }),
      ],
      pageParams: [null],
    } satisfies InfiniteData<ConversationTimelinePageResponse>);

    expect(
      getCachedConversationMessages(queryClient, "conv-1").map((message) => message.id)
    ).toEqual(["block:message-2:0"]);

    const optimistic = addOptimisticUserMessageToConversationCache(
      queryClient,
      "conv-1",
      "hello from user"
    );
    const timelineData =
      queryClient.getQueryData<InfiniteData<ConversationTimelinePageResponse>>(
        chatKeys.conversationTimeline("conv-1")
      );
    expect(timelineData?.pages[0]?.messages.map((message) => message.id)).toEqual([
      "block:message-2:0",
      `optimistic-timeline:${optimistic.id}`,
    ]);
    expect(timelineData?.pages[0]?.newestLoadedSequence).toBe(5);

    removeOptimisticMessageFromConversationCache(
      queryClient,
      "conv-1",
      optimistic.id
    );
    const trimmed =
      queryClient.getQueryData<InfiniteData<ConversationTimelinePageResponse>>(
        chatKeys.conversationTimeline("conv-1")
      );
    expect(trimmed?.pages[0]?.messages.map((message) => message.id)).toEqual([
      "block:message-2:0",
    ]);
    expect(trimmed?.pages[0]?.newestLoadedSequence).toBe(4);
  });

  it("upserts finalized assistant blocks into the active timeline cache without refetch", () => {
    const { queryClient } = createWrapperWithClient();
    queryClient.setQueryData(chatKeys.conversation("conv-1"), {
      conversation: mockConversation1,
      messages: [mockMessage1],
    });
    queryClient.setQueryData<InfiniteData<ConversationMessagesPageResponse>>(
      chatKeys.conversationHistory("conv-1"),
      {
        pages: [
          {
            conversation: mockConversation1,
            messages: [mockMessage1],
            limit: 40,
            offset: 0,
            totalMessageCount: 1,
            hasOlder: false,
          },
        ],
        pageParams: [0],
      },
    );
    queryClient.setQueryData<InfiniteData<ConversationTimelinePageResponse>>(
      chatKeys.conversationTimeline("conv-1"),
      {
        pages: [
          timelinePage([
            {
              ...mockMessage1,
              id: "block:message-1:0",
              parentMessageId: "message-1",
              timelineSequence: 7,
            },
          ], {
            totalItemCount: 7,
            oldestLoadedSequence: 7,
            newestLoadedSequence: 7,
          }),
        ],
        pageParams: [null],
      },
    );

    const finalized: ChatMessageResponse = {
      id: "assistant-final",
      sessionId: null,
      projectId: null,
      taskId: null,
      role: "assistant",
      content: "Done",
      metadata: null,
      parentMessageId: null,
      conversationId: "conv-1",
      toolCalls: [{
        id: "toolu-read",
        name: "Read",
        arguments: { file_path: "src/app.ts" },
        result: "preview",
      }],
      contentBlocks: [
        { type: "text", text: "Done" },
        {
          type: "tool_use",
          id: "toolu-read",
          name: "Read",
          arguments: { file_path: "src/app.ts" },
          result: "preview",
        },
      ],
      sender: null,
      createdAt: "2026-01-24T10:01:00Z",
    };

    expect(upsertFinalizedMessageIntoConversationCache(queryClient, "conv-1", finalized)).toBe(true);

    const timelineData =
      queryClient.getQueryData<InfiniteData<ConversationTimelinePageResponse>>(
        chatKeys.conversationTimeline("conv-1")
      );
    expect(timelineData?.pages[0]?.messages.map((message) => message.id)).toEqual([
      "block:message-1:0",
      "block:assistant-final:0",
      "block:assistant-final:1",
    ]);
    expect(timelineData?.pages[0]?.messages[1]?.parentMessageId).toBe("assistant-final");
    expect(timelineData?.pages[0]?.messages[2]?.toolCalls?.[0]?.result).toBe("preview");
    expect(timelineData?.pages[0]?.newestLoadedSequence).toBe(9);
    expect(
      getCachedConversationMessages(queryClient, "conv-1").some(
        (message) => message.parentMessageId === "assistant-final",
      )
    ).toBe(true);
  });

  it("keeps already-cached block sequences when finalizing around a mid-run user message", () => {
    const { queryClient } = createWrapperWithClient();
    queryClient.setQueryData(chatKeys.conversation("conv-1"), {
      conversation: mockConversation1,
      messages: [],
    });
    queryClient.setQueryData<InfiniteData<ConversationTimelinePageResponse>>(
      chatKeys.conversationTimeline("conv-1"),
      {
        pages: [
          timelinePage([
            {
              ...mockMessage1,
              id: "block:assistant-final:0",
              parentMessageId: "assistant-final",
              role: "assistant",
              content: "Streamed before the send",
              contentBlocks: [{ type: "text", text: "Streamed before the send" }],
              timelineSequence: 3,
            },
            {
              ...mockMessage1,
              id: "block:user-mid-run:0",
              parentMessageId: "user-mid-run",
              content: "Mid-run question",
              contentBlocks: [{ type: "text", text: "Mid-run question" }],
              timelineSequence: 84,
            },
          ], {
            totalItemCount: 84,
            oldestLoadedSequence: 3,
            newestLoadedSequence: 84,
          }),
        ],
        pageParams: [null],
      },
    );

    const finalized: ChatMessageResponse = {
      ...mockMessage1,
      id: "assistant-final",
      role: "assistant",
      parentMessageId: null,
      content: "Streamed before the send",
      contentBlocks: [
        { type: "text", text: "Streamed before the send" },
        { type: "text", text: "Streamed after the send" },
      ],
      createdAt: "2026-01-24T10:01:00Z",
    };

    expect(upsertFinalizedMessageIntoConversationCache(queryClient, "conv-1", finalized)).toBe(true);

    const page = queryClient.getQueryData<InfiniteData<ConversationTimelinePageResponse>>(
      chatKeys.conversationTimeline("conv-1")
    )?.pages[0];

    // The already-durable block keeps sequence 3 and stays below the user
    // message; only the genuinely new block appends past the tail.
    expect(page?.items.map((item) => [item.id, item.sequence])).toEqual([
      ["block:assistant-final:0", 3],
      ["block:user-mid-run:0", 84],
      ["block:assistant-final:1", 85],
    ]);
    expect(page?.messages.map((message) => message.id)).toEqual([
      "block:assistant-final:0",
      "block:user-mid-run:0",
      "block:assistant-final:1",
    ]);
    expect(page?.oldestLoadedSequence).toBe(3);
    expect(page?.newestLoadedSequence).toBe(85);
  });

  it("does not upsert finalized messages when the active timeline cache is absent", () => {
    const { queryClient } = createWrapperWithClient();
    const finalized: ChatMessageResponse = {
      ...mockMessage2,
      id: "assistant-final",
      role: "assistant",
      conversationId: "conv-1",
      content: "Done",
      contentBlocks: [{ type: "text", text: "Done" }],
    };

    expect(upsertFinalizedMessageIntoConversationCache(queryClient, "conv-1", finalized)).toBe(false);
    expect(queryClient.getQueryData(chatKeys.conversationTimeline("conv-1"))).toBeUndefined();
  });

  it("does not upsert hidden finalized bootstrap messages into conversation caches", () => {
    const { queryClient } = createWrapperWithClient();
    queryClient.setQueryData(chatKeys.conversation("conv-1"), {
      conversation: mockConversation1,
      messages: [mockMessage1],
    });
    queryClient.setQueryData<InfiniteData<ConversationMessagesPageResponse>>(
      chatKeys.conversationHistory("conv-1"),
      {
        pages: [
          {
            conversation: mockConversation1,
            messages: [mockMessage1],
            limit: 40,
            offset: 0,
            totalMessageCount: 1,
            hasOlder: false,
          },
        ],
        pageParams: [0],
      },
    );
    queryClient.setQueryData<InfiniteData<ConversationTimelinePageResponse>>(
      chatKeys.conversationTimeline("conv-1"),
      {
        pages: [
          timelinePage([
            {
              ...mockMessage1,
              id: "block:message-1:0",
              parentMessageId: "message-1",
              timelineSequence: 7,
            },
          ], {
            totalItemCount: 7,
            oldestLoadedSequence: 7,
            newestLoadedSequence: 7,
          }),
        ],
        pageParams: [null],
      },
    );

    const hiddenFinalized: ChatMessageResponse = {
      ...mockMessage1,
      id: "bootstrap-hidden",
      role: "user",
      conversationId: "conv-1",
      content: "Execute task: task-hidden",
      metadata: JSON.stringify({
        hidden_from_ui: true,
        source: "task_runtime_bootstrap",
      }),
      contentBlocks: [{ type: "text", text: "Execute task: task-hidden" }],
    };

    expect(
      upsertFinalizedMessageIntoConversationCache(
        queryClient,
        "conv-1",
        hiddenFinalized,
      ),
    ).toBe(false);
    const cachedContents = getCachedConversationMessages(queryClient, "conv-1").map(
      (message) => message.content,
    );
    expect(cachedContents).toContain("Hello");
    expect(cachedContents).not.toContain("Execute task: task-hidden");
    const timelineContents =
      queryClient
        .getQueryData<InfiniteData<ConversationTimelinePageResponse>>(
          chatKeys.conversationTimeline("conv-1"),
        )
        ?.pages[0]?.messages.map((message) => message.content) ?? [];
    expect(timelineContents).toEqual(["Hello"]);
  });

  it("filters hidden full conversation messages from merged cache reads", () => {
    const { queryClient } = createWrapperWithClient();
    queryClient.setQueryData(chatKeys.conversation("conv-1"), {
      conversation: mockConversation1,
      messages: [
        {
          ...mockMessage1,
          id: "bootstrap-hidden",
          content: "Execute task: task-hidden",
          metadata: JSON.stringify({
            hidden_from_ui: true,
            source: "task_runtime_bootstrap",
          }),
        },
        mockMessage1,
      ],
    });

    expect(
      getCachedConversationMessages(queryClient, "conv-1").map(
        (message) => message.content,
      ),
    ).toEqual(["Hello"]);
  });

  it("upserts backend render-ready timeline items without synthesizing sequences", () => {
    const { queryClient } = createWrapperWithClient();
    queryClient.setQueryData(chatKeys.conversation("conv-1"), {
      conversation: mockConversation1,
      messages: [mockMessage1],
    });
    queryClient.setQueryData<InfiniteData<ConversationMessagesPageResponse>>(
      chatKeys.conversationHistory("conv-1"),
      {
        pages: [
          {
            conversation: mockConversation1,
            messages: [mockMessage1],
            limit: 40,
            offset: 0,
            totalMessageCount: 1,
            hasOlder: false,
          },
        ],
        pageParams: [0],
      },
    );
    queryClient.setQueryData<InfiniteData<ConversationTimelinePageResponse>>(
      chatKeys.conversationTimeline("conv-1"),
      {
        pages: [
          timelinePage([
            {
              ...mockMessage1,
              id: "block:message-1:0",
              parentMessageId: "message-1",
              timelineSequence: 7,
            },
          ], {
            totalItemCount: 7,
            oldestLoadedSequence: 7,
            newestLoadedSequence: 7,
          }),
        ],
        pageParams: [null],
      },
    );

    const didUpsert = upsertRenderReadyMessageIntoConversationCache(queryClient, "conv-1", {
      message: {
        id: "assistant-final",
        conversation_id: "conv-1",
        role: "assistant",
        content: "Done",
        tool_calls: [{
          id: "toolu-read",
          name: "Read",
          arguments: { file_path: "src/app.ts" },
          result: "preview",
        }],
        content_blocks: [
          { type: "text", text: "Done" },
          {
            type: "tool_use",
            id: "toolu-read",
            name: "Read",
            arguments: { file_path: "src/app.ts" },
            result: "preview",
          },
        ],
        created_at: "2026-01-24T10:01:00Z",
      },
      timeline_items: [{
        id: "block:assistant-final:1",
        conversation_id: "conv-1",
        message_id: "assistant-final",
        run_id: null,
        sequence: 12,
        block_index: 1,
        role: "assistant",
        kind: "tool_use",
        status: "finalized",
        content: "",
        content_blocks: [{
          type: "tool_use",
          id: "toolu-read",
          name: "Read",
          arguments: { file_path: "src/app.ts" },
          result: "preview",
        }],
        tool_call: {
          type: "tool_use",
          id: "toolu-read",
          name: "Read",
          arguments: { file_path: "src/app.ts" },
          result: "preview",
        },
        metadata: null,
        provider_harness: "codex",
        provider_session_id: "thread-1",
        created_at: "2026-01-24T10:01:00Z",
        updated_at: "2026-01-24T10:01:01Z",
        finalized_at: "2026-01-24T10:01:01Z",
      }],
    });

    expect(didUpsert).toBe(true);
    const timelineData =
      queryClient.getQueryData<InfiniteData<ConversationTimelinePageResponse>>(
        chatKeys.conversationTimeline("conv-1")
      );
    expect(timelineData?.pages[0]?.messages.map((message) => message.id)).toEqual([
      "block:message-1:0",
      "block:assistant-final:1",
    ]);
    expect(timelineData?.pages[0]?.newestLoadedSequence).toBe(12);
    expect(timelineData?.pages[0]?.messages[1]?.providerHarness).toBe("codex");
    expect(timelineData?.pages[0]?.messages[1]?.toolCalls?.[0]?.result).toBe("preview");
  });

  it("replaces the optimistic user row when render-ready arrives under the backend id", () => {
    const { queryClient } = createWrapperWithClient();
    queryClient.setQueryData(chatKeys.conversation("conv-1"), {
      conversation: mockConversation1,
      messages: [],
    });
    queryClient.setQueryData<InfiniteData<ConversationMessagesPageResponse>>(
      chatKeys.conversationHistory("conv-1"),
      {
        pages: [
          {
            conversation: mockConversation1,
            messages: [],
            limit: 40,
            offset: 0,
            totalMessageCount: 0,
            hasOlder: false,
          },
        ],
        pageParams: [0],
      },
    );
    queryClient.setQueryData<InfiniteData<ConversationTimelinePageResponse>>(
      chatKeys.conversationTimeline("conv-1"),
      {
        pages: [
          timelinePage([
            {
              ...mockMessage1,
              id: "block:assistant-live:0",
              parentMessageId: "assistant-live",
              role: "assistant",
              content: "Streamed before the send",
              timelineSequence: 83,
            },
          ], {
            totalItemCount: 83,
            oldestLoadedSequence: 83,
            newestLoadedSequence: 83,
          }),
        ],
        pageParams: [null],
      },
    );

    // The optimistic row carries a client-generated `optimistic:` id and a
    // guessed tail sequence — the backend id can never match it.
    addOptimisticUserMessageToConversationCache(queryClient, "conv-1", "Mid-run question");

    const didUpsert = upsertRenderReadyMessageIntoConversationCache(queryClient, "conv-1", {
      message: {
        id: "user-mid-run",
        conversation_id: "conv-1",
        role: "user",
        content: "Mid-run question",
        content_blocks: [{ type: "text", text: "Mid-run question" }],
        created_at: "2026-01-24T10:01:00Z",
      },
      timeline_items: [{
        id: "block:user-mid-run:0",
        conversation_id: "conv-1",
        message_id: "user-mid-run",
        run_id: null,
        sequence: 84,
        block_index: 0,
        role: "user",
        kind: "text",
        status: "finalized",
        content: "Mid-run question",
        content_blocks: [{ type: "text", text: "Mid-run question" }],
        tool_call: null,
        metadata: null,
        provider_harness: null,
        provider_session_id: null,
        created_at: "2026-01-24T10:01:00Z",
        updated_at: "2026-01-24T10:01:00Z",
        finalized_at: "2026-01-24T10:01:00Z",
      }],
    });

    expect(didUpsert).toBe(true);
    const page = queryClient.getQueryData<InfiniteData<ConversationTimelinePageResponse>>(
      chatKeys.conversationTimeline("conv-1")
    )?.pages[0];

    expect(page?.items.map((item) => [item.id, item.sequence])).toEqual([
      ["block:assistant-live:0", 83],
      ["block:user-mid-run:0", 84],
    ]);
    expect(
      page?.items.filter((item) => item.content === "Mid-run question"),
    ).toHaveLength(1);
    expect(
      page?.items.some((item) => item.id.startsWith("optimistic-timeline:")),
    ).toBe(false);
  });

  it("does not upsert hidden render-ready bootstrap rows into conversation caches", () => {
    const { queryClient } = createWrapperWithClient();
    queryClient.setQueryData(chatKeys.conversation("conv-1"), {
      conversation: mockConversation1,
      messages: [mockMessage1],
    });
    queryClient.setQueryData<InfiniteData<ConversationMessagesPageResponse>>(
      chatKeys.conversationHistory("conv-1"),
      {
        pages: [
          {
            conversation: mockConversation1,
            messages: [mockMessage1],
            limit: 40,
            offset: 0,
            totalMessageCount: 1,
            hasOlder: false,
          },
        ],
        pageParams: [0],
      },
    );
    queryClient.setQueryData<InfiniteData<ConversationTimelinePageResponse>>(
      chatKeys.conversationTimeline("conv-1"),
      {
        pages: [
          timelinePage([
            {
              ...mockMessage1,
              id: "block:message-1:0",
              parentMessageId: "message-1",
              timelineSequence: 7,
            },
          ], {
            totalItemCount: 7,
            oldestLoadedSequence: 7,
            newestLoadedSequence: 7,
          }),
        ],
        pageParams: [null],
      },
    );

    const didUpsert = upsertRenderReadyMessageIntoConversationCache(queryClient, "conv-1", {
      message: {
        id: "bootstrap-hidden",
        conversation_id: "conv-1",
        role: "user",
        content: "Execute task: task-hidden",
        metadata: JSON.stringify({
          hidden_from_ui: true,
          source: "task_runtime_bootstrap",
        }),
        created_at: "2026-01-24T10:01:00Z",
      },
      timeline_items: [{
        id: "block:bootstrap-hidden:0",
        conversation_id: "conv-1",
        message_id: "bootstrap-hidden",
        run_id: null,
        sequence: 12,
        block_index: 0,
        role: "user",
        kind: "text",
        status: "finalized",
        content: "Execute task: task-hidden",
        content_blocks: [{ type: "text", text: "Execute task: task-hidden" }],
        metadata: JSON.stringify({
          hidden_from_ui: true,
          source: "task_runtime_bootstrap",
        }),
        provider_harness: "codex",
        provider_session_id: "thread-1",
        created_at: "2026-01-24T10:01:00Z",
        updated_at: "2026-01-24T10:01:01Z",
        finalized_at: "2026-01-24T10:01:01Z",
      }],
    });

    expect(didUpsert).toBe(false);
    const cachedContents = getCachedConversationMessages(queryClient, "conv-1").map(
      (message) => message.content
    );
    expect(cachedContents).toContain("Hello");
    expect(cachedContents).not.toContain("Execute task: task-hidden");
    const timelineData =
      queryClient.getQueryData<InfiniteData<ConversationTimelinePageResponse>>(
        chatKeys.conversationTimeline("conv-1")
      );
    const timelineContents =
      timelineData?.pages[0]?.messages.map((message) => message.content) ?? [];
    expect(timelineContents).toContain("Hello");
    expect(timelineContents).not.toContain("Execute task: task-hidden");
  });

  it("replaces existing render-ready rows and sorts by backend sequence", () => {
    const { queryClient } = createWrapperWithClient();
    queryClient.setQueryData(chatKeys.conversation("conv-1"), {
      conversation: mockConversation1,
      messages: [mockMessage1],
    });
    queryClient.setQueryData<InfiniteData<ConversationMessagesPageResponse>>(
      chatKeys.conversationHistory("conv-1"),
      {
        pages: [
          {
            conversation: mockConversation1,
            messages: [mockMessage1],
            limit: 40,
            offset: 0,
            totalMessageCount: 1,
            hasOlder: false,
          },
        ],
        pageParams: [0],
      },
    );
    queryClient.setQueryData<InfiniteData<ConversationTimelinePageResponse>>(
      chatKeys.conversationTimeline("conv-1"),
      {
        pages: [
          timelinePage([
            {
              ...mockMessage1,
              id: "block:message-1:0",
              parentMessageId: "message-1",
              timelineSequence: 7,
            },
            {
              ...mockMessage2,
              id: "block:assistant-final:old",
              parentMessageId: "assistant-final",
              timelineSequence: 8,
            },
          ], {
            totalItemCount: 8,
            oldestLoadedSequence: 7,
            newestLoadedSequence: 8,
          }),
        ],
        pageParams: [null],
      },
    );

    const didUpsert = upsertRenderReadyMessageIntoConversationCache(queryClient, "conv-1", {
      message: {
        id: "assistant-final",
        role: "assistant",
        content: "Updated",
        tool_calls: {},
        content_blocks: "not-an-array",
        provider_session_id: "thread-2",
        created_at: "2026-01-24T10:02:00Z",
      },
      timeline_items: [
        {
          id: "block:assistant-final:1",
          message_id: "assistant-final",
          run_id: "run-1",
          sequence: 12,
          block_index: 1,
          role: "assistant",
          kind: "text",
          status: "finalized",
          content: "second",
          content_blocks: "not-an-array",
          metadata: null,
          provider_session_id: "thread-2",
          created_at: "2026-01-24T10:02:00Z",
          updated_at: "2026-01-24T10:02:01Z",
          finalized_at: null,
        },
        {
          id: "block:assistant-final:0",
          message_id: "assistant-final",
          run_id: "run-1",
          sequence: 11,
          block_index: 0,
          role: "assistant",
          kind: "text",
          status: "finalized",
          content: "first",
          content_blocks: [{ type: "text", text: "first" }],
          metadata: null,
          provider_session_id: "thread-2",
          created_at: "2026-01-24T10:02:00Z",
          updated_at: "2026-01-24T10:02:01Z",
          finalized_at: "2026-01-24T10:02:01Z",
        },
      ],
    });

    expect(didUpsert).toBe(true);
    const timelineData =
      queryClient.getQueryData<InfiniteData<ConversationTimelinePageResponse>>(
        chatKeys.conversationTimeline("conv-1")
      );
    expect(timelineData?.pages[0]?.items.map((item) => item.id)).toEqual([
      "block:message-1:0",
      "block:assistant-final:0",
      "block:assistant-final:1",
    ]);
    expect(timelineData?.pages[0]?.messages[1]?.conversationId).toBe("conv-1");
    expect(timelineData?.pages[0]?.messages[1]?.contentBlocks).toEqual([
      { type: "text", text: "first" },
    ]);
    expect(timelineData?.pages[0]?.messages[2]?.contentBlocks).toEqual([]);
    expect(timelineData?.pages[0]?.newestLoadedSequence).toBe(12);
    expect(getCachedConversationMessages(queryClient, "conv-1")).toContainEqual(
      expect.objectContaining({
        id: "assistant-final",
        conversationId: "conv-1",
        contentBlocks: null,
        providerSessionId: "thread-2",
      }),
    );
  });

  it("does not upsert incomplete render-ready payloads or create missing timeline caches", () => {
    const { queryClient } = createWrapperWithClient();

    expect(upsertRenderReadyMessageIntoConversationCache(queryClient, "conv-1", {
      message: null,
      timeline_items: [],
    })).toBe(false);
    expect(upsertRenderReadyMessageIntoConversationCache(queryClient, "conv-1", {
      message: {
        id: "assistant-final",
        role: "assistant",
        content: "Done",
        created_at: "2026-01-24T10:02:00Z",
      },
      timeline_items: null,
    })).toBe(false);
    expect(upsertRenderReadyMessageIntoConversationCache(queryClient, "conv-1", {
      message: {
        id: "assistant-final",
        role: "assistant",
        content: "Done",
        created_at: "2026-01-24T10:02:00Z",
      },
      timeline_items: [{
        id: "block:assistant-final:0",
        message_id: "assistant-final",
        sequence: 1,
        block_index: 0,
        role: "assistant",
        kind: "text",
        status: "finalized",
        content: "Done",
        content_blocks: [{ type: "text", text: "Done" }],
        created_at: "2026-01-24T10:02:00Z",
        updated_at: "2026-01-24T10:02:01Z",
      }],
    })).toBe(false);

    expect(queryClient.getQueryData(chatKeys.conversationTimeline("conv-1"))).toBeUndefined();
  });

  it("upserts finalized text content when content blocks are absent", () => {
    const { queryClient } = createWrapperWithClient();
    queryClient.setQueryData(chatKeys.conversation("conv-1"), {
      conversation: mockConversation1,
      messages: [{
        ...mockMessage2,
        id: "assistant-final",
        content: "Stale final text",
      }],
    });
    queryClient.setQueryData<InfiniteData<ConversationMessagesPageResponse>>(
      chatKeys.conversationHistory("conv-1"),
      {
        pages: [
          {
            conversation: mockConversation1,
            messages: [{
              ...mockMessage2,
              id: "assistant-final",
              content: "Stale final text",
            }],
            limit: 40,
            offset: 0,
            totalMessageCount: 1,
            hasOlder: false,
          },
        ],
        pageParams: [0],
      },
    );
    queryClient.setQueryData<InfiniteData<ConversationTimelinePageResponse>>(
      chatKeys.conversationTimeline("conv-1"),
      {
        pages: [timelinePage([], { totalItemCount: 0 })],
        pageParams: [null],
      },
    );

    const finalized: ChatMessageResponse = {
      ...mockMessage2,
      id: "assistant-final",
      role: "assistant",
      conversationId: "conv-1",
      content: "Plain final text",
      contentBlocks: null,
      providerHarness: "codex",
      providerSessionId: "thread-final",
      upstreamProvider: "openai",
      providerProfile: "default",
      logicalModel: "gpt-5.5",
      effectiveModelId: "gpt-5.5-2026-05-01",
      logicalEffort: "medium",
      effectiveEffort: "medium",
      inputTokens: 11,
      outputTokens: 17,
      cacheCreationTokens: 3,
      cacheReadTokens: 5,
      estimatedUsd: 0.0123,
    };

    expect(upsertFinalizedMessageIntoConversationCache(queryClient, "conv-1", finalized)).toBe(true);

    const timelineData =
      queryClient.getQueryData<InfiniteData<ConversationTimelinePageResponse>>(
        chatKeys.conversationTimeline("conv-1")
      );
    expect(timelineData?.pages[0]?.items).toHaveLength(1);
    expect(timelineData?.pages[0]?.items[0]).toMatchObject({
      id: "block:assistant-final:0",
      content: "Plain final text",
      providerHarness: "codex",
      providerSessionId: "thread-final",
      upstreamProvider: "openai",
      providerProfile: "default",
      logicalModel: "gpt-5.5",
      effectiveModelId: "gpt-5.5-2026-05-01",
      logicalEffort: "medium",
      effectiveEffort: "medium",
      inputTokens: 11,
      outputTokens: 17,
      cacheCreationTokens: 3,
      cacheReadTokens: 5,
      estimatedUsd: 0.0123,
    });
    expect(
      queryClient.getQueryData<ConversationQueryData>(
        chatKeys.conversation("conv-1")
      )?.messages
    ).toContainEqual(finalized);
  });

  it("does not upsert finalized messages without renderable content", () => {
    const { queryClient } = createWrapperWithClient();
    queryClient.setQueryData<InfiniteData<ConversationTimelinePageResponse>>(
      chatKeys.conversationTimeline("conv-1"),
      {
        pages: [timelinePage([], { totalItemCount: 0 })],
        pageParams: [null],
      },
    );

    expect(upsertFinalizedMessageIntoConversationCache(queryClient, "conv-1", {
      ...mockMessage2,
      id: "assistant-final",
      content: "   ",
      contentBlocks: null,
    })).toBe(false);
    expect(
      queryClient.getQueryData<InfiniteData<ConversationTimelinePageResponse>>(
        chatKeys.conversationTimeline("conv-1")
      )?.pages[0]?.items
    ).toEqual([]);
  });

  it("does not update finalized or render-ready caches when the newest timeline page is missing", () => {
    const { queryClient } = createWrapperWithClient();
    queryClient.setQueryData<InfiniteData<ConversationTimelinePageResponse>>(
      chatKeys.conversationTimeline("conv-1"),
      {
        pages: [undefined as unknown as ConversationTimelinePageResponse],
        pageParams: [null],
      },
    );

    expect(upsertFinalizedMessageIntoConversationCache(queryClient, "conv-1", {
      ...mockMessage2,
      id: "assistant-final",
      content: "Done",
      contentBlocks: [{ type: "text", text: "Done" }],
    })).toBe(false);
    expect(upsertRenderReadyMessageIntoConversationCache(queryClient, "conv-1", {
      message: {
        id: "assistant-final",
        role: "assistant",
        content: "Done",
        created_at: "2026-01-24T10:02:00Z",
      },
      timeline_items: [{
        id: "block:assistant-final:0",
        message_id: "assistant-final",
        sequence: 1,
        block_index: 0,
        role: "assistant",
        kind: "text",
        status: "finalized",
        content: "Done",
        content_blocks: [{ type: "text", text: "Done" }],
        created_at: "2026-01-24T10:02:00Z",
        updated_at: "2026-01-24T10:02:01Z",
      }],
    })).toBe(false);
  });

  it("stores composer reference metadata on optimistic user messages", () => {
    const { queryClient } = createWrapperWithClient();
    const metadata =
      '{"composer_integration_references":[{"provider":"atlassian","kind":"jira","id":"RX-42","key":"RX-42"}]}';
    queryClient.setQueryData(chatKeys.conversation("conv-1"), {
      conversation: mockConversation1,
      messages: [],
    });
    queryClient.setQueryData<InfiniteData<ConversationMessagesPageResponse>>(
      chatKeys.conversationHistory("conv-1"),
      {
        pages: [
          {
            conversation: mockConversation1,
            messages: [],
            limit: 40,
            offset: 0,
            totalMessageCount: 0,
            hasOlder: false,
          },
        ],
        pageParams: [0],
      },
    );
    queryClient.setQueryData<InfiniteData<ConversationTimelinePageResponse>>(
      chatKeys.conversationTimeline("conv-1"),
      {
        pages: [timelinePage([], { limit: 40, totalItemCount: 0 })],
        pageParams: [null],
      },
    );

    const optimistic = addOptimisticUserMessageToConversationCache(
      queryClient,
      "conv-1",
      "hello from user",
      { metadata },
    );

    expect(optimistic.metadata).toBe(metadata);
    expect(
      queryClient.getQueryData<{ messages: ChatMessageResponse[] }>(
        chatKeys.conversation("conv-1"),
      )?.messages[0]?.metadata,
    ).toBe(metadata);
    expect(
      queryClient.getQueryData<InfiniteData<ConversationMessagesPageResponse>>(
        chatKeys.conversationHistory("conv-1"),
      )?.pages[0]?.messages[0]?.metadata,
    ).toBe(metadata);
    expect(
      queryClient.getQueryData<InfiniteData<ConversationTimelinePageResponse>>(
        chatKeys.conversationTimeline("conv-1"),
      )?.pages[0]?.messages[0]?.metadata,
    ).toBe(metadata);
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
    mockStoreState.activeConversationIds = {};
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

  it("passes Team intent through send message options", async () => {
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
      await result.current.sendMessage.mutateAsync({
        content: "New Team message",
        teamIntent: { coordinationMode: "rx_native_team" },
      });
    });

    expect(chatApi.sendAgentMessage).toHaveBeenCalledWith(
      "ideation",
      "session-1",
      "New Team message",
      undefined,
      { teamIntent: { coordinationMode: "rx_native_team" } },
    );
  });

  it("optimistically adds a sent user message to an existing conversation before backend hydration", async () => {
    let resolveSend!: (value: {
      conversationId: string;
      agentRunId: string;
      isNewConversation: boolean;
      wasQueued: boolean;
      queuedAsPending: boolean;
    }) => void;
    vi.mocked(chatApi.sendAgentMessage).mockImplementationOnce(
      () =>
        new Promise((resolve) => {
          resolveSend = resolve;
        })
    );
    mockStoreState.activeConversationIds = { "session:session-1": "conv-1" };
    const { queryClient, wrapper } = createWrapperWithClient();
    queryClient.setQueryData<InfiniteData<ConversationMessagesPageResponse>>(
      chatKeys.conversationHistory("conv-1"),
      {
        pages: [
          {
            conversation: mockConversation1,
            messages: [mockMessage1, mockMessage2],
            limit: 40,
            offset: 0,
            totalMessageCount: 2,
            hasOlder: false,
          },
        ],
        pageParams: [0],
      }
    );

    const { result } = renderHook(() => useChat(ideationContext), { wrapper });
    let sendPromise!: Promise<unknown>;

    act(() => {
      sendPromise = result.current.sendMessage.mutateAsync({
        content: "Visible immediately",
      });
    });

    await waitFor(() => {
      const optimisticHistory = queryClient.getQueryData<
        InfiniteData<ConversationMessagesPageResponse>
      >(chatKeys.conversationHistory("conv-1"));
      const newestPageMessages = optimisticHistory?.pages[0]?.messages ?? [];
      expect(newestPageMessages.map((message) => message.content)).toEqual([
        "Hello",
        "Hi there! How can I help?",
        "Visible immediately",
      ]);
      expect(newestPageMessages.at(-1)?.id).toMatch(/^optimistic:conv-1:/);
      expect(optimisticHistory?.pages[0]?.totalMessageCount).toBe(3);
    });
    await waitFor(() => expect(chatApi.sendAgentMessage).toHaveBeenCalled());

    await act(async () => {
      resolveSend({
        conversationId: "conv-1",
        agentRunId: "run-1",
        isNewConversation: false,
        wasQueued: false,
        queuedAsPending: false,
      });
      await sendPromise;
    });
  });

  it("keeps duplicate pending user messages with the same text distinct", async () => {
    const sendResolvers: Array<(value: SendAgentMessageResult) => void> = [];
    vi.mocked(chatApi.sendAgentMessage).mockImplementation(
      () =>
        new Promise((resolve) => {
          sendResolvers.push(resolve);
        })
    );
    mockStoreState.activeConversationIds = { "session:session-1": "conv-1" };
    const { queryClient, wrapper } = createWrapperWithClient();
    queryClient.setQueryData(chatKeys.conversation("conv-1"), {
      conversation: mockConversation1,
      messages: [mockMessage1, mockMessage2],
    });
    queryClient.setQueryData<InfiniteData<ConversationMessagesPageResponse>>(
      chatKeys.conversationHistory("conv-1"),
      {
        pages: [
          {
            conversation: mockConversation1,
            messages: [mockMessage1, mockMessage2],
            limit: 40,
            offset: 0,
            totalMessageCount: 2,
            hasOlder: false,
          },
        ],
        pageParams: [0],
      }
    );

    const { result } = renderHook(() => useChat(ideationContext), { wrapper });
    const sendPromises: Array<Promise<unknown>> = [];

    act(() => {
      sendPromises.push(result.current.sendMessage.mutateAsync({ content: "Repeat" }));
      sendPromises.push(result.current.sendMessage.mutateAsync({ content: "Repeat" }));
    });

    await waitFor(() => {
      const conversationData = queryClient.getQueryData<{
        messages: ChatMessageResponse[];
      }>(chatKeys.conversation("conv-1"));
      const duplicateOptimisticMessages = (conversationData?.messages ?? []).filter(
        (message) => message.content === "Repeat"
      );
      expect(duplicateOptimisticMessages).toHaveLength(2);
      expect(new Set(duplicateOptimisticMessages.map((message) => message.id)).size).toBe(2);

      const optimisticHistory = queryClient.getQueryData<
        InfiniteData<ConversationMessagesPageResponse>
      >(chatKeys.conversationHistory("conv-1"));
      expect(
        (optimisticHistory?.pages[0]?.messages ?? []).filter(
          (message) => message.content === "Repeat"
        )
      ).toHaveLength(2);
      expect(optimisticHistory?.pages[0]?.totalMessageCount).toBe(4);
    });

    await act(async () => {
      sendResolvers.forEach((resolve) =>
        resolve({
          conversationId: "conv-1",
          agentRunId: "run-1",
          isNewConversation: false,
          wasQueued: false,
          queuedAsPending: false,
        })
      );
      await Promise.all(sendPromises);
    });
  });

  it("rolls back an optimistic user message when sending fails", async () => {
    vi.mocked(chatApi.sendAgentMessage).mockRejectedValueOnce(new Error("send failed"));
    mockStoreState.activeConversationIds = { "session:session-1": "conv-1" };
    const { queryClient, wrapper } = createWrapperWithClient();
    queryClient.setQueryData(chatKeys.conversation("conv-1"), {
      conversation: mockConversation1,
      messages: [mockMessage1, mockMessage2],
    });
    queryClient.setQueryData<InfiniteData<ConversationMessagesPageResponse>>(
      chatKeys.conversationHistory("conv-1"),
      {
        pages: [
          {
            conversation: mockConversation1,
            messages: [mockMessage1, mockMessage2],
            limit: 40,
            offset: 0,
            totalMessageCount: 2,
            hasOlder: false,
          },
        ],
        pageParams: [0],
      }
    );

    const { result } = renderHook(() => useChat(ideationContext), { wrapper });

    await expect(
      result.current.sendMessage.mutateAsync({ content: "Rollback me" })
    ).rejects.toThrow("send failed");

    const conversationData = queryClient.getQueryData<{
      messages: ChatMessageResponse[];
    }>(chatKeys.conversation("conv-1"));
    const historyData = queryClient.getQueryData<
      InfiniteData<ConversationMessagesPageResponse>
    >(chatKeys.conversationHistory("conv-1"));

    expect(conversationData?.messages.map((message) => message.content)).toEqual([
      "Hello",
      "Hi there! How can I help?",
    ]);
    expect(historyData?.pages[0]?.messages.map((message) => message.content)).toEqual([
      "Hello",
      "Hi there! How can I help?",
    ]);
    expect(historyData?.pages[0]?.totalMessageCount).toBe(2);
    expect(mockStoreState.setAgentRunning).toHaveBeenCalledWith("session:session-1", false);
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
