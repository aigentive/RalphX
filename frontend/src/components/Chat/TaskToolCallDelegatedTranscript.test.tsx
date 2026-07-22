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
  it("surfaces active-state recovery failure instead of an empty transcript", async () => {
    vi.spyOn(chatApi, "getConversationActiveState").mockRejectedValue(
      new Error("active-state unavailable"),
    );
    vi.spyOn(chatApi, "getConversationMessagesPage").mockResolvedValue({
      conversation: {
        id: "child-conv-error",
        contextType: "project",
        contextId: "project-1",
        claudeSessionId: null,
        providerSessionId: null,
        providerHarness: "codex",
        upstreamProvider: null,
        providerProfile: null,
        title: "Delegated reviewer",
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
        conversationId="child-conv-error"
        delegatedAgentRunId="child-run-error"
        fallbackText={undefined}
      />,
    );

    expect(
      await screen.findByText("Unable to recover the delegated live state."),
    ).toBeInTheDocument();
    expect(screen.queryByText("No delegated output available.")).not.toBeInTheDocument();
  });

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

  it("hydrates legacy tool state and reconciles live child tools and tasks", async () => {
    vi.spyOn(chatApi, "getConversationActiveState").mockResolvedValue({
      is_active: true,
      runId: "run-child-tools",
      tool_calls: [
        {
          name: "LegacyInspect",
          id: "read-1",
          arguments: { file_path: "/tmp/recovered.ts" },
          result: "recovered contents",
        },
        { arguments: { ignored: true } },
      ],
      streaming_tasks: [],
      partial_text: "Recovered answer",
    });
    vi.spyOn(chatApi, "getConversationMessagesPage").mockResolvedValue({
      conversation: {
        id: "child-conv-tools",
        contextType: "project",
        contextId: "project-1",
        claudeSessionId: null,
        providerSessionId: "thread-tools",
        providerHarness: "codex",
        upstreamProvider: "openai",
        providerProfile: "openai",
        title: "Tool-using delegate",
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
        conversationId="child-conv-tools"
        delegatedAgentRunId="run-child-tools"
        fallbackText={undefined}
      />,
    );

    expect(await screen.findByText("Recovered answer")).toBeInTheDocument();
    await waitFor(() => {
      expect(screen.getAllByTestId("tool-call-indicator")).toHaveLength(1);
    });

    act(() => {
      emitEvent("agent:tool_call", {
        conversation_id: "another-conversation",
        run_id: "run-child-tools",
        tool_name: "Ignored",
        tool_id: "ignored-1",
      });
      emitEvent("agent:tool_call", {
        conversation_id: "child-conv-tools",
        run_id: "run-child-tools",
        tool_id: "ignored-without-name",
      });
      emitEvent("agent:tool_call", {
        conversation_id: "child-conv-tools",
        run_id: "run-child-tools",
        tool_name: "CustomWrite",
        tool_id: "write-1",
        arguments: { file_path: "/tmp/result.ts" },
      });
      emitEvent("agent:tool_call", {
        conversation_id: "child-conv-tools",
        run_id: "run-child-tools",
        tool_name: "result:write-1",
        result: "write complete",
      });
      emitEvent("agent:tool_call", {
        conversation_id: "child-conv-tools",
        run_id: "run-child-tools",
        tool_name: "result:missing-tool",
        result: "ignored result",
      });
      emitEvent("agent:task_started", {
        conversation_id: "child-conv-tools",
        run_id: "run-child-tools",
        tool_use_id: "nested-task-1",
        description: "Live nested review",
        subagent_type: "Explore",
        model: "gpt-5.4-mini",
        status: "running",
      });
      emitEvent("agent:task_completed", {
        conversation_id: "child-conv-tools",
        run_id: "run-child-tools",
        tool_use_id: "nested-task-1",
        description: "Live nested review",
        subagent_type: "Explore",
        model: "gpt-5.4-mini",
        status: "completed",
        text_output: "Nested review complete",
        total_tokens: 42,
        total_tool_use_count: 3,
        total_duration_ms: 1200,
      });
      emitEvent("agent:task_started", {
        conversation_id: "child-conv-tools",
        run_id: "stale-child-run",
        tool_use_id: "stale-task",
        description: "Stale nested review",
        status: "running",
      });
    });

    expect(screen.getAllByTestId("tool-call-indicator")).toHaveLength(2);
    expect(screen.getByText("Live nested review")).toBeInTheDocument();
    expect(screen.queryByText("Stale nested review")).not.toBeInTheDocument();

    act(() => {
      emitEvent("agent:message_created", {
        conversation_id: "another-conversation",
        role: "user",
      });
      emitEvent("agent:message_created", {
        conversation_id: "child-conv-tools",
        role: "user",
      });
    });

    expect(screen.queryByText("Recovered answer")).not.toBeInTheDocument();
  });

  it("coalesces nested provider and lifecycle aliases for one delegation job", async () => {
    vi.spyOn(chatApi, "getConversationActiveState").mockResolvedValue({
      is_active: true,
      runId: "run-child-aliases",
      tool_calls: [],
      streaming_tasks: [],
      partial_text: "",
    });
    vi.spyOn(chatApi, "getConversationMessagesPage").mockResolvedValue({
      conversation: {
        id: "child-conv-aliases",
        contextType: "project",
        contextId: "project-1",
        claudeSessionId: null,
        providerSessionId: "thread-aliases",
        providerHarness: "codex",
        upstreamProvider: "openai",
        providerProfile: "openai",
        title: "Nested delegation aliases",
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
        conversationId="child-conv-aliases"
        delegatedAgentRunId="run-child-aliases"
        fallbackText={undefined}
      />,
    );
    expect(await screen.findByText("Waiting for delegated output...")).toBeInTheDocument();

    act(() => {
      emitEvent("agent:tool_call", {
        conversation_id: "child-conv-aliases",
        run_id: "run-child-aliases",
        tool_name: "delegate_start",
        tool_id: "provider-nested",
        arguments: {
          title: "Inspect nested reconciliation",
          prompt: "Inspect nested reconciliation",
        },
      });
      emitEvent("agent:task_started", {
        conversation_id: "child-conv-aliases",
        run_id: "run-child-aliases",
        tool_use_id: "delegate-job:nested-job",
        description: "ralphx-general-explorer",
        subagent_type: "delegated",
        delegated_job_id: "nested-job",
        status: "running",
      });
    });

    await waitFor(() => {
      expect(screen.getAllByTestId("task-tool-call-card")).toHaveLength(1);
    });
    expect(screen.getAllByText("Inspect nested reconciliation")).toHaveLength(2);
    expect(screen.queryByText("ralphx-general-explorer")).not.toBeInTheDocument();
  });

  it("shows fallback text after an empty delegated conversation settles", async () => {
    vi.spyOn(chatApi, "getConversationActiveState").mockResolvedValue({
      is_active: false,
      tool_calls: [],
      streaming_tasks: [],
      partial_text: "",
    });
    vi.spyOn(chatApi, "getConversationMessagesPage").mockResolvedValue({
      conversation: {
        id: "child-conv-fallback",
        contextType: "project",
        contextId: "project-1",
        claudeSessionId: null,
        providerSessionId: null,
        providerHarness: "codex",
        upstreamProvider: "openai",
        providerProfile: "openai",
        title: "Fallback delegate",
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
        conversationId="child-conv-fallback"
        fallbackText="Delegated fallback output"
      />,
    );

    expect(await screen.findByText("Delegated fallback output")).toBeInTheDocument();
  });

  it("recovers nested child tasks from active state", async () => {
    vi.spyOn(chatApi, "getConversationActiveState").mockResolvedValue({
      is_active: true,
      runId: "run-child-tasks",
      tool_calls: [],
      streaming_tasks: [{
        tool_use_id: "task-nested-1",
        description: "Nested audit",
        subagent_type: "Explore",
        model: "sonnet",
        status: "running",
      }],
      partial_text: "",
    });
    vi.spyOn(chatApi, "getConversationMessagesPage").mockResolvedValue({
      conversation: {
        id: "child-conv-tasks",
        contextType: "project",
        contextId: "project-1",
        claudeSessionId: null,
        providerSessionId: "thread-tasks",
        providerHarness: "claude",
        upstreamProvider: "anthropic",
        providerProfile: null,
        title: "Nested task delegate",
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
        conversationId="child-conv-tasks"
        delegatedAgentRunId="run-child-tasks"
        fallbackText={undefined}
      />,
    );

    expect(await screen.findByText("Nested audit")).toBeInTheDocument();
  });

  it("rejects recovered state and live events from a different child run", async () => {
    vi.spyOn(chatApi, "getConversationActiveState").mockResolvedValue({
      is_active: true,
      runId: "stale-run",
      tool_calls: [],
      streaming_tasks: [],
      partial_text: "stale recovered text",
    });
    vi.spyOn(chatApi, "getConversationMessagesPage").mockResolvedValue({
      conversation: {
        id: "child-conv-run-scope",
        contextType: "project",
        contextId: "project-1",
        claudeSessionId: null,
        providerSessionId: "thread-run-scope",
        providerHarness: "codex",
        upstreamProvider: "openai",
        providerProfile: "openai",
        title: "Run-scoped delegate",
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
        conversationId="child-conv-run-scope"
        delegatedAgentRunId="current-run"
        fallbackText={undefined}
      />,
    );

    expect(await screen.findByText("No delegated output available.")).toBeInTheDocument();
    expect(screen.queryByText("stale recovered text")).not.toBeInTheDocument();

    act(() => {
      emitEvent("agent:chunk", {
        conversation_id: "child-conv-run-scope",
        run_id: "stale-run",
        text: "wrong run event",
      });
      emitEvent("agent:chunk", {
        conversation_id: "child-conv-run-scope",
        run_id: "current-run",
        text: "current run event",
      });
    });

    expect(screen.queryByText("wrong run event")).not.toBeInTheDocument();
    expect(screen.getByText("current run event")).toBeInTheDocument();
  });
});
