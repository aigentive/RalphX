import { describe, it, expect, vi, afterEach } from "vitest";
import { render, screen, waitFor, act } from "@testing-library/react";
import { QueryClientProvider } from "@tanstack/react-query";
import { TaskToolCallDelegatedTranscript } from "./TaskToolCallDelegatedTranscript";
import { createTestQueryClient } from "@/test/store-utils";
import { chatApi, type ChatMessageResponse } from "@/api/chat";

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

function renderWithQueryClient(ui: React.ReactElement) {
  const queryClient = createTestQueryClient();
  return render(
    <QueryClientProvider client={queryClient}>{ui}</QueryClientProvider>,
  );
}

afterEach(() => {
  listeners.clear();
  vi.restoreAllMocks();
});

describe("TaskToolCallDelegatedTranscript", () => {
  it("refetches when the delegated conversation receives a new message_created event", async () => {
    const getConversationMessagesPageSpy = vi
      .spyOn(chatApi, "getConversationMessagesPage")
      .mockResolvedValueOnce({
        conversation: {
          id: "child-conv-1",
          contextType: "project",
          contextId: "project-1",
          claudeSessionId: null,
          providerSessionId: "thread-123",
          providerHarness: "codex",
          upstreamProvider: "openai",
          providerProfile: "openai",
          title: "Delegated reviewer",
          messageCount: 1,
          lastMessageAt: "2026-04-12T10:00:00Z",
          createdAt: "2026-04-12T10:00:00Z",
          updatedAt: "2026-04-12T10:00:00Z",
        },
        messages: [
          {
            id: "child-msg-1",
            sessionId: null,
            projectId: null,
            taskId: null,
            role: "assistant",
            content: "First delegated update",
            metadata: null,
            parentMessageId: null,
            conversationId: "child-conv-1",
            toolCalls: null,
            contentBlocks: null,
            sender: null,
            createdAt: "2026-04-12T10:00:00Z",
          } satisfies ChatMessageResponse,
        ],
        limit: 40,
        offset: 0,
        totalMessageCount: 1,
        hasOlder: false,
      })
      .mockResolvedValueOnce({
        conversation: {
          id: "child-conv-1",
          contextType: "project",
          contextId: "project-1",
          claudeSessionId: null,
          providerSessionId: "thread-123",
          providerHarness: "codex",
          upstreamProvider: "openai",
          providerProfile: "openai",
          title: "Delegated reviewer",
          messageCount: 2,
          lastMessageAt: "2026-04-12T10:00:06Z",
          createdAt: "2026-04-12T10:00:00Z",
          updatedAt: "2026-04-12T10:00:06Z",
        },
        messages: [
          {
            id: "child-msg-1",
            sessionId: null,
            projectId: null,
            taskId: null,
            role: "assistant",
            content: "First delegated update",
            metadata: null,
            parentMessageId: null,
            conversationId: "child-conv-1",
            toolCalls: null,
            contentBlocks: null,
            sender: null,
            createdAt: "2026-04-12T10:00:00Z",
          } satisfies ChatMessageResponse,
          {
            id: "child-msg-2",
            sessionId: null,
            projectId: null,
            taskId: null,
            role: "assistant",
            content: "Second delegated update",
            metadata: null,
            parentMessageId: null,
            conversationId: "child-conv-1",
            toolCalls: null,
            contentBlocks: null,
            sender: null,
            createdAt: "2026-04-12T10:00:06Z",
          } satisfies ChatMessageResponse,
        ],
        limit: 40,
        offset: 0,
        totalMessageCount: 2,
        hasOlder: false,
      });

    renderWithQueryClient(
      <TaskToolCallDelegatedTranscript
        conversationId="child-conv-1"
        fallbackText="fallback"
      />,
    );

    expect(await screen.findByText("First delegated update")).toBeInTheDocument();
    expect(getConversationMessagesPageSpy).toHaveBeenCalledTimes(1);
    expect(getConversationMessagesPageSpy).toHaveBeenCalledWith("child-conv-1", 40, 0);

    await act(async () => {
      emitEvent("agent:message_created", {
        conversation_id: "child-conv-1",
      });
    });

    await waitFor(() => expect(getConversationMessagesPageSpy).toHaveBeenCalledTimes(2));
    await waitFor(
      () => {
        expect(screen.getByText("Second delegated update")).toBeInTheDocument();
      },
      { timeout: 5_000 },
    );
  });

  it("recovers live child text, appends matching chunks once, and hands off to persisted history", async () => {
    const conversation = {
      id: "child-conv-live",
      contextType: "project",
      contextId: "project-1",
      claudeSessionId: null,
      providerSessionId: "thread-live",
      providerHarness: "codex",
      upstreamProvider: "openai",
      providerProfile: "openai",
      title: "Live delegated reviewer",
      messageCount: 0,
      lastMessageAt: null,
      createdAt: "2026-04-12T10:00:00Z",
      updatedAt: "2026-04-12T10:00:00Z",
    };
    vi.spyOn(chatApi, "getConversationActiveState").mockResolvedValue({
      is_active: true,
      tool_calls: [],
      streaming_tasks: [],
      partial_text: "Recovered delegated text",
    });
    vi.spyOn(chatApi, "getConversationMessagesPage")
      .mockResolvedValueOnce({
        conversation,
        messages: [],
        limit: 40,
        offset: 0,
        totalMessageCount: 0,
        hasOlder: false,
      })
      .mockResolvedValueOnce({
        conversation: {
          ...conversation,
          messageCount: 1,
          lastMessageAt: "2026-04-12T10:00:06Z",
          updatedAt: "2026-04-12T10:00:06Z",
        },
        messages: [
          {
            id: "child-msg-final",
            sessionId: null,
            projectId: null,
            taskId: null,
            role: "assistant",
            content: "Recovered delegated text and live suffix",
            metadata: null,
            parentMessageId: null,
            conversationId: "child-conv-live",
            toolCalls: null,
            contentBlocks: null,
            sender: null,
            createdAt: "2026-04-12T10:00:06Z",
          } satisfies ChatMessageResponse,
        ],
        limit: 40,
        offset: 0,
        totalMessageCount: 1,
        hasOlder: false,
      });

    renderWithQueryClient(
      <TaskToolCallDelegatedTranscript
        conversationId="child-conv-live"
        fallbackText={undefined}
      />,
    );

    expect(await screen.findByText("Recovered delegated text")).toBeInTheDocument();

    act(() => {
      emitEvent("agent:chunk", {
        conversation_id: "unrelated-conversation",
        text: " ignored",
      });
      emitEvent("agent:chunk", {
        conversation_id: "child-conv-live",
        text: " and live suffix",
      });
    });

    expect(screen.getByText("Recovered delegated text and live suffix")).toBeInTheDocument();
    expect(screen.queryByText(/ignored/)).not.toBeInTheDocument();

    await act(async () => {
      emitEvent("agent:message_created", {
        conversation_id: "child-conv-live",
        message_id: "child-msg-final",
        role: "assistant",
      });
    });

    expect(screen.getByText("Recovered delegated text and live suffix")).toBeInTheDocument();
    await waitFor(() => {
      expect(screen.getByTestId("delegated-conversation-transcript")).toBeInTheDocument();
    });
    expect(screen.getAllByText("Recovered delegated text and live suffix")).toHaveLength(1);
  });

  it("keeps persisted history visible while rendering and then handing off current live text", async () => {
    const conversation = {
      id: "child-conv-with-history",
      contextType: "project",
      contextId: "project-1",
      claudeSessionId: null,
      providerSessionId: "thread-with-history",
      providerHarness: "codex",
      upstreamProvider: "openai",
      providerProfile: "openai",
      title: "Delegated reviewer with history",
      messageCount: 1,
      lastMessageAt: "2026-04-12T10:00:00Z",
      createdAt: "2026-04-12T10:00:00Z",
      updatedAt: "2026-04-12T10:00:00Z",
    };
    const earlierMessage = {
      id: "child-msg-earlier",
      sessionId: null,
      projectId: null,
      taskId: null,
      role: "assistant" as const,
      content: "Earlier delegated update",
      metadata: null,
      parentMessageId: null,
      conversationId: "child-conv-with-history",
      toolCalls: null,
      contentBlocks: null,
      sender: null,
      createdAt: "2026-04-12T10:00:00Z",
    } satisfies ChatMessageResponse;
    vi.spyOn(chatApi, "getConversationActiveState").mockResolvedValue({
      is_active: true,
      tool_calls: [],
      streaming_tasks: [],
      partial_text: "Current delegated answer",
    });
    vi.spyOn(chatApi, "getConversationMessagesPage")
      .mockResolvedValueOnce({
        conversation,
        messages: [earlierMessage],
        limit: 40,
        offset: 0,
        totalMessageCount: 1,
        hasOlder: false,
      })
      .mockResolvedValueOnce({
        conversation: {
          ...conversation,
          messageCount: 2,
          lastMessageAt: "2026-04-12T10:00:06Z",
          updatedAt: "2026-04-12T10:00:06Z",
        },
        messages: [
          earlierMessage,
          {
            ...earlierMessage,
            id: "child-msg-current",
            content: "Current delegated answer and final suffix",
            createdAt: "2026-04-12T10:00:06Z",
          },
        ],
        limit: 40,
        offset: 0,
        totalMessageCount: 2,
        hasOlder: false,
      });

    renderWithQueryClient(
      <TaskToolCallDelegatedTranscript
        conversationId="child-conv-with-history"
        fallbackText={undefined}
      />,
    );

    expect(await screen.findByText("Earlier delegated update")).toBeInTheDocument();
    expect(await screen.findByText("Current delegated answer")).toBeInTheDocument();

    act(() => {
      emitEvent("agent:chunk", {
        conversation_id: "child-conv-with-history",
        text: " and final suffix",
      });
    });

    expect(screen.getByText("Current delegated answer and final suffix")).toBeInTheDocument();
    expect(screen.getByText("Earlier delegated update")).toBeInTheDocument();

    await act(async () => {
      emitEvent("agent:message_created", {
        conversation_id: "child-conv-with-history",
        message_id: "child-msg-current",
        role: "assistant",
      });
    });

    await waitFor(() => {
      expect(screen.getAllByText("Current delegated answer and final suffix")).toHaveLength(1);
    });
    expect(screen.getByText("Earlier delegated update")).toBeInTheDocument();
  });

  it("shows an explicit awaiting-output state before the first delegated chunk", async () => {
    vi.spyOn(chatApi, "getConversationActiveState").mockResolvedValue({
      is_active: true,
      tool_calls: [],
      streaming_tasks: [],
      partial_text: "",
    });
    vi.spyOn(chatApi, "getConversationMessagesPage").mockResolvedValue({
      conversation: {
        id: "child-conv-waiting",
        contextType: "project",
        contextId: "project-1",
        claudeSessionId: null,
        providerSessionId: "thread-waiting",
        providerHarness: "codex",
        upstreamProvider: "openai",
        providerProfile: "openai",
        title: "Waiting delegated reviewer",
        messageCount: 0,
        lastMessageAt: null,
        createdAt: "2026-04-12T10:00:00Z",
        updatedAt: "2026-04-12T10:00:00Z",
      },
      messages: [],
      limit: 40,
      offset: 0,
      totalMessageCount: 0,
      hasOlder: false,
    });

    renderWithQueryClient(
      <TaskToolCallDelegatedTranscript
        conversationId="child-conv-waiting"
        fallbackText={undefined}
      />,
    );

    expect(await screen.findByText("Waiting for delegated output...")).toBeInTheDocument();
  });
});
