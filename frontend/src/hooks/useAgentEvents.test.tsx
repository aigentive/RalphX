/**
 * Tests for useAgentEvents hook
 *
 * Covers:
 * - agent:run_started sets running state
 * - agent:run_completed clears running state
 * - agent:stopped clears running state (defensive)
 * - agent:error clears running state
 * - Event listeners are properly cleaned up on unmount
 */

import { renderHook, act } from "@testing-library/react";
import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import type { ReactNode } from "react";
import { QueryClient, QueryClientProvider, type InfiniteData } from "@tanstack/react-query";
import { useAgentEvents } from "./useAgentEvents";
import { useChatStore } from "@/stores/chatStore";
import { useIdeationStore } from "@/stores/ideationStore";
import { useUiStore } from "@/stores/uiStore";
import type { AskUserQuestionPayload } from "@/types/ask-user-question";
import type { ChatConversation } from "@/types/chat-conversation";
import type {
  ChatMessageResponse,
  ConversationMessagesPageResponse,
} from "@/api/chat";

// ============================================================================
// Mock EventBus
// ============================================================================

type EventHandler = (payload: unknown) => void;

const listeners = new Map<string, Set<EventHandler>>();

function mockSubscribe(event: string, handler: EventHandler) {
  if (!listeners.has(event)) {
    listeners.set(event, new Set());
  }
  listeners.get(event)!.add(handler);
  return () => {
    listeners.get(event)?.delete(handler);
  };
}

function emitEvent(event: string, payload: unknown) {
  listeners.get(event)?.forEach((handler) => handler(payload));
}

vi.mock("@/providers/EventProvider", () => ({
  useEventBus: () => ({
    subscribe: mockSubscribe,
    emit: vi.fn(),
  }),
}));

// Mock useChat to provide chatKeys
vi.mock("@/hooks/useChat", () => ({
  chatKeys: {
    conversationList: (type: string, id: string) => ["chat", "conversations", type, id],
    conversation: (id: string) => ["chat", "conversation", id],
    conversationHistory: (id: string) => ["chat", "conversation", id, "history"],
    agentRun: (id: string) => ["chat", "agentRun", id],
  },
  invalidateConversationDataQueries: (
    queryClient: { invalidateQueries: (input: { queryKey: unknown[] }) => void },
    conversationId: string
  ) => {
    queryClient.invalidateQueries({ queryKey: ["chat", "conversation", conversationId] });
    queryClient.invalidateQueries({
      queryKey: ["chat", "conversation", conversationId, "history"],
    });
  },
}));

// ============================================================================
// Test Setup
// ============================================================================

function createWrapper() {
  const queryClient = new QueryClient({
    defaultOptions: {
      queries: { retry: false, gcTime: 0 },
      mutations: { retry: false },
    },
  });
  return ({ children }: { children: ReactNode }) => (
    <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
  );
}

function createWrapperWithClient() {
  const queryClient = new QueryClient({
    defaultOptions: {
      queries: { retry: false, gcTime: 0 },
      mutations: { retry: false },
    },
  });
  const wrapper = ({ children }: { children: ReactNode }) => (
    <QueryClientProvider client={queryClient}>{children}</QueryClientProvider>
  );
  return { queryClient, wrapper };
}

function makeConversation(overrides: Partial<ChatConversation> = {}): ChatConversation {
  return {
    id: "conv-1",
    contextType: "task_execution",
    contextId: "task-123",
    claudeSessionId: null,
    providerSessionId: null,
    providerHarness: null,
    title: "Execution",
    messageCount: 0,
    lastMessageAt: null,
    createdAt: "2026-04-07T10:00:00.000Z",
    updatedAt: "2026-04-07T10:00:00.000Z",
    ...overrides,
  };
}

function makeMessage(overrides: Partial<ChatMessageResponse> = {}): ChatMessageResponse {
  return {
    id: "msg-1",
    conversationId: "conv-1",
    sessionId: null,
    projectId: null,
    taskId: null,
    role: "user",
    content: "Hello",
    metadata: null,
    parentMessageId: null,
    createdAt: "2026-04-07T10:01:00.000Z",
    toolCalls: null,
    contentBlocks: null,
    sender: null,
    ...overrides,
  };
}

describe("useAgentEvents", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    listeners.clear();

    // Reset chat store
    useChatStore.setState({
      activeConversationIds: {},
      queuedMessages: {},
      agentStatus: {},
      isSending: {},
      lastAgentEventTimestamp: {},
      toolCallStartTimes: {},
      lastToolCallCompletionTimestamp: {},
    });

    // Reset ideation store verification child state
    useIdeationStore.setState({
      activeVerificationChildId: {},
    } as Parameters<typeof useIdeationStore.setState>[0]);
  });

  describe("agent:run_started", () => {
    it("sets agent running state for the event context", () => {
      const wrapper = createWrapper();
      renderHook(() => useAgentEvents("conv-1"), { wrapper });

      act(() => {
        emitEvent("agent:run_started", {
          run_id: "run-1",
          context_type: "task",
          context_id: "task-123",
          conversation_id: "conv-1",
        });
      });

      const state = useChatStore.getState();
      expect(state.agentStatus["task:task-123"]).toBe("generating");
    });

    it("sets running state for task_execution context", () => {
      const wrapper = createWrapper();
      renderHook(() => useAgentEvents("conv-1"), { wrapper });

      act(() => {
        emitEvent("agent:run_started", {
          run_id: "run-1",
          context_type: "task_execution",
          context_id: "task-123",
          conversation_id: "conv-1",
        });
      });

      const state = useChatStore.getState();
      expect(state.agentStatus["task_execution:task-123"]).toBe("generating");
    });

    it("sets running state for review context", () => {
      const wrapper = createWrapper();
      renderHook(() => useAgentEvents("conv-1"), { wrapper });

      act(() => {
        emitEvent("agent:run_started", {
          run_id: "run-1",
          context_type: "review",
          context_id: "task-123",
          conversation_id: "conv-1",
        });
      });

      const state = useChatStore.getState();
      expect(state.agentStatus["review:task-123"]).toBe("generating");
    });

    it("sets running state for merge context", () => {
      const wrapper = createWrapper();
      renderHook(() => useAgentEvents("conv-1"), { wrapper });

      act(() => {
        emitEvent("agent:run_started", {
          run_id: "run-1",
          context_type: "merge",
          context_id: "task-123",
          conversation_id: "conv-1",
        });
      });

      const state = useChatStore.getState();
      expect(state.agentStatus["merge:task-123"]).toBe("generating");
    });

    it("sets running state for ideation context", () => {
      const wrapper = createWrapper();
      renderHook(() => useAgentEvents("conv-1"), { wrapper });

      act(() => {
        emitEvent("agent:run_started", {
          run_id: "run-1",
          context_type: "ideation",
          context_id: "session-789",
          conversation_id: "conv-1",
        });
      });

      const state = useChatStore.getState();
      expect(state.agentStatus["session:session-789"]).toBe("generating");
    });

    it("merges provider metadata into cached conversation state", () => {
      const { queryClient, wrapper } = createWrapperWithClient();
      const conversation = makeConversation();
      queryClient.setQueryData(
        ["chat", "conversation", "conv-1"],
        { conversation, messages: [] }
      );
      queryClient.setQueryData(
        ["chat", "conversations", "task_execution", "task-123"],
        [conversation]
      );

      renderHook(() => useAgentEvents("conv-1"), { wrapper });

      act(() => {
        emitEvent("agent:run_started", {
          run_id: "run-1",
          context_type: "task_execution",
          context_id: "task-123",
          conversation_id: "conv-1",
          provider_harness: "codex",
          provider_session_id: "thread-7",
        });
      });

      const conversationQuery = queryClient.getQueryData<{
        conversation: ChatConversation;
        messages: unknown[];
      }>(["chat", "conversation", "conv-1"]);
      const listQuery = queryClient.getQueryData<ChatConversation[]>([
        "chat",
        "conversations",
        "task_execution",
        "task-123",
      ]);

      expect(conversationQuery?.conversation.providerHarness).toBe("codex");
      expect(conversationQuery?.conversation.providerSessionId).toBe("thread-7");
      expect(conversationQuery?.conversation.claudeSessionId).toBeNull();
      expect(listQuery?.[0]?.providerHarness).toBe("codex");
      expect(listQuery?.[0]?.providerSessionId).toBe("thread-7");
    });

    it("merges provider metadata into cached infinite conversation history pages", () => {
      const { queryClient, wrapper } = createWrapperWithClient();
      const conversation = makeConversation();
      queryClient.setQueryData<InfiniteData<ConversationMessagesPageResponse>>(
        ["chat", "conversation", "conv-1", "history"],
        {
          pages: [
            {
              conversation,
              messages: [],
              limit: 40,
              offset: 0,
              totalMessageCount: 0,
              hasOlder: false,
            },
          ],
          pageParams: [0],
        }
      );

      renderHook(() => useAgentEvents("conv-1"), { wrapper });

      act(() => {
        emitEvent("agent:run_started", {
          run_id: "run-1",
          context_type: "task_execution",
          context_id: "task-123",
          conversation_id: "conv-1",
          provider_harness: "codex",
          provider_session_id: "thread-7",
        });
      });

      const historyQuery = queryClient.getQueryData<
        InfiniteData<ConversationMessagesPageResponse>
      >(["chat", "conversation", "conv-1", "history"]);

      expect(historyQuery?.pages[0]?.conversation.providerHarness).toBe("codex");
      expect(historyQuery?.pages[0]?.conversation.providerSessionId).toBe("thread-7");
      expect((historyQuery as unknown as { conversation?: unknown }).conversation).toBeUndefined();
      expect((historyQuery as unknown as { messages?: unknown }).messages).toBeUndefined();
    });

    it("clears stale claude alias when provider metadata switches to codex", () => {
      const { queryClient, wrapper } = createWrapperWithClient();
      const conversation: ChatConversation = {
        ...makeConversation(),
        claudeSessionId: "claude-session-1",
        providerSessionId: "claude-session-1",
        providerHarness: "claude",
      };

      queryClient.setQueryData(
        ["chat", "conversation", "conv-1"],
        { conversation, messages: [] }
      );
      queryClient.setQueryData(
        ["chat", "conversations", "task_execution", "task-123"],
        [conversation]
      );

      renderHook(() => useAgentEvents("conv-1"), { wrapper });

      act(() => {
        emitEvent("agent:run_started", {
          run_id: "run-2",
          context_type: "task_execution",
          context_id: "task-123",
          conversation_id: "conv-1",
          provider_harness: "codex",
          provider_session_id: "thread-9",
        });
      });

      const conversationQuery = queryClient.getQueryData<{
        conversation: ChatConversation;
        messages: unknown[];
      }>(["chat", "conversation", "conv-1"]);

      expect(conversationQuery?.conversation.providerHarness).toBe("codex");
      expect(conversationQuery?.conversation.providerSessionId).toBe("thread-9");
      expect(conversationQuery?.conversation.claudeSessionId).toBeNull();
    });
  });

  describe("agent:message_created cache updates", () => {
    it("appends optimistic user messages to the infinite conversation history shape", () => {
      const { queryClient, wrapper } = createWrapperWithClient();
      const conversation = makeConversation();
      const existingMessage = makeMessage({
        id: "msg-existing",
        content: "Existing",
        createdAt: "2026-04-07T10:00:00.000Z",
      });

      queryClient.setQueryData<InfiniteData<ConversationMessagesPageResponse>>(
        ["chat", "conversation", "conv-1", "history"],
        {
          pages: [
            {
              conversation,
              messages: [existingMessage],
              limit: 40,
              offset: 0,
              totalMessageCount: 1,
              hasOlder: false,
            },
          ],
          pageParams: [0],
        }
      );

      renderHook(() => useAgentEvents("conv-1"), { wrapper });

      act(() => {
        emitEvent("agent:message_created", {
          context_type: "project",
          context_id: "project-1",
          conversation_id: "conv-1",
          message_id: "msg-new",
          role: "user",
          content: "New message",
          created_at: "2026-04-07T10:02:00.000Z",
        });
      });

      const historyQuery = queryClient.getQueryData<
        InfiniteData<ConversationMessagesPageResponse>
      >(["chat", "conversation", "conv-1", "history"]);

      expect(historyQuery?.pages[0]?.messages.map((message) => message.id)).toEqual([
        "msg-existing",
        "msg-new",
      ]);
      expect(historyQuery?.pages[0]?.totalMessageCount).toBe(2);
      expect((historyQuery as unknown as { messages?: unknown }).messages).toBeUndefined();
    });
  });

  describe("agent:run_started — storeKey param", () => {
    it("uses caller-provided storeKey for setActiveConversation when no active conversation", () => {
      const wrapper = createWrapper();
      // Hook called from a panel with storeKey "task_execution:task-123"
      // but event arrives for "task_execution:task-123" too
      renderHook(() => useAgentEvents(null, "task_execution:task-123"), { wrapper });

      act(() => {
        emitEvent("agent:run_started", {
          run_id: "run-1",
          context_type: "task_execution",
          context_id: "task-123",
          conversation_id: "conv-new",
        });
      });

      const state = useChatStore.getState();
      // Should write to the caller-provided storeKey
      expect(state.activeConversationIds["task_execution:task-123"]).toBe("conv-new");
    });

    it("uses caller-provided storeKey instead of event-derived key when they differ", () => {
      const wrapper = createWrapper();
      // Panel is in "task" context but event is "task_execution" —
      // caller says to write to "task:task-123" (current panel slot)
      renderHook(() => useAgentEvents(null, "task:task-123"), { wrapper });

      act(() => {
        emitEvent("agent:run_started", {
          run_id: "run-1",
          context_type: "task_execution",
          context_id: "task-123",
          conversation_id: "conv-exec",
        });
      });

      const state = useChatStore.getState();
      // Should write to the caller-provided "task:task-123", NOT event-derived "task_execution:task-123"
      expect(state.activeConversationIds["task:task-123"]).toBe("conv-exec");
      expect(state.activeConversationIds["task_execution:task-123"]).toBeUndefined();
    });

    it("falls back to event-derived key when no storeKey provided", () => {
      const wrapper = createWrapper();
      renderHook(() => useAgentEvents(null), { wrapper });

      act(() => {
        emitEvent("agent:run_started", {
          run_id: "run-1",
          context_type: "task_execution",
          context_id: "task-456",
          conversation_id: "conv-456",
        });
      });

      const state = useChatStore.getState();
      expect(state.activeConversationIds["task_execution:task-456"]).toBe("conv-456");
    });

    it("does not overwrite existing active conversation", () => {
      const wrapper = createWrapper();
      // Pre-set an active conversation for this slot
      act(() => {
        useChatStore.getState().setActiveConversation("task_execution:task-123", "conv-existing");
      });

      renderHook(() => useAgentEvents("conv-existing", "task_execution:task-123"), { wrapper });

      act(() => {
        emitEvent("agent:run_started", {
          run_id: "run-1",
          context_type: "task_execution",
          context_id: "task-123",
          conversation_id: "conv-new",
        });
      });

      // Should NOT overwrite because activeConversationId is already set
      const state = useChatStore.getState();
      expect(state.activeConversationIds["task_execution:task-123"]).toBe("conv-existing");
    });

    it("does not write another project conversation into a scoped Agents workspace slot", () => {
      const wrapper = createWrapper();
      renderHook(() => useAgentEvents(null, "project:conv-selected"), { wrapper });

      act(() => {
        emitEvent("agent:run_started", {
          run_id: "run-1",
          context_type: "project",
          context_id: "project-1",
          conversation_id: "conv-other",
        });
      });

      const state = useChatStore.getState();
      expect(state.agentStatus["project:conv-other"]).toBe("generating");
      expect(state.activeConversationIds["project:conv-selected"]).toBeUndefined();
      expect(state.activeConversationIds["project:conv-other"]).toBeUndefined();
    });
  });

  describe("agent:run_completed", () => {
    it("clears agent running state on completion", () => {
      const wrapper = createWrapper();

      // First set running state
      act(() => {
        useChatStore.getState().setAgentRunning("task:task-123", true);
      });
      expect(useChatStore.getState().agentStatus["task:task-123"]).toBe("generating");

      renderHook(() => useAgentEvents("conv-1"), { wrapper });

      act(() => {
        emitEvent("agent:run_completed", {
          context_type: "task",
          context_id: "task-123",
          conversation_id: "conv-1",
          status: "completed",
        });
      });

      const state = useChatStore.getState();
      // After run_completed, the running state should be cleared
      expect(state.agentStatus["task:task-123"]).toBeUndefined();
    });

    it("invalidates agent workspace publish state when a project agent completes", () => {
      const { queryClient, wrapper } = createWrapperWithClient();
      const invalidateSpy = vi.spyOn(queryClient, "invalidateQueries");

      renderHook(() => useAgentEvents("conv-1"), { wrapper });

      act(() => {
        emitEvent("agent:run_completed", {
          context_type: "project",
          context_id: "project-1",
          conversation_id: "conv-1",
          status: "completed",
        });
      });

      expect(invalidateSpy).toHaveBeenCalledWith({
        queryKey: ["agents", "conversation-workspace", "conv-1"],
      });
      expect(invalidateSpy).toHaveBeenCalledWith({
        queryKey: ["agents", "conversation-workspace-freshness", "conv-1"],
      });
      expect(invalidateSpy).toHaveBeenCalledWith({
        queryKey: ["agents", "conversation-workspace-publication-events", "conv-1"],
      });
    });

    it("invalidates workspace publish queries from workspace-changed payloads", () => {
      const { queryClient, wrapper } = createWrapperWithClient();
      const invalidateSpy = vi.spyOn(queryClient, "invalidateQueries");

      renderHook(() => useAgentEvents("conv-1"), { wrapper });

      act(() => {
        emitEvent("agent:workspace_changed", {
          conversation_id: "conv-snake",
        });
      });
      act(() => {
        emitEvent("agent:workspace_changed", {
          conversationId: "conv-camel",
        });
      });
      act(() => {
        emitEvent("agent:workspace_changed", null);
        emitEvent("agent:workspace_changed", { conversation_id: "   " });
      });

      expect(invalidateSpy).toHaveBeenCalledWith({
        queryKey: ["agents", "conversation-workspace", "conv-snake"],
      });
      expect(invalidateSpy).toHaveBeenCalledWith({
        queryKey: ["agents", "conversation-workspace-freshness", "conv-snake"],
      });
      expect(invalidateSpy).toHaveBeenCalledWith({
        queryKey: ["agents", "conversation-workspace-publication-events", "conv-snake"],
      });
      expect(invalidateSpy).toHaveBeenCalledWith({
        queryKey: ["agents", "workspace-diff", "conv-camel"],
      });
      expect(invalidateSpy).toHaveBeenCalledWith({
        queryKey: ["agents", "workspace-commits", "conv-camel"],
      });
      expect(invalidateSpy).not.toHaveBeenCalledWith({
        queryKey: ["agents", "conversation-workspace", "   "],
      });
    });

    it("clears running state for task_execution on stop/completion", () => {
      const wrapper = createWrapper();

      act(() => {
        useChatStore.getState().setAgentRunning("task_execution:task-123", true);
      });

      renderHook(() => useAgentEvents("conv-1"), { wrapper });

      act(() => {
        emitEvent("agent:run_completed", {
          context_type: "task_execution",
          context_id: "task-123",
          conversation_id: "conv-1",
          status: "completed",
        });
      });

      expect(useChatStore.getState().agentStatus["task_execution:task-123"]).toBeUndefined();
    });

    it("clears running state for review on stop/completion", () => {
      const wrapper = createWrapper();

      act(() => {
        useChatStore.getState().setAgentRunning("review:task-123", true);
      });

      renderHook(() => useAgentEvents("conv-1"), { wrapper });

      act(() => {
        emitEvent("agent:run_completed", {
          context_type: "review",
          context_id: "task-123",
          conversation_id: "conv-1",
          status: "completed",
        });
      });

      expect(useChatStore.getState().agentStatus["review:task-123"]).toBeUndefined();
    });

    it("clears running state for ideation on stop/completion", () => {
      const wrapper = createWrapper();

      act(() => {
        useChatStore.getState().setAgentRunning("session:session-789", true);
      });

      renderHook(() => useAgentEvents("conv-1"), { wrapper });

      act(() => {
        emitEvent("agent:run_completed", {
          context_type: "ideation",
          context_id: "session-789",
          conversation_id: "conv-1",
          status: "completed",
        });
      });

      expect(useChatStore.getState().agentStatus["session:session-789"]).toBeUndefined();
    });

    it("clears running state for merge on stop/completion", () => {
      const wrapper = createWrapper();

      act(() => {
        useChatStore.getState().setAgentRunning("merge:task-123", true);
      });

      renderHook(() => useAgentEvents("conv-1"), { wrapper });

      act(() => {
        emitEvent("agent:run_completed", {
          context_type: "merge",
          context_id: "task-123",
          conversation_id: "conv-1",
          status: "completed",
        });
      });

      expect(useChatStore.getState().agentStatus["merge:task-123"]).toBeUndefined();
    });
  });

  describe("agent:stopped", () => {
    it("clears agent running state on stop (defensive)", () => {
      const wrapper = createWrapper();

      act(() => {
        useChatStore.getState().setAgentRunning("task:task-123", true);
      });
      expect(useChatStore.getState().agentStatus["task:task-123"]).toBe("generating");

      renderHook(() => useAgentEvents("conv-1"), { wrapper });

      act(() => {
        emitEvent("agent:stopped", {
          context_type: "task",
          context_id: "task-123",
          conversation_id: "conv-1",
          agent_run_id: "run-1",
        });
      });

      expect(useChatStore.getState().agentStatus["task:task-123"]).toBeUndefined();
    });

    it("clears running state for task_execution on stop", () => {
      const wrapper = createWrapper();

      act(() => {
        useChatStore.getState().setAgentRunning("task_execution:task-123", true);
      });

      renderHook(() => useAgentEvents("conv-1"), { wrapper });

      act(() => {
        emitEvent("agent:stopped", {
          context_type: "task_execution",
          context_id: "task-123",
          conversation_id: "conv-1",
          agent_run_id: "run-1",
        });
      });

      expect(useChatStore.getState().agentStatus["task_execution:task-123"]).toBeUndefined();
    });

    it("clears running state for review on stop", () => {
      const wrapper = createWrapper();

      act(() => {
        useChatStore.getState().setAgentRunning("review:task-123", true);
      });

      renderHook(() => useAgentEvents("conv-1"), { wrapper });

      act(() => {
        emitEvent("agent:stopped", {
          context_type: "review",
          context_id: "task-123",
          conversation_id: "conv-1",
          agent_run_id: "run-1",
        });
      });

      expect(useChatStore.getState().agentStatus["review:task-123"]).toBeUndefined();
    });
  });

  describe("agent:error", () => {
    it("clears agent running state on error", () => {
      const wrapper = createWrapper();

      act(() => {
        useChatStore.getState().setAgentRunning("task:task-123", true);
      });

      renderHook(() => useAgentEvents("conv-1"), { wrapper });

      act(() => {
        emitEvent("agent:error", {
          context_type: "task",
          context_id: "task-123",
          conversation_id: "conv-1",
          error: "Something went wrong",
        });
      });

      expect(useChatStore.getState().agentStatus["task:task-123"]).toBeUndefined();
    });

    it("clears running state for task_execution on error", () => {
      const wrapper = createWrapper();

      act(() => {
        useChatStore.getState().setAgentRunning("task_execution:task-123", true);
      });

      renderHook(() => useAgentEvents("conv-1"), { wrapper });

      act(() => {
        emitEvent("agent:error", {
          context_type: "task_execution",
          context_id: "task-123",
          conversation_id: "conv-1",
          error: "Agent crashed",
        });
      });

      expect(useChatStore.getState().agentStatus["task_execution:task-123"]).toBeUndefined();
    });

    it("clears running state for ideation on error", () => {
      const wrapper = createWrapper();

      act(() => {
        useChatStore.getState().setAgentRunning("session:session-789", true);
      });

      renderHook(() => useAgentEvents("conv-1"), { wrapper });

      act(() => {
        emitEvent("agent:error", {
          context_type: "ideation",
          context_id: "session-789",
          conversation_id: "conv-1",
          error: "Session error",
        });
      });

      expect(useChatStore.getState().agentStatus["session:session-789"]).toBeUndefined();
    });

    it("clears running state for review on error", () => {
      const wrapper = createWrapper();

      act(() => {
        useChatStore.getState().setAgentRunning("review:task-123", true);
      });

      renderHook(() => useAgentEvents("conv-1"), { wrapper });

      act(() => {
        emitEvent("agent:error", {
          context_type: "review",
          context_id: "task-123",
          conversation_id: "conv-1",
          error: "Review failed",
        });
      });

      expect(useChatStore.getState().agentStatus["review:task-123"]).toBeUndefined();
    });

    it("clears running state for merge on error", () => {
      const wrapper = createWrapper();

      act(() => {
        useChatStore.getState().setAgentRunning("merge:task-123", true);
      });

      renderHook(() => useAgentEvents("conv-1"), { wrapper });

      act(() => {
        emitEvent("agent:error", {
          context_type: "merge",
          context_id: "task-123",
          conversation_id: "conv-1",
          error: "Merge conflict",
        });
      });

      expect(useChatStore.getState().agentStatus["merge:task-123"]).toBeUndefined();
    });
  });

  describe("agent:turn_completed", () => {
    it("sets waiting_for_input for task_execution — agent stays alive between turns", () => {
      const wrapper = createWrapper();
      renderHook(() => useAgentEvents("conv-1"), { wrapper });

      act(() => {
        emitEvent("agent:run_started", {
          run_id: "run-1",
          context_type: "task_execution",
          context_id: "task-123",
          conversation_id: "conv-1",
        });
      });

      expect(useChatStore.getState().agentStatus["task_execution:task-123"]).toBe("generating");

      act(() => {
        emitEvent("agent:turn_completed", {
          context_type: "task_execution",
          context_id: "task-123",
          conversation_id: "conv-1",
          status: "turn_complete",
        });
      });

      // Transitions to waiting_for_input — agent alive, not generating
      expect(useChatStore.getState().agentStatus["task_execution:task-123"]).toBe("waiting_for_input");
    });

    it("sets waiting_for_input for ideation — agent stays alive between turns", () => {
      const wrapper = createWrapper();
      renderHook(() => useAgentEvents("conv-1"), { wrapper });

      act(() => {
        emitEvent("agent:run_started", {
          run_id: "run-1",
          context_type: "ideation",
          context_id: "session-789",
          conversation_id: "conv-1",
        });
      });

      expect(useChatStore.getState().agentStatus["session:session-789"]).toBe("generating");

      act(() => {
        emitEvent("agent:turn_completed", {
          context_type: "ideation",
          context_id: "session-789",
          conversation_id: "conv-1",
          status: "turn_complete",
        });
      });

      expect(useChatStore.getState().agentStatus["session:session-789"]).toBe("waiting_for_input");
    });

    it("sets waiting_for_input for review — agent stays alive between turns", () => {
      const wrapper = createWrapper();
      renderHook(() => useAgentEvents("conv-1"), { wrapper });

      act(() => {
        emitEvent("agent:run_started", {
          run_id: "run-1",
          context_type: "review",
          context_id: "task-123",
          conversation_id: "conv-1",
        });
      });

      expect(useChatStore.getState().agentStatus["review:task-123"]).toBe("generating");

      act(() => {
        emitEvent("agent:turn_completed", {
          context_type: "review",
          context_id: "task-123",
          conversation_id: "conv-1",
          status: "turn_complete",
        });
      });

      expect(useChatStore.getState().agentStatus["review:task-123"]).toBe("waiting_for_input");
    });

    it("sets waiting_for_input for merge — agent stays alive between turns", () => {
      const wrapper = createWrapper();
      renderHook(() => useAgentEvents("conv-1"), { wrapper });

      act(() => {
        emitEvent("agent:run_started", {
          run_id: "run-1",
          context_type: "merge",
          context_id: "task-123",
          conversation_id: "conv-1",
        });
      });

      expect(useChatStore.getState().agentStatus["merge:task-123"]).toBe("generating");

      act(() => {
        emitEvent("agent:turn_completed", {
          context_type: "merge",
          context_id: "task-123",
          conversation_id: "conv-1",
          status: "turn_complete",
        });
      });

      expect(useChatStore.getState().agentStatus["merge:task-123"]).toBe("waiting_for_input");
    });

    it("invalidates only agentRun when conversation_id matches active", () => {
      const { queryClient, wrapper } = createWrapperWithClient();
      const invalidateSpy = vi.spyOn(queryClient, "invalidateQueries");

      renderHook(() => useAgentEvents("conv-1"), { wrapper });

      act(() => {
        emitEvent("agent:turn_completed", {
          context_type: "task_execution",
          context_id: "task-123",
          conversation_id: "conv-1",
          status: "turn_complete",
        });
      });

      expect(invalidateSpy).toHaveBeenCalledWith({
        queryKey: ["chat", "agentRun", "conv-1"],
      });
      expect(invalidateSpy).not.toHaveBeenCalledWith({
        queryKey: ["chat", "conversation", "conv-1"],
      });
    });

    it("invalidates queries using payload conversation_id when it differs from active", () => {
      const { queryClient, wrapper } = createWrapperWithClient();
      const invalidateSpy = vi.spyOn(queryClient, "invalidateQueries");

      renderHook(() => useAgentEvents("conv-1"), { wrapper });

      act(() => {
        emitEvent("agent:turn_completed", {
          context_type: "task_execution",
          context_id: "task-123",
          conversation_id: "conv-OTHER",
          status: "turn_complete",
        });
      });

      expect(invalidateSpy).toHaveBeenCalledWith({
        queryKey: ["chat", "agentRun", "conv-OTHER"],
      });
      expect(invalidateSpy).toHaveBeenCalledWith({
        queryKey: ["chat", "conversation", "conv-OTHER"],
      });
    });

    it("skips event and does not invalidate queries when teammate_name is set", () => {
      const { queryClient, wrapper } = createWrapperWithClient();
      const invalidateSpy = vi.spyOn(queryClient, "invalidateQueries");

      renderHook(() => useAgentEvents("conv-1"), { wrapper });

      act(() => {
        emitEvent("agent:turn_completed", {
          context_type: "task_execution",
          context_id: "task-123",
          conversation_id: "conv-1",
          status: "turn_complete",
          teammate_name: "researcher",
        });
      });

      // No queries invalidated — teammate event was skipped
      expect(invalidateSpy).not.toHaveBeenCalled();
    });

    it("merges Claude provider metadata into cached conversation state", () => {
      const { queryClient, wrapper } = createWrapperWithClient();
      const conversation = makeConversation();
      queryClient.setQueryData(
        ["chat", "conversation", "conv-1"],
        { conversation, messages: [] }
      );
      queryClient.setQueryData(
        ["chat", "conversations", "task_execution", "task-123"],
        [conversation]
      );

      renderHook(() => useAgentEvents("conv-1"), { wrapper });

      act(() => {
        emitEvent("agent:turn_completed", {
          context_type: "task_execution",
          context_id: "task-123",
          conversation_id: "conv-1",
          status: "turn_complete",
          provider_harness: "claude",
          provider_session_id: "session-42",
          claude_session_id: "session-42",
        });
      });

      const conversationQuery = queryClient.getQueryData<{
        conversation: ChatConversation;
        messages: unknown[];
      }>(["chat", "conversation", "conv-1"]);
      const listQuery = queryClient.getQueryData<ChatConversation[]>([
        "chat",
        "conversations",
        "task_execution",
        "task-123",
      ]);

      expect(conversationQuery?.conversation.providerHarness).toBe("claude");
      expect(conversationQuery?.conversation.providerSessionId).toBe("session-42");
      expect(conversationQuery?.conversation.claudeSessionId).toBe("session-42");
      expect(listQuery?.[0]?.providerHarness).toBe("claude");
      expect(listQuery?.[0]?.providerSessionId).toBe("session-42");
      expect(listQuery?.[0]?.claudeSessionId).toBe("session-42");
    });
  });

  describe("turn_completed → run_completed sequence (process dies between turns)", () => {
    it("run_started → turn_completed → run_completed settles isAgentRunning=false", () => {
      const wrapper = createWrapper();
      renderHook(() => useAgentEvents("conv-1"), { wrapper });

      act(() => {
        emitEvent("agent:run_started", {
          run_id: "run-1",
          context_type: "task_execution",
          context_id: "task-123",
          conversation_id: "conv-1",
        });
      });

      expect(useChatStore.getState().agentStatus["task_execution:task-123"]).toBe("generating");

      act(() => {
        emitEvent("agent:turn_completed", {
          context_type: "task_execution",
          context_id: "task-123",
          conversation_id: "conv-1",
          status: "turn_complete",
        });
      });

      // Turn completed — waiting for user input
      expect(useChatStore.getState().agentStatus["task_execution:task-123"]).toBe("waiting_for_input");

      act(() => {
        emitEvent("agent:run_completed", {
          context_type: "task_execution",
          context_id: "task-123",
          conversation_id: "conv-1",
          status: "completed",
        });
      });

      // Process died — should be cleared
      expect(useChatStore.getState().agentStatus["task_execution:task-123"]).toBeUndefined();
    });

    it("rapid burst: turn_completed ×3 keeps agent alive throughout", () => {
      const wrapper = createWrapper();
      renderHook(() => useAgentEvents("conv-1"), { wrapper });

      act(() => {
        emitEvent("agent:run_started", {
          run_id: "run-1",
          context_type: "task_execution",
          context_id: "task-123",
          conversation_id: "conv-1",
        });
      });

      expect(useChatStore.getState().agentStatus["task_execution:task-123"]).toBe("generating");

      act(() => {
        emitEvent("agent:turn_completed", {
          context_type: "task_execution",
          context_id: "task-123",
          conversation_id: "conv-1",
          status: "turn_complete",
        });
        emitEvent("agent:turn_completed", {
          context_type: "task_execution",
          context_id: "task-123",
          conversation_id: "conv-1",
          status: "turn_complete",
        });
        emitEvent("agent:turn_completed", {
          context_type: "task_execution",
          context_id: "task-123",
          conversation_id: "conv-1",
          status: "turn_complete",
        });
      });

      // Still alive after burst — waiting for user input (last turn_completed wins)
      expect(useChatStore.getState().agentStatus["task_execution:task-123"]).toBe("waiting_for_input");
    });

    it("turn_completed followed by agent:error clears isAgentRunning", () => {
      const wrapper = createWrapper();
      renderHook(() => useAgentEvents("conv-1"), { wrapper });

      act(() => {
        emitEvent("agent:run_started", {
          run_id: "run-1",
          context_type: "task_execution",
          context_id: "task-123",
          conversation_id: "conv-1",
        });
      });

      expect(useChatStore.getState().agentStatus["task_execution:task-123"]).toBe("generating");

      act(() => {
        emitEvent("agent:turn_completed", {
          context_type: "task_execution",
          context_id: "task-123",
          conversation_id: "conv-1",
          status: "turn_complete",
        });
      });

      // Turn completed — waiting for user input
      expect(useChatStore.getState().agentStatus["task_execution:task-123"]).toBe("waiting_for_input");

      act(() => {
        emitEvent("agent:error", {
          context_type: "task_execution",
          context_id: "task-123",
          conversation_id: "conv-1",
          error: "Process crashed after turn",
        });
      });

      // Error clears the running state
      expect(useChatStore.getState().agentStatus["task_execution:task-123"]).toBeUndefined();
    });
  });

  describe("stale question cleanup", () => {
    const testQuestion: AskUserQuestionPayload = {
      requestId: "req-1",
      taskId: "task-123",
      sessionId: "task-123",
      question: "Approve team?",
      header: "Team",
      options: [{ label: "Yes", description: "Approve" }],
      multiSelect: false,
    };

    it("clears active question on agent:run_completed", () => {
      const wrapper = createWrapper();

      // Set up an active question for this context
      act(() => {
        useUiStore.getState().setActiveQuestion("task-123", testQuestion);
      });
      expect(useUiStore.getState().activeQuestions["task-123"]).toBeDefined();

      renderHook(() => useAgentEvents("conv-1"), { wrapper });

      act(() => {
        emitEvent("agent:run_completed", {
          context_type: "task",
          context_id: "task-123",
          conversation_id: "conv-1",
          status: "completed",
        });
      });

      // Question should be cleaned up when agent dies
      expect(useUiStore.getState().activeQuestions["task-123"]).toBeUndefined();
    });

    it("clears active question on agent:stopped", () => {
      const wrapper = createWrapper();

      act(() => {
        useUiStore.getState().setActiveQuestion("task-123", testQuestion);
      });

      renderHook(() => useAgentEvents("conv-1"), { wrapper });

      act(() => {
        emitEvent("agent:stopped", {
          context_type: "task",
          context_id: "task-123",
          conversation_id: "conv-1",
          agent_run_id: "run-1",
        });
      });

      expect(useUiStore.getState().activeQuestions["task-123"]).toBeUndefined();
    });

    it("clears active question on agent:error", () => {
      const wrapper = createWrapper();

      act(() => {
        useUiStore.getState().setActiveQuestion("task-123", testQuestion);
      });

      renderHook(() => useAgentEvents("conv-1"), { wrapper });

      act(() => {
        emitEvent("agent:error", {
          context_type: "task",
          context_id: "task-123",
          conversation_id: "conv-1",
          error: "Agent crashed",
        });
      });

      expect(useUiStore.getState().activeQuestions["task-123"]).toBeUndefined();
    });

    it("clears ideation session question on agent:run_completed", () => {
      const wrapper = createWrapper();
      const ideationQuestion = { ...testQuestion, sessionId: "session-789" };

      act(() => {
        useUiStore.getState().setActiveQuestion("session-789", ideationQuestion);
      });

      renderHook(() => useAgentEvents("conv-1"), { wrapper });

      act(() => {
        emitEvent("agent:run_completed", {
          context_type: "ideation",
          context_id: "session-789",
          conversation_id: "conv-1",
          status: "completed",
        });
      });

      expect(useUiStore.getState().activeQuestions["session-789"]).toBeUndefined();
    });

    it("does not affect questions for other contexts", () => {
      const wrapper = createWrapper();

      act(() => {
        useUiStore.getState().setActiveQuestion("task-123", testQuestion);
        useUiStore.getState().setActiveQuestion("task-456", { ...testQuestion, sessionId: "task-456" });
      });

      renderHook(() => useAgentEvents("conv-1"), { wrapper });

      act(() => {
        emitEvent("agent:run_completed", {
          context_type: "task",
          context_id: "task-123",
          conversation_id: "conv-1",
          status: "completed",
        });
      });

      // Only task-123 should be cleared
      expect(useUiStore.getState().activeQuestions["task-123"]).toBeUndefined();
      expect(useUiStore.getState().activeQuestions["task-456"]).toBeDefined();
    });
  });

  describe("cleanup", () => {
    it("unsubscribes from events on unmount", () => {
      const wrapper = createWrapper();
      const { unmount } = renderHook(() => useAgentEvents("conv-1"), { wrapper });

      // Events should be registered
      expect(listeners.get("agent:run_started")?.size).toBe(1);
      expect(listeners.get("agent:run_completed")?.size).toBe(1);
      expect(listeners.get("agent:stopped")?.size).toBe(1);
      expect(listeners.get("agent:error")?.size).toBe(1);

      unmount();

      // After unmount, listeners should be cleared
      expect(listeners.get("agent:run_started")?.size ?? 0).toBe(0);
      expect(listeners.get("agent:run_completed")?.size ?? 0).toBe(0);
      expect(listeners.get("agent:stopped")?.size ?? 0).toBe(0);
      expect(listeners.get("agent:error")?.size ?? 0).toBe(0);
    });

    it("registers turn_completed listener on mount and unregisters on unmount", () => {
      const wrapper = createWrapper();
      const { unmount } = renderHook(() => useAgentEvents("conv-1"), { wrapper });

      expect(listeners.get("agent:turn_completed")?.size).toBe(1);

      unmount();

      expect(listeners.get("agent:turn_completed")?.size ?? 0).toBe(0);
    });
  });

  describe("watchdog — stuck generating state recovery", () => {
    beforeEach(() => {
      vi.useFakeTimers();
    });

    afterEach(() => {
      vi.useRealTimers();
    });

    it("fires after 5 minutes of inactivity and forces idle", () => {
      const wrapper = createWrapper();
      renderHook(() => useAgentEvents(null), { wrapper });

      // Put a context into generating with a timestamp at t=0
      act(() => {
        useChatStore.getState().setAgentStatus("session:abc", "generating");
        useChatStore.getState().updateLastAgentEvent("session:abc");
      });

      expect(useChatStore.getState().agentStatus["session:abc"]).toBe("generating");

      // Advance 5 min (300s) — check at 300s: elapsed = 300000, NOT > 300000, no fire
      act(() => {
        vi.advanceTimersByTime(300_000);
      });
      expect(useChatStore.getState().agentStatus["session:abc"]).toBe("generating");

      // Advance one more interval (30s) — check at 330s: elapsed = 330000 > 300000 → fires
      act(() => {
        vi.advanceTimersByTime(30_000);
      });

      // Watchdog should have forced idle
      expect(useChatStore.getState().agentStatus["session:abc"]).toBeUndefined();
    });

    it("resets on message_created — does NOT fire while events keep coming", () => {
      const wrapper = createWrapper();
      renderHook(() => useAgentEvents("conv-1"), { wrapper });

      // Start generating at t=0
      act(() => {
        useChatStore.getState().setAgentStatus("session:xyz", "generating");
        useChatStore.getState().updateLastAgentEvent("session:xyz");
      });

      // Advance 4 min without a watchdog-triggering gap
      act(() => {
        vi.advanceTimersByTime(240_000);
      });

      // Emit message_created at t=240s — resets the watchdog timer for this context
      act(() => {
        emitEvent("agent:message_created", {
          context_type: "ideation",
          context_id: "xyz",
          conversation_id: "conv-1",
          message_id: "msg-heartbeat",
          role: "assistant",
          content: "still alive",
        });
      });

      // Advance another 4 min (to t=480s) — only 240s since the reset → no fire
      act(() => {
        vi.advanceTimersByTime(240_000);
      });

      // Should still be generating: last event was at t=240s, only 240s ago
      expect(useChatStore.getState().agentStatus["session:xyz"]).toBe("generating");
    });

    it("does NOT fire during active event flow", () => {
      const wrapper = createWrapper();
      renderHook(() => useAgentEvents("conv-1"), { wrapper });

      // Start with run_started at t=0
      act(() => {
        emitEvent("agent:run_started", {
          run_id: "run-1",
          context_type: "ideation",
          context_id: "active-session",
          conversation_id: "conv-1",
        });
      });

      expect(useChatStore.getState().agentStatus["session:active-session"]).toBe("generating");

      // Emit a message every 30s for 10 intervals (5 min total)
      // Each message resets the watchdog timer so it never fires
      for (let i = 0; i < 10; i++) {
        act(() => {
          // Advance one watchdog interval
          vi.advanceTimersByTime(30_000);
          // Emit a message to reset the timer (simulates active streaming)
          emitEvent("agent:message_created", {
            context_type: "ideation",
            context_id: "active-session",
            conversation_id: "conv-1",
            message_id: `msg-${i}`,
            role: "assistant",
            content: `chunk ${i}`,
          });
        });
      }

      // 5 min passed, but events came every 30s — watchdog should NOT have fired
      expect(useChatStore.getState().agentStatus["session:active-session"]).toBe("generating");
    });

    it("does NOT fire when toolCallStartTimes has an active entry within 10-min ceiling", () => {
      const wrapper = createWrapper();
      renderHook(() => useAgentEvents(null), { wrapper });

      const now = Date.now();
      act(() => {
        // lastAgentEventTimestamp is past the 5-min watchdog timeout,
        // but the tool call itself started recently (within 10-min ceiling)
        useChatStore.setState((state) => ({
          ...state,
          agentStatus: { "session:abc": "generating" },
          lastAgentEventTimestamp: { "session:abc": now - 360_000 }, // 6 min ago
          toolCallStartTimes: { "session:abc": { "tool-1": now - 60_000 } }, // 1 min ago — active
        }));
      });

      act(() => {
        vi.advanceTimersByTime(30_000); // One check interval
      });

      // Watchdog should NOT have fired — tool call is still within 10-min ceiling
      expect(useChatStore.getState().agentStatus["session:abc"]).toBe("generating");
    });

    it("DOES fire when all tool calls exceed the 10-min ceiling", () => {
      const wrapper = createWrapper();
      renderHook(() => useAgentEvents(null), { wrapper });

      const now = Date.now();
      act(() => {
        // Set both lastAgentEventTimestamp and toolCall start to > 10 min ago
        useChatStore.setState((state) => ({
          ...state,
          agentStatus: { "session:stalled": "generating" },
          lastAgentEventTimestamp: { "session:stalled": now - 660_000 }, // 11 min ago
          toolCallStartTimes: { "session:stalled": { "tool-old": now - 660_000 } }, // also 11 min old
        }));
      });

      act(() => {
        vi.advanceTimersByTime(30_000); // One check interval
      });

      // All tool calls exceeded ceiling — watchdog should fire and reset to idle
      expect(useChatStore.getState().agentStatus["session:stalled"]).toBeUndefined();
    });

    it("does NOT fire during grace period after last tool completion", () => {
      const wrapper = createWrapper();
      renderHook(() => useAgentEvents(null), { wrapper });

      // Set stale lastAgentEventTimestamp (past watchdog timeout)
      act(() => {
        useChatStore.setState((state) => ({
          ...state,
          agentStatus: { "session:grace": "generating" },
          lastAgentEventTimestamp: { "session:grace": Date.now() - 360_000 }, // 6 min ago
        }));
      });

      // Advance to 1ms before the check fires (check fires at 30_000ms)
      act(() => { vi.advanceTimersByTime(29_999); });

      // Set completion timestamp to "just now" — it will be <1ms old when check fires
      act(() => {
        useChatStore.setState((state) => ({
          ...state,
          lastToolCallCompletionTimestamp: { "session:grace": Date.now() },
        }));
      });

      // Advance the last 1ms — watchdog check fires, completion is <1ms old → within 5s grace
      act(() => { vi.advanceTimersByTime(1); });

      // Should not have fired — within grace period
      expect(useChatStore.getState().agentStatus["session:grace"]).toBe("generating");
    });

    it("does NOT fire when activeVerificationChildId is set for the parent session", () => {
      const wrapper = createWrapper();
      renderHook(() => useAgentEvents(null), { wrapper });

      const now = Date.now();
      act(() => {
        useChatStore.setState((state) => ({
          ...state,
          agentStatus: { "session:parent": "generating" },
          lastAgentEventTimestamp: { "session:parent": now - 360_000 }, // 6 min ago
        }));
        // Set verification child — synthetic generating status
        useIdeationStore.getState().setActiveVerificationChildId("parent", "child-session-id");
      });

      act(() => {
        vi.advanceTimersByTime(30_000);
      });

      // Should not have fired — verification child is active
      expect(useChatStore.getState().agentStatus["session:parent"]).toBe("generating");

      // Cleanup
      act(() => {
        useIdeationStore.getState().setActiveVerificationChildId("parent", null);
      });
    });

    it("clears toolCallStartTimes when firing stall reset", () => {
      const wrapper = createWrapper();
      renderHook(() => useAgentEvents(null), { wrapper });

      const stalledStart = Date.now() - 660_000; // 11 min ago (> 10-min ceiling)
      act(() => {
        useChatStore.setState((state) => ({
          ...state,
          agentStatus: { "session:clear-test": "generating" },
          lastAgentEventTimestamp: { "session:clear-test": Date.now() - 360_000 },
          toolCallStartTimes: { "session:clear-test": { "tool-stale": stalledStart } },
        }));
      });

      act(() => {
        vi.advanceTimersByTime(30_000);
      });

      // Status reset to idle AND toolCallStartTimes cleared
      expect(useChatStore.getState().agentStatus["session:clear-test"]).toBeUndefined();
      expect(useChatStore.getState().toolCallStartTimes["session:clear-test"]).toBeUndefined();
    });

    it("fires silently — status resets to idle without requiring external side effects", () => {
      // The watchdog previously called toast.warning(). We verify it no longer does
      // by confirming the stall fires cleanly: no unhandled exceptions, status becomes idle.
      const wrapper = createWrapper();
      renderHook(() => useAgentEvents(null), { wrapper });

      const now = Date.now();
      act(() => {
        useChatStore.setState((state) => ({
          ...state,
          agentStatus: { "session:silent": "generating" },
          lastAgentEventTimestamp: { "session:silent": now - 360_000 }, // 6 min ago
        }));
      });

      act(() => {
        vi.advanceTimersByTime(30_000);
      });

      // Status reset to idle — no exception thrown, no toast dependency needed
      expect(useChatStore.getState().agentStatus["session:silent"]).toBeUndefined();
    });
  });

  describe("verification child guard — parent status protected during verification", () => {
    it("PO1: run_completed with active verification child → re-asserts generating, skips termination", () => {
      const wrapper = createWrapper();

      act(() => {
        useChatStore.getState().setAgentRunning("session:parent-session", true);
        useIdeationStore.getState().setActiveVerificationChildId("parent-session", "child-session-id");
      });

      expect(useChatStore.getState().agentStatus["session:parent-session"]).toBe("generating");

      renderHook(() => useAgentEvents("conv-1"), { wrapper });

      act(() => {
        emitEvent("agent:run_completed", {
          context_type: "ideation",
          context_id: "parent-session",
          conversation_id: "conv-1",
          status: "completed",
        });
      });

      // Status must remain generating — verification child is still running
      expect(useChatStore.getState().agentStatus["session:parent-session"]).toBe("generating");

      // Cleanup
      act(() => {
        useIdeationStore.getState().setActiveVerificationChildId("parent-session", null);
      });
    });

    it("PO5: stopped with active verification child → re-asserts generating, skips termination", () => {
      const wrapper = createWrapper();

      act(() => {
        useChatStore.getState().setAgentRunning("session:parent-session", true);
        useIdeationStore.getState().setActiveVerificationChildId("parent-session", "child-session-id");
      });

      renderHook(() => useAgentEvents("conv-1"), { wrapper });

      act(() => {
        emitEvent("agent:stopped", {
          context_type: "ideation",
          context_id: "parent-session",
          conversation_id: "conv-1",
          agent_run_id: "run-1",
        });
      });

      // Status must remain generating — verification child is still running
      expect(useChatStore.getState().agentStatus["session:parent-session"]).toBe("generating");

      // Cleanup
      act(() => {
        useIdeationStore.getState().setActiveVerificationChildId("parent-session", null);
      });
    });

    it("error with active verification child → re-asserts generating, skips termination", () => {
      const wrapper = createWrapper();

      act(() => {
        useChatStore.getState().setAgentRunning("session:parent-session", true);
        useIdeationStore.getState().setActiveVerificationChildId("parent-session", "child-session-id");
      });

      renderHook(() => useAgentEvents("conv-1"), { wrapper });

      act(() => {
        emitEvent("agent:error", {
          context_type: "ideation",
          context_id: "parent-session",
          conversation_id: "conv-1",
          error: "Agent exited",
        });
      });

      // Status must remain generating — verification child is still running
      expect(useChatStore.getState().agentStatus["session:parent-session"]).toBe("generating");

      // Cleanup
      act(() => {
        useIdeationStore.getState().setActiveVerificationChildId("parent-session", null);
      });
    });

    it("PO2: turn_completed with active verification child → re-asserts generating, does not transition to waiting_for_input", () => {
      const wrapper = createWrapper();

      act(() => {
        useChatStore.getState().setAgentRunning("session:parent-session", true);
        useIdeationStore.getState().setActiveVerificationChildId("parent-session", "child-session-id");
      });

      expect(useChatStore.getState().agentStatus["session:parent-session"]).toBe("generating");

      renderHook(() => useAgentEvents("conv-1"), { wrapper });

      act(() => {
        emitEvent("agent:turn_completed", {
          context_type: "ideation",
          context_id: "parent-session",
          conversation_id: "conv-1",
          status: "turn_complete",
        });
      });

      // Status must remain generating — verification child is still running
      expect(useChatStore.getState().agentStatus["session:parent-session"]).toBe("generating");

      // Cleanup
      act(() => {
        useIdeationStore.getState().setActiveVerificationChildId("parent-session", null);
      });
    });

    it("turn_completed with NO verification child → transitions to waiting_for_input (normal flow unchanged)", () => {
      const wrapper = createWrapper();

      act(() => {
        useChatStore.getState().setAgentRunning("session:parent-session", true);
        // No verification child set
      });

      renderHook(() => useAgentEvents("conv-1"), { wrapper });

      act(() => {
        emitEvent("agent:turn_completed", {
          context_type: "ideation",
          context_id: "parent-session",
          conversation_id: "conv-1",
          status: "turn_complete",
        });
      });

      // Normal flow: transitions to waiting_for_input
      expect(useChatStore.getState().agentStatus["session:parent-session"]).toBe("waiting_for_input");
    });

    it("run_completed with NO verification child → clears to idle (normal flow unchanged)", () => {
      const wrapper = createWrapper();

      act(() => {
        useChatStore.getState().setAgentRunning("session:parent-session", true);
        // No verification child set
      });

      renderHook(() => useAgentEvents("conv-1"), { wrapper });

      act(() => {
        emitEvent("agent:run_completed", {
          context_type: "ideation",
          context_id: "parent-session",
          conversation_id: "conv-1",
          status: "completed",
        });
      });

      // Normal flow: status cleared
      expect(useChatStore.getState().agentStatus["session:parent-session"]).toBeUndefined();
    });

    it("non-ideation run_completed is not guarded even if unrelated verification child exists", () => {
      const wrapper = createWrapper();

      act(() => {
        useChatStore.getState().setAgentRunning("task_execution:task-abc", true);
        // Verification child on some ideation session (unrelated to this event)
        useIdeationStore.getState().setActiveVerificationChildId("some-session", "child-id");
      });

      renderHook(() => useAgentEvents("conv-1"), { wrapper });

      act(() => {
        emitEvent("agent:run_completed", {
          context_type: "task_execution",
          context_id: "task-abc",
          conversation_id: "conv-1",
          status: "completed",
        });
      });

      // Non-ideation context: normal termination applies
      expect(useChatStore.getState().agentStatus["task_execution:task-abc"]).toBeUndefined();

      // Cleanup
      act(() => {
        useIdeationStore.getState().setActiveVerificationChildId("some-session", null);
      });
    });

    it("child run_completed clears activeVerificationChildId but lastVerificationChildId retains child ID", () => {
      const wrapper = createWrapper();

      act(() => {
        useIdeationStore.getState().setActiveVerificationChildId("parent-session", "child-session-id");
        useIdeationStore.getState().setLastVerificationChildId("parent-session", "child-session-id");
      });

      expect(useIdeationStore.getState().activeVerificationChildId["parent-session"]).toBe("child-session-id");
      expect(useIdeationStore.getState().lastVerificationChildId["parent-session"]).toBe("child-session-id");

      renderHook(() => useAgentEvents("conv-1"), { wrapper });

      act(() => {
        emitEvent("agent:run_completed", {
          context_type: "ideation",
          context_id: "child-session-id",
          conversation_id: "conv-1",
          status: "completed",
        });
      });

      // activeVerificationChildId is cleared on child termination
      expect(useIdeationStore.getState().activeVerificationChildId["parent-session"]).toBeNull();
      // lastVerificationChildId persists — display-only reference for the Verification tab
      expect(useIdeationStore.getState().lastVerificationChildId["parent-session"]).toBe("child-session-id");

      // Cleanup
      act(() => {
        useIdeationStore.getState().setLastVerificationChildId("parent-session", null);
      });
    });
  });

  describe("agent:task_started / agent:task_completed", () => {
    it("agent:task_started resets lastAgentEventTimestamp for matching context", () => {
      const wrapper = createWrapper();
      renderHook(() => useAgentEvents(null), { wrapper });

      act(() => {
        useChatStore.setState((state) => ({
          ...state,
          agentStatus: { "session:task-ctx": "generating" },
          lastAgentEventTimestamp: { "session:task-ctx": 100 }, // very old timestamp
        }));
      });

      act(() => {
        emitEvent("agent:task_started", {
          conversation_id: "conv-x",
          context_id: "task-ctx",
        });
      });

      // Timestamp should be updated to a recent value (> 100)
      const ts = useChatStore.getState().lastAgentEventTimestamp["session:task-ctx"] ?? 0;
      expect(ts).toBeGreaterThan(100);
    });

    it("agent:task_completed resets lastAgentEventTimestamp for matching context", () => {
      const wrapper = createWrapper();
      renderHook(() => useAgentEvents(null), { wrapper });

      act(() => {
        useChatStore.setState((state) => ({
          ...state,
          agentStatus: { "session:task-done": "generating" },
          lastAgentEventTimestamp: { "session:task-done": 100 }, // very old timestamp
        }));
      });

      act(() => {
        emitEvent("agent:task_completed", {
          conversation_id: "conv-x",
          context_id: "task-done",
        });
      });

      const ts = useChatStore.getState().lastAgentEventTimestamp["session:task-done"] ?? 0;
      expect(ts).toBeGreaterThan(100);
    });
  });
});
