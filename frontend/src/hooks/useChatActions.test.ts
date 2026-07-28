import { describe, it, expect, vi, beforeEach } from "vitest";
import { renderHook, act } from "@testing-library/react";
import { useChatActions } from "./useChatActions";
import type { ContextType } from "@/types/chat-conversation";

// ============================================================================
// Mocks
// ============================================================================

const mockToastError = vi.fn();
const mockToastWarning = vi.fn();
vi.mock("sonner", () => ({
  toast: {
    error: (...args: unknown[]) => mockToastError(...args),
    warning: (...args: unknown[]) => mockToastWarning(...args),
  },
}));

const mockInvalidateQueries = vi.fn();
const mockSetQueryData = vi.fn();
vi.mock("@tanstack/react-query", () => ({
  useQueryClient: () => ({
    invalidateQueries: mockInvalidateQueries,
    setQueryData: mockSetQueryData,
  }),
}));

const mockActions = {
  queueMessage: vi.fn(),
  deleteQueuedMessage: vi.fn(),
  startEditingQueuedMessage: vi.fn(),
  setActiveConversation: vi.fn(),
  setAgentRunning: vi.fn(),
  setSending: vi.fn(),
};
vi.mock("@/stores/chatStore", () => ({
  useChatStore: (selector: (state: typeof mockActions) => unknown) => selector(mockActions),
}));

const mockSendAgentMessage = vi.fn();
const mockDeleteQueuedAgentMessage = vi.fn();
const mockSendQueuedAgentMessageNow = vi.fn();
const mockStopAgent = vi.fn();

vi.mock("@/api/chat", () => ({
  chatApi: {
    sendAgentMessage: (...args: unknown[]) => mockSendAgentMessage(...args),
    deleteQueuedAgentMessage: (...args: unknown[]) => mockDeleteQueuedAgentMessage(...args),
    sendQueuedAgentMessageNow: (...args: unknown[]) => mockSendQueuedAgentMessageNow(...args),
  },
  stopAgent: (...args: unknown[]) => mockStopAgent(...args),
}));

const mockRecoverTaskExecution = vi.fn();
vi.mock("@/api/recovery", () => ({
  recoverTaskExecution: (...args: unknown[]) => mockRecoverTaskExecution(...args),
}));

const selectionSnapshot = {
  sourceType: "artifact" as const,
  sourceKind: "plan" as const,
  sourceId: "plan-v3",
  artifactVersion: 3,
  startLine: 7,
  endLine: 8,
  content: "first\nsecond",
};

const mockSpawnSessionNamer = vi.fn();
vi.mock("@/api/ideation", () => ({
  ideationApi: {
    sessions: {
      spawnSessionNamer: (...args: unknown[]) => mockSpawnSessionNamer(...args),
    },
  },
}));

const mockAddOptimisticUserMessageToConversationCache = vi.fn(() => ({
  id: "optimistic:conv-1:test",
}));
const mockRemoveOptimisticMessageFromConversationCache = vi.fn();

vi.mock("@/hooks/useChat", () => ({
  chatKeys: {
    all: ["chat"] as const,
    conversations: () => ["chat", "conversations"] as const,
    conversation: (id: string) => ["chat", "conversations", id] as const,
    conversationHistory: (id: string) =>
      ["chat", "conversations", id, "history"] as const,
    conversationList: (ct: string, ci: string) => ["chat", "conversations", ct, ci] as const,
  },
  invalidateConversationDataQueries: (_queryClient: unknown, conversationId: string) => {
    mockInvalidateQueries({ queryKey: ["chat", "conversations", conversationId] });
    mockInvalidateQueries({ queryKey: ["chat", "conversations", conversationId, "history"] });
  },
  addOptimisticUserMessageToConversationCache: (
    ...args: unknown[]
  ) => mockAddOptimisticUserMessageToConversationCache(...args),
  removeOptimisticMessageFromConversationCache: (
    ...args: unknown[]
  ) => mockRemoveOptimisticMessageFromConversationCache(...args),
}));

// ============================================================================
// Helpers
// ============================================================================

interface SetupOptions {
  contextType?: ContextType;
  contextId?: string;
  storeContextKey?: string;
  selectedTaskId?: string | undefined;
  ideationSessionId?: string | undefined;
  queueContextId?: string | undefined;
  isPending?: boolean;
  messageCount?: number;
  activeConversationId?: string | null | undefined;
  onUserMessageSent?: Parameters<typeof useChatActions>[0]["onUserMessageSent"];
  onPersonaUnavailable?: Parameters<typeof useChatActions>[0]["onPersonaUnavailable"];
}

function setup(opts: SetupOptions = {}) {
  const {
    contextType = "task",
    contextId = "task-1",
    storeContextKey = "task:task-1",
    selectedTaskId = undefined,
    ideationSessionId = undefined,
    queueContextId = undefined,
    isPending = false,
    messageCount = 5,
    activeConversationId = undefined,
    onUserMessageSent = undefined,
    onPersonaUnavailable = undefined,
  } = opts;

  const mutateAsync = vi.fn().mockResolvedValue({
    conversationId: "conv-1",
    agentRunId: "run-1",
    isNewConversation: false,
    wasQueued: false,
    queuedAsPending: false,
  });

  const { result } = renderHook(() =>
    useChatActions({
      contextType,
      contextId,
      queueContextId,
      storeContextKey,
      selectedTaskId,
      ideationSessionId,
      sendMessage: { isPending, mutateAsync },
      activeConversationId,
      messageCount,
      onUserMessageSent,
      onPersonaUnavailable,
    })
  );

  return { result, mutateAsync };
}

// ============================================================================
// Tests
// ============================================================================

describe("useChatActions", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockSendAgentMessage.mockResolvedValue({
      conversationId: "conv-1",
      agentRunId: "run-1",
      isNewConversation: false,
      wasQueued: false,
      queuedAsPending: false,
    });
    mockDeleteQueuedAgentMessage.mockResolvedValue(true);
    mockSendQueuedAgentMessageNow.mockResolvedValue({
      conversationId: "conv-1",
      agentRunId: "run-now",
      isNewConversation: false,
      wasQueued: false,
      queuedAsPending: false,
    });
    mockStopAgent.mockResolvedValue(true);
    mockRecoverTaskExecution.mockResolvedValue(true);
    mockSpawnSessionNamer.mockResolvedValue(undefined);
  });

  // ── handleSend ──────────────────────────────────────────────────

  describe("handleSend", () => {
    it("calls sendMessage.mutateAsync with content", async () => {
      const { result, mutateAsync } = setup();

      await act(async () => {
        await result.current.handleSend("hello world");
      });

      expect(mutateAsync).toHaveBeenCalledWith({ content: "hello world", attachmentIds: undefined });
    });

    it("passes Team intent to the send mutation", async () => {
      const { result, mutateAsync } = setup();

      await act(async () => {
        await result.current.handleSend("hello Team", undefined, {
          teamIntent: { coordinationMode: "rx_native_team" },
        });
      });

      expect(mutateAsync).toHaveBeenCalledWith({
        content: "hello Team",
        attachmentIds: undefined,
        teamIntent: { coordinationMode: "rx_native_team" },
      });
    });

    it("preserves attachment IDs when a send is queued", async () => {
      const { result, mutateAsync } = setup();
      mutateAsync.mockResolvedValue({
        conversationId: "conv-1",
        agentRunId: "run-1",
        isNewConversation: false,
        wasQueued: true,
        queuedAsPending: false,
        queuedMessageId: "q-with-file",
      });

      await act(async () => {
        await result.current.handleSend("review attached file", ["att-1"]);
      });

      expect(mutateAsync).toHaveBeenCalledWith({
        content: "review attached file",
        attachmentIds: ["att-1"],
      });
      expect(mockActions.queueMessage).toHaveBeenCalledWith(
        "task:task-1",
        "review attached file",
        "q-with-file",
        ["att-1"]
      );
    });

    it("passes composer integration references to the post-send callback", async () => {
      const onUserMessageSent = vi.fn();
      const jiraReference = {
        provider: "atlassian",
        kind: "jira",
        id: "RX-42",
        key: "RX-42",
        title: "Fix composer references",
      };
      const { result } = setup({ onUserMessageSent });

      await act(async () => {
        await result.current.handleSend("work on jira", undefined, {
          integrationReferences: [jiraReference],
        });
      });

      expect(onUserMessageSent).toHaveBeenCalledWith({
        content: "work on jira",
        result: {
          conversationId: "conv-1",
          agentRunId: "run-1",
          isNewConversation: false,
          wasQueued: false,
          queuedAsPending: false,
        },
        composerIntegrationReferences: [jiraReference],
      });
    });

    it("does not send empty or whitespace-only strings", async () => {
      const { result, mutateAsync } = setup();

      await act(async () => {
        await result.current.handleSend("");
        await result.current.handleSend("   ");
        await result.current.handleSend("\n\t");
      });

      expect(mutateAsync).not.toHaveBeenCalled();
    });

    it("does not send when isPending is true", async () => {
      const { result, mutateAsync } = setup({ isPending: true });

      await act(async () => {
        await result.current.handleSend("hello");
      });

      expect(mutateAsync).not.toHaveBeenCalled();
    });

    it("suppresses the generic toast and reports persona-unavailable sends inline", async () => {
      const onPersonaUnavailable = vi.fn();
      const { result, mutateAsync } = setup({ onPersonaUnavailable });
      mutateAsync.mockRejectedValue(
        new Error("[Persona unavailable: Reviewer Voice was archived]"),
      );

      await act(async () => {
        await result.current.handleSend("continue the review");
      });

      expect(onPersonaUnavailable).toHaveBeenCalledWith(
        "[Persona unavailable: Reviewer Voice was archived]",
      );
      expect(mockToastError).not.toHaveBeenCalled();
    });

    it("review mode sends via chatApi.sendAgentMessage directly", async () => {
      const { result, mutateAsync } = setup({
        contextType: "review",
        contextId: "task-42",
        storeContextKey: "review:task-42",
        selectedTaskId: "task-42",
      });

      await act(async () => {
        await result.current.handleSend("looks good");
      });

      // Should use direct API, NOT the mutation
      expect(mutateAsync).not.toHaveBeenCalled();
      expect(mockSendAgentMessage).toHaveBeenCalledWith(
        "review",
        "task-42",
        "looks good",
        undefined,
        undefined
      );
      expect(mockActions.setSending).toHaveBeenCalledWith("review:task-42", true);
      expect(mockInvalidateQueries).toHaveBeenCalled();
    });

    it("review mode sends Team intent via direct send options", async () => {
      const { result, mutateAsync } = setup({
        contextType: "review",
        contextId: "task-42",
        storeContextKey: "review:task-42",
        selectedTaskId: "task-42",
      });

      await act(async () => {
        await result.current.handleSend("review with Team", undefined, {
          teamIntent: { coordinationMode: "rx_native_team" },
        });
      });

      expect(mutateAsync).not.toHaveBeenCalled();
      expect(mockSendAgentMessage).toHaveBeenCalledWith(
        "review",
        "task-42",
        "review with Team",
        undefined,
        { teamIntent: { coordinationMode: "rx_native_team" } },
      );
    });

    it("review mode sets activeConversation when isNewConversation is true", async () => {
      mockSendAgentMessage.mockResolvedValue({
        conversationId: "new-conv",
        agentRunId: "run-1",
        isNewConversation: true,
        wasQueued: false,
      });

      const { result } = setup({
        contextType: "review",
        contextId: "task-42",
        storeContextKey: "review:task-42",
        selectedTaskId: "task-42",
      });

      await act(async () => {
        await result.current.handleSend("review this");
      });

      expect(mockActions.setActiveConversation).toHaveBeenCalledWith("review:task-42", "new-conv");
    });

    it("review mode optimistically adds the user message to the active conversation", async () => {
      const { result } = setup({
        contextType: "review",
        contextId: "task-42",
        storeContextKey: "review:task-42",
        selectedTaskId: "task-42",
        activeConversationId: "conv-review",
      });

      await act(async () => {
        await result.current.handleSend("review this");
      });

      expect(mockAddOptimisticUserMessageToConversationCache).toHaveBeenCalledWith(
        expect.anything(),
        "conv-review",
        "review this"
      );
    });

    it("review mode includes selected references in optimistic message metadata", async () => {
      const { result } = setup({
        contextType: "review",
        contextId: "task-42",
        storeContextKey: "review:task-42",
        selectedTaskId: "task-42",
        activeConversationId: "conv-review",
      });

      await act(async () => {
        await result.current.handleSend("review this", undefined, {
          folderReferences: [
            {
              id: "folder-1",
              folderPath: "/work/brand-kit",
              displayName: "brand-kit",
            },
          ],
          projectReferences: [{ path: "src/main.ts", kind: "file" }],
          integrationReferences: [
            {
              provider: "atlassian",
              kind: "jira",
              id: "RX-42",
              key: "RX-42",
              title: "Fix composer references",
            },
          ],
        });
      });

      expect(mockAddOptimisticUserMessageToConversationCache).toHaveBeenCalledWith(
        expect.anything(),
        "conv-review",
        "review this",
        {
          metadata: JSON.stringify({
            composer_folder_references: [
              {
                id: "folder-1",
                folderPath: "/work/brand-kit",
                displayName: "brand-kit",
              },
            ],
            composer_project_references: [
              { path: "src/main.ts", kind: "file" },
            ],
            composer_integration_references: [
              {
                provider: "atlassian",
                kind: "jira",
                id: "RX-42",
                key: "RX-42",
                title: "Fix composer references",
              },
            ],
          }),
        },
      );
    });

    it("review mode rolls back the optimistic user message when send fails", async () => {
      mockSendAgentMessage.mockRejectedValue(new Error("review send failed"));
      const { result } = setup({
        contextType: "review",
        contextId: "task-42",
        storeContextKey: "review:task-42",
        selectedTaskId: "task-42",
        activeConversationId: "conv-review",
      });

      await act(async () => {
        await result.current.handleSend("review this");
      });

      expect(mockRemoveOptimisticMessageFromConversationCache).toHaveBeenCalledWith(
        expect.anything(),
        "conv-review",
        "optimistic:conv-1:test"
      );
    });

    it("ideation first message triggers auto-naming", async () => {
      const { result } = setup({
        contextType: "ideation",
        contextId: "session-1",
        ideationSessionId: "session-1",
        messageCount: 0,
      });

      await act(async () => {
        await result.current.handleSend("build a todo app");
      });

      expect(mockSpawnSessionNamer).toHaveBeenCalledWith("session-1", "build a todo app");
    });

    it("ideation does not trigger auto-naming when messageCount > 0", async () => {
      const { result } = setup({
        contextType: "ideation",
        contextId: "session-1",
        ideationSessionId: "session-1",
        messageCount: 3,
      });

      await act(async () => {
        await result.current.handleSend("follow-up message");
      });

      expect(mockSpawnSessionNamer).not.toHaveBeenCalled();
    });

    it("ideation seeds waiting-for-capacity state immediately when queued as pending", async () => {
      const queuedAsPendingResult = {
        conversationId: "conv-pending",
        agentRunId: "run-pending",
        isNewConversation: true,
        wasQueued: true,
        queuedAsPending: true,
        queuedMessageId: undefined,
      };
      const { result, mutateAsync } = setup({
        contextType: "ideation",
        contextId: "session-1",
        storeContextKey: "ideation:session-1",
        ideationSessionId: "session-1",
        messageCount: 0,
      });
      mutateAsync.mockResolvedValue(queuedAsPendingResult);

      await act(async () => {
        await result.current.handleSend("queued first prompt");
      });

      expect(mockActions.setActiveConversation).toHaveBeenCalledWith(
        "ideation:session-1",
        "conv-pending",
      );
      expect(mockSetQueryData).toHaveBeenCalledWith(
        ["child-session-status", "session-1"],
        {
          session_id: "session-1",
          title: null,
          agent_state: { estimated_status: "idle" },
          recent_messages: [],
          pending_initial_prompt: "queued first prompt",
          lastEffectiveModel: null,
        },
      );
    });

    it("shows a toast when the backend rejects a send before agent spawn", async () => {
      const { result, mutateAsync } = setup({
        contextType: "project",
        contextId: "project-1",
        storeContextKey: "project:conv-1",
      });
      mutateAsync.mockRejectedValue(
        new Error(
          'Command /Users/example/.nvm/versions/node/v22.16.0/bin/codex ["--version"] exited with status 127: env: node: No such file or directory',
        ),
      );

      await act(async () => {
        await result.current.handleSend("start agent");
      });

      expect(mockActions.setAgentRunning).toHaveBeenCalledWith(
        "project:conv-1",
        false,
      );
      expect(mockToastError).toHaveBeenCalledWith("Failed to send message", {
        description: expect.stringContaining(
          "env: node: No such file or directory",
        ),
        duration: 10000,
      });
    });

    it("keeps the turn visible when the agent received it but it was not saved", async () => {
      const { result, mutateAsync } = setup({
        contextType: "project",
        contextId: "project-1",
        storeContextKey: "project:conv-1",
      });
      mutateAsync.mockRejectedValue(
        new Error(
          "[Message delivered but not saved: Repository error: chat message create failed]",
        ),
      );

      await act(async () => {
        await result.current.handleSend("keep me visible");
      });

      expect(mockToastError).not.toHaveBeenCalled();
      expect(mockToastWarning).toHaveBeenCalledWith("Message sent, but not saved", {
        description: expect.stringContaining("is replying"),
        duration: 10000,
      });
      // The agent is answering this turn — the spinner must not be cleared.
      expect(mockActions.setAgentRunning).not.toHaveBeenCalledWith(
        "project:conv-1",
        false,
      );
    });

    it("keeps the optimistic bubble for a delivered-but-unsaved direct send", async () => {
      mockSendAgentMessage.mockRejectedValue(
        new Error(
          "[Message delivered but not saved: Repository error: chat message create failed]",
        ),
      );
      const { result } = setup({
        contextType: "review",
        contextId: "task-42",
        storeContextKey: "review:task-42",
        selectedTaskId: "task-42",
        activeConversationId: "conv-review",
      });

      await act(async () => {
        await result.current.handleSend("review this");
      });

      expect(mockRemoveOptimisticMessageFromConversationCache).not.toHaveBeenCalled();
      expect(mockToastWarning).toHaveBeenCalled();
    });
  });

  // ── handleStopAgent ─────────────────────────────────────────────

  describe("handleStopAgent", () => {
    it("calls stopAgent API", async () => {
      const { result } = setup({
        contextType: "ideation",
        contextId: "session-1",
      });

      await act(async () => {
        await result.current.handleStopAgent();
      });

      expect(mockStopAgent).toHaveBeenCalledWith("ideation", "session-1");
      expect(mockRecoverTaskExecution).not.toHaveBeenCalled();
    });

    it("task_execution mode also calls recoverTaskExecution", async () => {
      const { result } = setup({
        contextType: "task_execution",
        contextId: "task-99",
        selectedTaskId: "task-99",
      });

      await act(async () => {
        await result.current.handleStopAgent();
      });

      expect(mockStopAgent).toHaveBeenCalledWith("task_execution", "task-99");
      expect(mockRecoverTaskExecution).toHaveBeenCalledWith("task-99");
    });
  });

  // ── handleDeleteQueuedMessage ───────────────────────────────────

  describe("handleDeleteQueuedMessage", () => {
    it("deletes from store and backend", async () => {
      const { result } = setup();

      await act(async () => {
        await result.current.handleDeleteQueuedMessage("msg-123");
      });

      expect(mockActions.deleteQueuedMessage).toHaveBeenCalledWith("task:task-1", "msg-123");
      expect(mockDeleteQueuedAgentMessage).toHaveBeenCalledWith("task", "task-1", "msg-123");
    });
  });

  // ── handleSendQueuedMessageNow ──────────────────────────────────

  describe("handleSendQueuedMessageNow", () => {
    it("deletes locally and asks backend to interrupt and send queued message", async () => {
      const { result } = setup({
        contextType: "project",
        contextId: "project-1",
        queueContextId: "conv-agent",
        storeContextKey: "project:conv-agent",
      });

      await act(async () => {
        await result.current.handleSendQueuedMessageNow("queued-1");
      });

      expect(mockActions.deleteQueuedMessage).toHaveBeenCalledWith(
        "project:conv-agent",
        "queued-1"
      );
      expect(mockSendQueuedAgentMessageNow).toHaveBeenCalledWith(
        "project",
        "conv-agent",
        "queued-1"
      );
    });

    it("queues replacement locally when backend cannot send immediately", async () => {
      mockSendQueuedAgentMessageNow.mockResolvedValue({
        conversationId: "conv-1",
        agentRunId: "run-1",
        isNewConversation: false,
        wasQueued: true,
        queuedAsPending: false,
        queuedMessageId: "queued-replacement",
      });
      const { result } = setup();

      await act(async () => {
        await result.current.handleSendQueuedMessageNow(
          "queued-1",
          "send when possible",
          ["att-1"]
        );
      });

      expect(mockActions.queueMessage).toHaveBeenCalledWith(
        "task:task-1",
        "send when possible",
        "queued-replacement",
        ["att-1"]
      );
    });

    it("does not re-queue a send-now turn the agent already received", async () => {
      mockSendQueuedAgentMessageNow.mockRejectedValue(
        new Error(
          "[Message delivered but not saved: Repository error: chat message create failed]",
        ),
      );
      const { result } = setup();

      await act(async () => {
        await result.current.handleSendQueuedMessageNow(
          "queued-1",
          "already delivered",
        );
      });

      expect(mockActions.queueMessage).not.toHaveBeenCalled();
      expect(mockActions.setAgentRunning).toHaveBeenCalledWith("task:task-1", true);
      expect(mockToastWarning).toHaveBeenCalled();
    });

    it("keeps the frozen selection on a queued send-now replacement", async () => {
      mockSendQueuedAgentMessageNow.mockResolvedValue({
        conversationId: "conv-1",
        agentRunId: "run-1",
        isNewConversation: false,
        wasQueued: true,
        queuedAsPending: false,
        queuedMessageId: "queued-replacement",
      });
      const { result } = setup();

      await act(async () => {
        await result.current.handleSendQueuedMessageNow(
          "queued-1",
          "send when possible",
          undefined,
          selectionSnapshot,
        );
      });

      expect(mockActions.queueMessage).toHaveBeenCalledWith(
        "task:task-1",
        "send when possible",
        "queued-replacement",
        undefined,
        selectionSnapshot,
      );
    });
  });

  // ── handleEditQueuedMessage ─────────────────────────────────────

  describe("handleEditQueuedMessage", () => {
    it("deletes old message and sends via sendAgentMessage", async () => {
      const { result } = setup();

      await act(async () => {
        await result.current.handleEditQueuedMessage("old-id", "updated content");
      });

      // Old message deleted from backend and store
      expect(mockDeleteQueuedAgentMessage).toHaveBeenCalledWith("task", "task-1", "old-id");
      expect(mockActions.deleteQueuedMessage).toHaveBeenCalledWith("task:task-1", "old-id");

      // Sends via sendAgentMessage (not queueAgentMessage)
      expect(mockSendAgentMessage).toHaveBeenCalledWith(
        "task",
        "task-1",
        "updated content",
        undefined,
        undefined
      );
    });

    it("queues locally when sendAgentMessage returns wasQueued=true", async () => {
      mockSendAgentMessage.mockResolvedValue({
        conversationId: "conv-1",
        agentRunId: "run-1",
        isNewConversation: false,
        wasQueued: true,
        queuedMessageId: "q-new-1",
      });

      const { result } = setup();

      await act(async () => {
        await result.current.handleEditQueuedMessage("old-id", "updated content");
      });

      expect(mockActions.queueMessage).toHaveBeenCalledWith("task:task-1", "updated content", "q-new-1");
    });

    it("keeps attachment IDs when editing a queued message", async () => {
      mockSendAgentMessage.mockResolvedValue({
        conversationId: "conv-1",
        agentRunId: "run-1",
        isNewConversation: false,
        wasQueued: true,
        queuedMessageId: "q-edited-with-file",
      });

      const { result } = setup();

      await act(async () => {
        await result.current.handleEditQueuedMessage(
          "old-id",
          "updated content",
          ["att-1"]
        );
      });

      expect(mockSendAgentMessage).toHaveBeenCalledWith(
        "task",
        "task-1",
        "updated content",
        ["att-1"],
        undefined
      );
      expect(mockActions.queueMessage).toHaveBeenCalledWith(
        "task:task-1",
        "updated content",
        "q-edited-with-file",
        ["att-1"]
      );
    });

    it("keeps the frozen selection when editing a queued message", async () => {
      mockSendAgentMessage.mockResolvedValue({
        conversationId: "conv-1",
        agentRunId: "run-1",
        isNewConversation: false,
        wasQueued: true,
        queuedMessageId: "q-edited-with-selection",
      });
      const { result } = setup();

      await act(async () => {
        await result.current.handleEditQueuedMessage(
          "old-id",
          "updated content",
          undefined,
          selectionSnapshot,
        );
      });

      expect(mockSendAgentMessage).toHaveBeenCalledWith(
        "task",
        "task-1",
        "updated content",
        undefined,
        { composerSelectionSnapshot: selectionSnapshot },
      );
      expect(mockActions.queueMessage).toHaveBeenCalledWith(
        "task:task-1",
        "updated content",
        "q-edited-with-selection",
        undefined,
        selectionSnapshot,
      );
    });

    it("sets and clears sending spinner", async () => {
      const { result } = setup();

      await act(async () => {
        await result.current.handleEditQueuedMessage("old-id", "updated content");
      });

      expect(mockActions.setSending).toHaveBeenCalledWith("task:task-1", true);
      expect(mockActions.setSending).toHaveBeenCalledWith("task:task-1", false);
    });

    it("uses queue context for delete and original context with send options for edited send", async () => {
      const mutateAsync = vi.fn().mockResolvedValue({
        conversationId: "conv-agent",
        agentRunId: "run-agent",
        isNewConversation: false,
        wasQueued: false,
        queuedAsPending: false,
      });

      const { result } = renderHook(() =>
        useChatActions({
          contextType: "project",
          contextId: "project-1",
          queueContextId: "conv-agent",
          storeContextKey: "project:conv-agent",
          selectedTaskId: undefined,
          ideationSessionId: undefined,
          sendMessage: { isPending: false, mutateAsync },
          sendOptions: {
            conversationId: "conv-agent",
            providerHarness: "codex",
            modelId: "gpt-5.4",
          },
        })
      );

      await act(async () => {
        await result.current.handleEditQueuedMessage("old-id", "updated content");
      });

      expect(mockDeleteQueuedAgentMessage).toHaveBeenCalledWith(
        "project",
        "conv-agent",
        "old-id"
      );
      expect(mockSendAgentMessage).toHaveBeenCalledWith(
        "project",
        "project-1",
        "updated content",
        undefined,
        {
          conversationId: "conv-agent",
          providerHarness: "codex",
          modelId: "gpt-5.4",
        }
      );
    });
  });

  // ── storeContextKey consistency (double-execution fix) ─────────

  describe("storeContextKey consistency", () => {
    it("task_execution context routes through sendMessage.mutateAsync", async () => {
      const { result, mutateAsync } = setup({
        contextType: "task_execution",
        contextId: "task-99",
        storeContextKey: "task_execution:task-99",
      });

      await act(async () => {
        await result.current.handleSend("do the work");
      });

      expect(mutateAsync).toHaveBeenCalledWith({ content: "do the work", attachmentIds: undefined });
    });

    it("merge context routes directly through merge send API", async () => {
      const { result, mutateAsync } = setup({
        contextType: "merge",
        contextId: "task-99",
        storeContextKey: "merge:task-99",
      });

      await act(async () => {
        await result.current.handleSend("merge it");
      });

      expect(mockSendAgentMessage).toHaveBeenCalledWith(
        "merge",
        "task-99",
        "merge it",
        undefined,
        undefined
      );
      expect(mutateAsync).not.toHaveBeenCalled();
    });

    it("error during task_execution send resets isAgentRunning with correct key", async () => {
      const { result, mutateAsync } = setup({
        contextType: "task_execution",
        contextId: "task-err",
        storeContextKey: "task_execution:task-err",
      });

      mutateAsync.mockRejectedValue(new Error("send failed"));

      await act(async () => {
        await result.current.handleSend("will fail");
      });

      // On error, agent running state is reset on the correct key
      expect(mockActions.setAgentRunning).toHaveBeenCalledWith("task_execution:task-err", false);
    });

    it("error during merge send resets isAgentRunning with merge key", async () => {
      const { result } = setup({
        contextType: "merge",
        contextId: "task-merge-err",
        storeContextKey: "merge:task-merge-err",
      });

      mockSendAgentMessage.mockRejectedValue(new Error("merge failed"));

      await act(async () => {
        await result.current.handleSend("will fail");
      });

      expect(mockActions.setAgentRunning).toHaveBeenCalledWith("merge:task-merge-err", false);
    });
  });

  // ── ideation regression ─────────────────────────────────────────

  describe("ideation regression", () => {
    it("ideation handleSend routes through sendMessage.mutateAsync", async () => {
      const { result, mutateAsync } = setup({
        contextType: "ideation",
        contextId: "session-1",
        storeContextKey: "session:session-1",
        ideationSessionId: "session-1",
        messageCount: 5,
      });

      await act(async () => {
        await result.current.handleSend("ideation message");
      });

      expect(mutateAsync).toHaveBeenCalledWith({ content: "ideation message", attachmentIds: undefined });
    });

  });

  // ── handleEditLastQueued ────────────────────────────────────────

  describe("handleEditLastQueued", () => {
    it("starts editing last queued message", () => {
      const { result } = setup();

      act(() => {
        result.current.handleEditLastQueued([
          { id: "q-1" },
          { id: "q-2" },
          { id: "q-3" },
        ]);
      });

      expect(mockActions.startEditingQueuedMessage).toHaveBeenCalledWith("task:task-1", "q-3");
    });

    it("does nothing when queue is empty", () => {
      const { result } = setup();

      act(() => {
        result.current.handleEditLastQueued([]);
      });

      expect(mockActions.startEditingQueuedMessage).not.toHaveBeenCalled();
    });
  });
});
