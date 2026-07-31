import { describe, it, expect, beforeEach } from "vitest";
import {
  useChatStore,
  selectMessagesForContext,
  selectMessageCount,
  selectQueuedMessages,
  selectAgentActivityLabel,
  selectIsAgentRunning,
  selectActiveConversationId,
  selectActiveAgentRunId,
  selectActiveAgentRunHarness,
  selectToolCallStartTimes,
  selectEffectiveModel,
  getContextKey,
} from "./chatStore";
import type { ChatMessage } from "@/types/ideation";
import type { ChatContext } from "@/types/chat";

// Helper to create test messages
const createTestMessage = (overrides: Partial<ChatMessage> = {}): ChatMessage => ({
  id: `msg-${Math.random().toString(36).slice(2)}`,
  sessionId: null,
  projectId: null,
  taskId: null,
  role: "user",
  content: "Test message",
  metadata: null,
  parentMessageId: null,
  createdAt: "2026-01-24T12:00:00Z",
  ...overrides,
});

// Helper to create test context
const createTestContext = (overrides: Partial<ChatContext> = {}): ChatContext => ({
  view: "kanban",
  projectId: "project-1",
  ...overrides,
});

describe("chatStore", () => {
  beforeEach(() => {
    // Reset store to initial state before each test
    useChatStore.setState({
      messages: {},
      context: null,
      isLoading: false,
      activeConversationIds: {},
      activeAgentRunIds: {},
      activeAgentRunHarnesses: {},
      queuedMessages: {},
      agentStatus: {},
      agentActivityLabels: {},
      isSending: {},
      lastAgentEventTimestamp: {},
      toolCallStartTimes: {},
      lastToolCallCompletionTimestamp: {},
      toolCallCompletionTimestamps: {},
      effectiveModel: {},
      composerDraftsByKey: {},
      delegateExpansionByConversation: {},
    });
  });

  describe("initial state", () => {
    it("has empty messages", () => {
      const state = useChatStore.getState();
      expect(Object.keys(state.messages)).toHaveLength(0);
    });

    it("has null context", () => {
      const state = useChatStore.getState();
      expect(state.context).toBeNull();
    });

    it("has isLoading false", () => {
      const state = useChatStore.getState();
      expect(state.isLoading).toBe(false);
    });

    it("has empty activeConversationIds", () => {
      const state = useChatStore.getState();
      expect(state.activeConversationIds).toEqual({});
    });

    it("has empty activeAgentRunIds", () => {
      const state = useChatStore.getState();
      expect(state.activeAgentRunIds).toEqual({});
      expect(state.activeAgentRunHarnesses).toEqual({});
    });

    it("has empty queuedMessages", () => {
      const state = useChatStore.getState();
      expect(Object.keys(state.queuedMessages)).toHaveLength(0);
    });

    it("has empty agentStatus", () => {
      const state = useChatStore.getState();
      expect(Object.keys(state.agentStatus)).toHaveLength(0);
    });
  });

  describe("setContext", () => {
    it("sets context", () => {
      const context = createTestContext({ view: "ideation", ideationSessionId: "session-1" });

      useChatStore.getState().setContext(context);

      const state = useChatStore.getState();
      expect(state.context).toEqual(context);
    });

    it("replaces previous context", () => {
      const context1 = createTestContext({ view: "kanban" });
      const context2 = createTestContext({ view: "ideation", ideationSessionId: "session-1" });

      useChatStore.getState().setContext(context1);
      useChatStore.getState().setContext(context2);

      const state = useChatStore.getState();
      expect(state.context?.view).toBe("ideation");
    });

    it("sets context to null", () => {
      useChatStore.setState({ context: createTestContext() });

      useChatStore.getState().setContext(null);

      const state = useChatStore.getState();
      expect(state.context).toBeNull();
    });
  });

  describe("composer drafts", () => {
    it("stores draft content and removes the draft when content and attachments are empty", () => {
      const store = useChatStore.getState();

      store.setComposerDraftContent("conversation:one", "unsent message");

      expect(useChatStore.getState().composerDraftsByKey["conversation:one"]).toMatchObject({
        content: "unsent message",
        attachments: [],
      });

      useChatStore.getState().setComposerDraftContent("conversation:one", "");

      expect(useChatStore.getState().composerDraftsByKey["conversation:one"]).toBeUndefined();
    });

    it("keeps attachment drafts scoped by composer key", () => {
      const file = new File(["draft"], "draft.txt", { type: "text/plain" });
      const store = useChatStore.getState();

      store.setComposerDraftContent("conversation:one", "first");
      store.setComposerDraftAttachments("conversation:one", [
        {
          id: "attachment-1",
          conversationId: "one",
          fileName: "draft.txt",
          filePath: "/tmp/draft.txt",
          fileSize: 5,
          mimeType: "text/plain",
          createdAt: "2026-06-29T00:00:00Z",
          file,
        },
      ]);
      store.setComposerDraftContent("conversation:two", "second");

      expect(
        useChatStore.getState().composerDraftsByKey["conversation:one"]?.attachments,
      ).toEqual([
        expect.objectContaining({
          id: "attachment-1",
          fileName: "draft.txt",
          file,
        }),
      ]);
      expect(
        useChatStore.getState().composerDraftsByKey["conversation:two"]?.attachments,
      ).toEqual([]);

      useChatStore.getState().clearComposerDraft("conversation:one");

      expect(useChatStore.getState().composerDraftsByKey["conversation:one"]).toBeUndefined();
      expect(useChatStore.getState().composerDraftsByKey["conversation:two"]).toMatchObject({
        content: "second",
      });
    });

    it("keeps folder-reference drafts scoped by composer key and preserves them alongside content", () => {
      const store = useChatStore.getState();

      store.setComposerDraftFolders("conversation:one", [
        { id: "folder-1", folderPath: "/work/design-notes", displayName: "design-notes" },
      ]);

      expect(
        useChatStore.getState().composerDraftsByKey["conversation:one"],
      ).toMatchObject({
        content: "",
        attachments: [],
        folders: [
          { id: "folder-1", folderPath: "/work/design-notes", displayName: "design-notes" },
        ],
      });

      useChatStore.getState().setComposerDraftContent("conversation:one", "add design-notes");
      expect(
        useChatStore.getState().composerDraftsByKey["conversation:one"]?.folders,
      ).toEqual([{ id: "folder-1", folderPath: "/work/design-notes", displayName: "design-notes" }]);

      useChatStore.getState().setComposerDraftFolders("conversation:one", []);
      // Content is still set, so the draft entry is not deleted — only folders clear.
      expect(
        useChatStore.getState().composerDraftsByKey["conversation:one"],
      ).toMatchObject({
        content: "add design-notes",
        folders: [],
      });

      useChatStore.getState().setComposerDraftContent("conversation:one", "");
      // Content, attachments, and folders are now all empty — the draft is removed.
      expect(useChatStore.getState().composerDraftsByKey["conversation:one"]).toBeUndefined();
    });
  });

  describe("addMessage", () => {
    it("adds message to context", () => {
      const message = createTestMessage({ id: "msg-1", content: "Hello" });

      useChatStore.getState().addMessage("session:session-1", message);

      const state = useChatStore.getState();
      expect(state.messages["session:session-1"]).toHaveLength(1);
      expect(state.messages["session:session-1"]?.[0].content).toBe("Hello");
    });

    it("appends to existing messages", () => {
      const msg1 = createTestMessage({ id: "msg-1", content: "First" });
      const msg2 = createTestMessage({ id: "msg-2", content: "Second" });

      useChatStore.getState().addMessage("session:session-1", msg1);
      useChatStore.getState().addMessage("session:session-1", msg2);

      const state = useChatStore.getState();
      expect(state.messages["session:session-1"]).toHaveLength(2);
      expect(state.messages["session:session-1"]?.[0].content).toBe("First");
      expect(state.messages["session:session-1"]?.[1].content).toBe("Second");
    });

    it("keeps messages separate by context key", () => {
      const msg1 = createTestMessage({ id: "msg-1" });
      const msg2 = createTestMessage({ id: "msg-2" });

      useChatStore.getState().addMessage("session:session-1", msg1);
      useChatStore.getState().addMessage("session:session-2", msg2);

      const state = useChatStore.getState();
      expect(state.messages["session:session-1"]).toHaveLength(1);
      expect(state.messages["session:session-2"]).toHaveLength(1);
    });
  });

  describe("setMessages", () => {
    it("sets messages for context", () => {
      const messages = [
        createTestMessage({ id: "msg-1", content: "First" }),
        createTestMessage({ id: "msg-2", content: "Second" }),
      ];

      useChatStore.getState().setMessages("session:session-1", messages);

      const state = useChatStore.getState();
      expect(state.messages["session:session-1"]).toHaveLength(2);
    });

    it("replaces existing messages", () => {
      useChatStore.setState({
        messages: {
          "session:session-1": [createTestMessage({ id: "old", content: "Old" })],
        },
      });

      const newMessages = [createTestMessage({ id: "new", content: "New" })];
      useChatStore.getState().setMessages("session:session-1", newMessages);

      const state = useChatStore.getState();
      expect(state.messages["session:session-1"]).toHaveLength(1);
      expect(state.messages["session:session-1"]?.[0].content).toBe("New");
    });

    it("preserves other context messages", () => {
      useChatStore.setState({
        messages: {
          "session:session-1": [createTestMessage({ id: "s1-msg" })],
          "session:session-2": [createTestMessage({ id: "s2-msg" })],
        },
      });

      const newMessages = [createTestMessage({ id: "new" })];
      useChatStore.getState().setMessages("session:session-1", newMessages);

      const state = useChatStore.getState();
      expect(state.messages["session:session-1"]).toHaveLength(1);
      expect(state.messages["session:session-2"]).toHaveLength(1);
    });

    it("handles empty array", () => {
      useChatStore.getState().setMessages("session:session-1", []);

      const state = useChatStore.getState();
      expect(state.messages["session:session-1"]).toHaveLength(0);
    });
  });

  describe("selectToolCallStartTimes", () => {
    it("returns a stable empty record when a context has no tool call timestamps", () => {
      const selector = selectToolCallStartTimes("task_execution:missing");

      const first = selector(useChatStore.getState());
      const second = selector(useChatStore.getState());

      expect(first).toBe(second);
      expect(first).toEqual({});
    });

    it("returns stored timestamps for the requested context", () => {
      useChatStore.getState().setToolCallStartTime("task_execution:task-1", "tool-1", 123);
      useChatStore.getState().setToolCallStartTime("task_execution:task-2", "tool-2", 456);

      const selector = selectToolCallStartTimes("task_execution:task-1");

      expect(selector(useChatStore.getState())).toEqual({ "tool-1": 123 });
    });
  });

  describe("clearMessages", () => {
    it("clears messages for context", () => {
      useChatStore.setState({
        messages: {
          "session:session-1": [createTestMessage()],
        },
      });

      useChatStore.getState().clearMessages("session:session-1");

      const state = useChatStore.getState();
      expect(state.messages["session:session-1"]).toBeUndefined();
    });

    it("preserves other context messages", () => {
      useChatStore.setState({
        messages: {
          "session:session-1": [createTestMessage()],
          "session:session-2": [createTestMessage()],
        },
      });

      useChatStore.getState().clearMessages("session:session-1");

      const state = useChatStore.getState();
      expect(state.messages["session:session-1"]).toBeUndefined();
      expect(state.messages["session:session-2"]).toHaveLength(1);
    });

    it("does nothing if context not found", () => {
      useChatStore.getState().clearMessages("nonexistent");

      const state = useChatStore.getState();
      expect(Object.keys(state.messages)).toHaveLength(0);
    });
  });

  describe("setLoading", () => {
    it("sets isLoading to true", () => {
      useChatStore.getState().setLoading(true);

      const state = useChatStore.getState();
      expect(state.isLoading).toBe(true);
    });

    it("sets isLoading to false", () => {
      useChatStore.setState({ isLoading: true });

      useChatStore.getState().setLoading(false);

      const state = useChatStore.getState();
      expect(state.isLoading).toBe(false);
    });
  });

  describe("setActiveConversation", () => {
    const storeKeyA = "session:session-a";
    const storeKeyB = "task_execution:task-b";

    it("sets activeConversationIds for a storeKey", () => {
      useChatStore.getState().setActiveConversation(storeKeyA, "conv-123");

      const state = useChatStore.getState();
      expect(state.activeConversationIds[storeKeyA]).toBe("conv-123");
    });

    it("replaces previous conversation ID for same storeKey", () => {
      useChatStore.setState({ activeConversationIds: { [storeKeyA]: "conv-old" } });

      useChatStore.getState().setActiveConversation(storeKeyA, "conv-new");

      const state = useChatStore.getState();
      expect(state.activeConversationIds[storeKeyA]).toBe("conv-new");
    });

    it("sets to null for a storeKey", () => {
      useChatStore.setState({ activeConversationIds: { [storeKeyA]: "conv-123" } });

      useChatStore.getState().setActiveConversation(storeKeyA, null);

      const state = useChatStore.getState();
      expect(state.activeConversationIds[storeKeyA]).toBeNull();
    });

    it("is no-op when setting same conversation ID for same storeKey", () => {
      useChatStore.getState().setActiveConversation(storeKeyA, "conv-123");

      // Get state reference after first set
      const stateBefore = useChatStore.getState();

      // Call with same value — should be a no-op
      useChatStore.getState().setActiveConversation(storeKeyA, "conv-123");

      const stateAfter = useChatStore.getState();
      // State object reference should be unchanged (no new immer draft)
      expect(stateAfter).toBe(stateBefore);
    });

    it("isolates storeKey A from storeKey B", () => {
      useChatStore.getState().setActiveConversation(storeKeyA, "conv-a");
      useChatStore.getState().setActiveConversation(storeKeyB, "conv-b");

      const state = useChatStore.getState();
      expect(state.activeConversationIds[storeKeyA]).toBe("conv-a");
      expect(state.activeConversationIds[storeKeyB]).toBe("conv-b");
    });

    it("setting storeKey B does not affect storeKey A", () => {
      useChatStore.getState().setActiveConversation(storeKeyA, "conv-a");
      const stateBefore = useChatStore.getState();

      useChatStore.getState().setActiveConversation(storeKeyB, "conv-b");

      const stateAfter = useChatStore.getState();
      expect(stateAfter.activeConversationIds[storeKeyA]).toBe("conv-a");
      // State for storeKeyA did not change
      expect(stateAfter.activeConversationIds[storeKeyA]).toBe(
        stateBefore.activeConversationIds[storeKeyA]
      );
    });

    it("returns undefined for unknown storeKey", () => {
      const state = useChatStore.getState();
      expect(state.activeConversationIds["unknown:key"]).toBeUndefined();
    });

    it("preserves delegate expansion within a conversation and prunes it on switch", () => {
      useChatStore.getState().setActiveConversation(storeKeyA, "conv-old");
      useChatStore.getState().setDelegateExpanded("conv-old", "call-delegate", true);

      expect(
        useChatStore.getState().delegateExpansionByConversation["conv-old"]?.[
          "call-delegate"
        ],
      ).toBe(true);

      useChatStore.getState().setActiveConversation(storeKeyA, "conv-new");

      expect(
        useChatStore.getState().delegateExpansionByConversation["conv-old"],
      ).toBeUndefined();
    });
  });

  describe("active agent run ids", () => {
    const storeKey = "project:conversation-1";

    it("sets activeAgentRunIds for a storeKey", () => {
      useChatStore.getState().setActiveAgentRun(storeKey, "run-123", "claude");

      const state = useChatStore.getState();
      expect(state.activeAgentRunIds[storeKey]).toBe("run-123");
      expect(state.activeAgentRunHarnesses[storeKey]).toBe("claude");
    });

    it("replaces previous active run id for same storeKey", () => {
      useChatStore.getState().setActiveAgentRun(storeKey, "run-old", "claude");
      useChatStore.getState().setActiveAgentRun(storeKey, "run-new", "codex");

      const state = useChatStore.getState();
      expect(state.activeAgentRunIds[storeKey]).toBe("run-new");
      expect(state.activeAgentRunHarnesses[storeKey]).toBe("codex");
    });

    it("preserves the original timing anchor when the same run is re-emitted", () => {
      useChatStore.getState().setActiveAgentRun(storeKey, "run-123", "claude", {
        startedAt: 100, agentName: "reviewer", launchRole: "workspace_reviewer",
      });
      useChatStore.getState().setActiveAgentRun(storeKey, "run-123", "claude", {
        startedAt: 200, agentName: "reviewer-v2", launchRole: "workspace_reviewer",
      });

      expect(useChatStore.getState().activeAgentRunMeta[storeKey]).toEqual({
        startedAt: 100, agentName: "reviewer-v2", launchRole: "workspace_reviewer",
      });
    });

    it("resets metadata for a new run and clears it with the active run", () => {
      useChatStore.getState().setActiveAgentRun(storeKey, "run-old", "claude", {
        startedAt: 100, agentName: "old", launchRole: "workspace_reviewer",
      });
      useChatStore.getState().setActiveAgentRun(storeKey, "run-new", "codex", {
        startedAt: 200, agentName: "new", launchRole: "pr_fixer",
      });
      expect(useChatStore.getState().activeAgentRunMeta[storeKey]).toEqual({
        startedAt: 200, agentName: "new", launchRole: "pr_fixer",
      });
      useChatStore.getState().clearActiveAgentRun(storeKey, "run-new");
      expect(useChatStore.getState().activeAgentRunMeta[storeKey]).toBeUndefined();
    });

    it("replaces a known harness with an unknown pair without leaking the old harness", () => {
      useChatStore.getState().setActiveAgentRun(storeKey, "run-old", "claude");
      useChatStore.getState().setActiveAgentRun(storeKey, "run-new");

      const state = useChatStore.getState();
      expect(state.activeAgentRunIds[storeKey]).toBe("run-new");
      expect(state.activeAgentRunHarnesses[storeKey]).toBeNull();
    });

    it("clears active run id only when the expected run id matches", () => {
      useChatStore.getState().setActiveAgentRun(storeKey, "run-new", "codex");
      useChatStore.getState().clearActiveAgentRun(storeKey, "run-old");

      expect(useChatStore.getState().activeAgentRunIds[storeKey]).toBe("run-new");
      expect(useChatStore.getState().activeAgentRunHarnesses[storeKey]).toBe("codex");

      useChatStore.getState().clearActiveAgentRun(storeKey, "run-new");

      expect(useChatStore.getState().activeAgentRunIds[storeKey]).toBeUndefined();
      expect(useChatStore.getState().activeAgentRunHarnesses[storeKey]).toBeUndefined();
    });

    it("keeps a newer unknown pair and another context intact when an old id clears", () => {
      const otherStoreKey = "task:task-2";
      useChatStore.getState().setActiveAgentRun(storeKey, "run-new");
      useChatStore.getState().setActiveAgentRun(otherStoreKey, "run-other", "claude");

      useChatStore.getState().clearActiveAgentRun(storeKey, "run-old");

      const state = useChatStore.getState();
      expect(state.activeAgentRunIds[storeKey]).toBe("run-new");
      expect(state.activeAgentRunHarnesses[storeKey]).toBeNull();
      expect(state.activeAgentRunIds[otherStoreKey]).toBe("run-other");
      expect(state.activeAgentRunHarnesses[otherStoreKey]).toBe("claude");
    });

    it("clears active run id when agent status is set to idle", () => {
      useChatStore.getState().setActiveAgentRun(storeKey, "run-123", "claude");
      useChatStore.getState().setAgentStatus(storeKey, "generating");
      useChatStore.getState().setAgentActivityLabel(storeKey, "Agent working");
      useChatStore.getState().setAgentStatus(storeKey, "idle");

      const state = useChatStore.getState();
      expect(state.agentStatus[storeKey]).toBeUndefined();
      expect(state.activeAgentRunIds[storeKey]).toBeUndefined();
      expect(state.activeAgentRunHarnesses[storeKey]).toBeUndefined();
      expect(state.agentActivityLabels[storeKey]).toBeUndefined();
    });

    it("keeps run pairs isolated by store key and clears both during task cleanup", () => {
      useChatStore.getState().setActiveAgentRun("task:task-1", "run-task", "claude");
      useChatStore.getState().setActiveAgentRun("project:conversation-1", "run-project", "codex");

      useChatStore.getState().clearAgentRunningForTask("task-1");

      expect(useChatStore.getState().activeAgentRunIds["task:task-1"]).toBeUndefined();
      expect(useChatStore.getState().activeAgentRunHarnesses["task:task-1"]).toBeUndefined();
      expect(useChatStore.getState().activeAgentRunIds["project:conversation-1"]).toBe("run-project");
      expect(useChatStore.getState().activeAgentRunHarnesses["project:conversation-1"]).toBe("codex");
    });

    it("clears an activity label when idle is set before a run id exists", () => {
      useChatStore.getState().setAgentActivityLabel(storeKey, "Starting agent");

      useChatStore.getState().setAgentStatus(storeKey, "idle");

      expect(useChatStore.getState().agentActivityLabels[storeKey]).toBeUndefined();
    });

    it("selectors return the paired active run identity and harness", () => {
      useChatStore.getState().setActiveAgentRun(storeKey, "run-123", "claude");

      expect(selectActiveAgentRunId(storeKey)(useChatStore.getState())).toBe("run-123");
      expect(selectActiveAgentRunHarness(storeKey)(useChatStore.getState())).toBe("claude");
    });
  });

  describe("setAgentRunning", () => {
    const contextKey = "task:test-task";

    it("sets agentStatus to 'generating' for context", () => {
      useChatStore.getState().setAgentRunning(contextKey, true);

      const state = useChatStore.getState();
      expect(state.agentStatus[contextKey]).toBe("generating");
    });

    it("removes context when set to false", () => {
      useChatStore.getState().setAgentRunning(contextKey, true);
      useChatStore.getState().setAgentActivityLabel(contextKey, "Agent working");
      useChatStore.getState().setAgentRunning(contextKey, false);

      const state = useChatStore.getState();
      expect(state.agentStatus[contextKey]).toBeUndefined();
      expect(state.agentActivityLabels[contextKey]).toBeUndefined();
    });

    it("clears an activity label when stopped before status exists", () => {
      useChatStore.getState().setAgentActivityLabel(contextKey, "Starting agent");

      useChatStore.getState().setAgentRunning(contextKey, false);

      expect(useChatStore.getState().agentActivityLabels[contextKey]).toBeUndefined();
    });

    it("clears an activity label when the agent is waiting for input", () => {
      useChatStore.getState().setAgentStatus(contextKey, "generating");
      useChatStore.getState().setAgentActivityLabel(contextKey, "Agent working");

      useChatStore.getState().setAgentStatus(contextKey, "waiting_for_input");

      expect(useChatStore.getState().agentStatus[contextKey]).toBe("waiting_for_input");
      expect(useChatStore.getState().agentActivityLabels[contextKey]).toBeUndefined();
    });

    it("keeps separate states by context key", () => {
      const taskKey = "task:task-123";
      const execKey = "task_execution:task-123";

      useChatStore.getState().setAgentRunning(taskKey, true);
      useChatStore.getState().setAgentRunning(execKey, true);
      useChatStore.getState().setAgentRunning(taskKey, false);

      const state = useChatStore.getState();
      expect(state.agentStatus[taskKey]).toBeUndefined();
      expect(state.agentStatus[execKey]).toBe("generating");
    });

    it("clearAgentRunningForTask removes active run ids for task-scoped keys", () => {
      useChatStore.getState().setActiveAgentRun("task:task-123", "run-task");
      useChatStore.getState().setActiveAgentRun("task_execution:task-123", "run-worker");
      useChatStore.getState().setActiveAgentRun("project:conversation-1", "run-project");

      useChatStore.getState().clearAgentRunningForTask("task-123");

      const state = useChatStore.getState();
      expect(state.activeAgentRunIds["task:task-123"]).toBeUndefined();
      expect(state.activeAgentRunIds["task_execution:task-123"]).toBeUndefined();
      expect(state.activeAgentRunIds["project:conversation-1"]).toBe("run-project");
    });

    it("is no-op when setting false and key is already absent", () => {
      // Get state reference before the call
      const stateBefore = useChatStore.getState();
      const agentStatusBefore = stateBefore.agentStatus;

      // Call setAgentRunning(false) when key doesn't exist — should be a no-op
      useChatStore.getState().setAgentRunning("nonexistent:key", false);

      const stateAfter = useChatStore.getState();
      // State object reference should be unchanged (no new immer draft)
      expect(stateAfter.agentStatus).toBe(agentStatusBefore);
    });

    it("is no-op when setting true and key is already generating", () => {
      useChatStore.getState().setAgentRunning(contextKey, true);

      // Get state reference after first set
      const stateBefore = useChatStore.getState();
      const agentStatusBefore = stateBefore.agentStatus;

      // Call setAgentRunning(true) again — should be a no-op
      useChatStore.getState().setAgentRunning(contextKey, true);

      const stateAfter = useChatStore.getState();
      // State object reference should be unchanged (no new immer draft)
      expect(stateAfter.agentStatus).toBe(agentStatusBefore);
    });
  });

  describe("setSending", () => {
    const contextKey = "task:test-task";

    it("sets isSending to true for context", () => {
      useChatStore.getState().setSending(contextKey, true);

      const state = useChatStore.getState();
      expect(state.isSending[contextKey]).toBe(true);
    });

    it("removes context when set to false", () => {
      useChatStore.getState().setSending(contextKey, true);
      useChatStore.getState().setSending(contextKey, false);

      const state = useChatStore.getState();
      expect(state.isSending[contextKey]).toBeUndefined();
    });

    it("is no-op when setting true and key is already true", () => {
      useChatStore.getState().setSending(contextKey, true);

      const stateBefore = useChatStore.getState();

      useChatStore.getState().setSending(contextKey, true);

      const stateAfter = useChatStore.getState();
      expect(stateAfter).toBe(stateBefore);
    });

    it("is no-op when setting false and key is already absent", () => {
      const stateBefore = useChatStore.getState();

      useChatStore.getState().setSending("nonexistent:key", false);

      const stateAfter = useChatStore.getState();
      expect(stateAfter).toBe(stateBefore);
    });
  });

  describe("queueMessage", () => {
    const contextKey = "task:test-task";

    it("adds message to queue for context", () => {
      useChatStore.getState().queueMessage(contextKey, "Hello");

      const state = useChatStore.getState();
      expect(state.queuedMessages[contextKey]).toHaveLength(1);
      expect(state.queuedMessages[contextKey]?.[0].content).toBe("Hello");
    });

    it("generates unique ID for queued message", () => {
      useChatStore.getState().queueMessage(contextKey, "First");
      useChatStore.getState().queueMessage(contextKey, "Second");

      const state = useChatStore.getState();
      expect(state.queuedMessages[contextKey]?.[0].id).not.toBe(state.queuedMessages[contextKey]?.[1].id);
    });

    it("sets createdAt timestamp", () => {
      useChatStore.getState().queueMessage(contextKey, "Test");

      const state = useChatStore.getState();
      expect(state.queuedMessages[contextKey]?.[0].createdAt).toBeDefined();
      expect(new Date(state.queuedMessages[contextKey]?.[0].createdAt || "").getTime()).toBeLessThanOrEqual(
        Date.now()
      );
    });

    it("sets isEditing to false by default", () => {
      useChatStore.getState().queueMessage(contextKey, "Test");

      const state = useChatStore.getState();
      expect(state.queuedMessages[contextKey]?.[0].isEditing).toBe(false);
    });

    it("stores attachment IDs for queued messages", () => {
      useChatStore
        .getState()
        .queueMessage(contextKey, "Test", "msg-with-attachments", ["att-1", "att-2"]);

      const state = useChatStore.getState();
      expect(state.queuedMessages[contextKey]?.[0].attachmentIds).toEqual([
        "att-1",
        "att-2",
      ]);
    });

    it("stores and merges an immutable composer selection snapshot", () => {
      const snapshot = {
        sourceType: "ticket" as const,
        sourceKind: "clickup" as const,
        sourceId: "task-1",
        provider: "clickup" as const,
        startLine: 4,
        endLine: 5,
        content: "first\nsecond",
      };

      useChatStore
        .getState()
        .queueMessage(contextKey, "Test", "msg-with-selection");
      useChatStore
        .getState()
        .queueMessage(
          contextKey,
          "Test",
          "msg-with-selection",
          undefined,
          snapshot,
        );

      expect(useChatStore.getState().queuedMessages[contextKey]).toHaveLength(1);
      expect(
        useChatStore.getState().queuedMessages[contextKey]?.[0]
          .composerSelectionSnapshot,
      ).toEqual(snapshot);
    });

    it("merges attachment IDs from duplicate backend queue events", () => {
      useChatStore.getState().queueMessage(contextKey, "Test", "msg-with-attachments");
      useChatStore
        .getState()
        .queueMessage(contextKey, "Test", "msg-with-attachments", ["att-1"]);

      const state = useChatStore.getState();
      expect(state.queuedMessages[contextKey]).toHaveLength(1);
      expect(state.queuedMessages[contextKey]?.[0].attachmentIds).toEqual(["att-1"]);
    });

    it("appends to existing queue", () => {
      useChatStore.getState().queueMessage(contextKey, "First");
      useChatStore.getState().queueMessage(contextKey, "Second");
      useChatStore.getState().queueMessage(contextKey, "Third");

      const state = useChatStore.getState();
      expect(state.queuedMessages[contextKey]).toHaveLength(3);
      expect(state.queuedMessages[contextKey]?.[0].content).toBe("First");
      expect(state.queuedMessages[contextKey]?.[1].content).toBe("Second");
      expect(state.queuedMessages[contextKey]?.[2].content).toBe("Third");
    });

    it("keeps queues separate by context key", () => {
      const taskKey = "task:task-123";
      const execKey = "task_execution:task-123";

      useChatStore.getState().queueMessage(taskKey, "Task message");
      useChatStore.getState().queueMessage(execKey, "Execution message");

      const state = useChatStore.getState();
      expect(state.queuedMessages[taskKey]).toHaveLength(1);
      expect(state.queuedMessages[execKey]).toHaveLength(1);
      expect(state.queuedMessages[taskKey]?.[0].content).toBe("Task message");
      expect(state.queuedMessages[execKey]?.[0].content).toBe("Execution message");
    });

    it("ignores duplicate message ID (idempotent for same non-null ID)", () => {
      const messageId = "backend-queued-msg-123";

      useChatStore.getState().queueMessage(contextKey, "Hello", messageId);
      useChatStore.getState().queueMessage(contextKey, "Hello", messageId);

      const state = useChatStore.getState();
      expect(state.queuedMessages[contextKey]).toHaveLength(1);
    });

    it("allows two messages with different IDs", () => {
      useChatStore.getState().queueMessage(contextKey, "First", "id-001");
      useChatStore.getState().queueMessage(contextKey, "Second", "id-002");

      const state = useChatStore.getState();
      expect(state.queuedMessages[contextKey]).toHaveLength(2);
    });

    it("allows messages without explicit ID even when an ID message exists", () => {
      useChatStore.getState().queueMessage(contextKey, "With ID", "id-001");
      useChatStore.getState().queueMessage(contextKey, "Without ID");

      const state = useChatStore.getState();
      expect(state.queuedMessages[contextKey]).toHaveLength(2);
    });
  });

  describe("setQueuedMessages", () => {
    const contextKey = "task:test-task";
    const otherContextKey = "task:other-task";

    it("replaces a context queue with backend messages", () => {
      useChatStore.getState().queueMessage(contextKey, "Stale", "stale-id");

      useChatStore.getState().setQueuedMessages(contextKey, [
        {
          id: "backend-1",
          content: "Backend first",
          createdAt: "2026-06-19T10:00:00Z",
          isEditing: false,
          attachmentIds: ["att-1"],
        },
        {
          id: "backend-2",
          content: "Backend second",
          createdAt: "2026-06-19T10:01:00Z",
          isEditing: false,
          attachmentIds: [],
        },
      ]);

      const state = useChatStore.getState();
      expect(state.queuedMessages[contextKey]).toEqual([
        {
          id: "backend-1",
          content: "Backend first",
          createdAt: "2026-06-19T10:00:00Z",
          isEditing: false,
          attachmentIds: ["att-1"],
          source: "backend",
        },
        {
          id: "backend-2",
          content: "Backend second",
          createdAt: "2026-06-19T10:01:00Z",
          isEditing: false,
          attachmentIds: [],
          source: "backend",
        },
      ]);
    });

    it("clears the context queue when backend has no messages", () => {
      useChatStore.getState().queueMessage(contextKey, "Queued", "queued-id");

      useChatStore.getState().setQueuedMessages(contextKey, []);

      expect(useChatStore.getState().queuedMessages[contextKey]).toBeUndefined();
    });

    it("preserves local editing state for matching backend messages", () => {
      useChatStore.getState().queueMessage(contextKey, "Original", "queued-id");
      useChatStore.getState().startEditingQueuedMessage(contextKey, "queued-id");

      useChatStore.getState().setQueuedMessages(contextKey, [
        {
          id: "queued-id",
          content: "Original",
          createdAt: "2026-06-19T10:00:00Z",
          isEditing: false,
          attachmentIds: [],
        },
      ]);

      expect(useChatStore.getState().queuedMessages[contextKey]?.[0].isEditing).toBe(true);
    });

    it("keeps the same queue reference when hydrated messages are unchanged", () => {
      useChatStore.getState().setQueuedMessages(contextKey, [
        {
          id: "queued-id",
          content: "Original",
          createdAt: "2026-06-19T10:00:00Z",
          isEditing: false,
          attachmentIds: ["att-1"],
        },
      ]);
      const before = useChatStore.getState().queuedMessages[contextKey];

      useChatStore.getState().setQueuedMessages(contextKey, [
        {
          id: "queued-id",
          content: "Original",
          createdAt: "2026-06-19T10:00:00Z",
          isEditing: false,
          attachmentIds: ["att-1"],
        },
      ]);

      expect(useChatStore.getState().queuedMessages[contextKey]).toBe(before);
    });

    it("does not change other context queues", () => {
      useChatStore.getState().queueMessage(otherContextKey, "Other", "other-id");

      useChatStore.getState().setQueuedMessages(contextKey, [
        {
          id: "backend-id",
          content: "Backend",
          createdAt: "2026-06-19T10:00:00Z",
          isEditing: false,
          attachmentIds: [],
        },
      ]);

      expect(useChatStore.getState().queuedMessages[otherContextKey]?.[0].content).toBe("Other");
    });
  });

  describe("editQueuedMessage", () => {
    const contextKey = "task:test-task";

    it("updates message content", () => {
      useChatStore.getState().queueMessage(contextKey, "Original");
      const messageId = useChatStore.getState().queuedMessages[contextKey]?.[0].id || "";

      useChatStore.getState().editQueuedMessage(contextKey, messageId, "Updated");

      const state = useChatStore.getState();
      expect(state.queuedMessages[contextKey]?.[0].content).toBe("Updated");
    });

    it("preserves attachment IDs when editing content", () => {
      useChatStore
        .getState()
        .queueMessage(contextKey, "Original", "msg-with-attachments", ["att-1"]);

      useChatStore
        .getState()
        .editQueuedMessage(contextKey, "msg-with-attachments", "Updated");

      const state = useChatStore.getState();
      expect(state.queuedMessages[contextKey]?.[0].content).toBe("Updated");
      expect(state.queuedMessages[contextKey]?.[0].attachmentIds).toEqual(["att-1"]);
    });

    it("sets isEditing to false after edit", () => {
      useChatStore.getState().queueMessage(contextKey, "Test");
      const messageId = useChatStore.getState().queuedMessages[contextKey]?.[0].id || "";
      useChatStore.setState({
        queuedMessages: { [contextKey]: [{ ...useChatStore.getState().queuedMessages[contextKey]![0], isEditing: true }] },
      });

      useChatStore.getState().editQueuedMessage(contextKey, messageId, "Updated");

      const state = useChatStore.getState();
      expect(state.queuedMessages[contextKey]?.[0].isEditing).toBe(false);
    });

    it("does nothing if message not found", () => {
      useChatStore.getState().queueMessage(contextKey, "Test");

      useChatStore.getState().editQueuedMessage(contextKey, "nonexistent-id", "Updated");

      const state = useChatStore.getState();
      expect(state.queuedMessages[contextKey]?.[0].content).toBe("Test");
    });

    it("only updates specified message", () => {
      useChatStore.getState().queueMessage(contextKey, "First");
      useChatStore.getState().queueMessage(contextKey, "Second");
      useChatStore.getState().queueMessage(contextKey, "Third");
      const secondId = useChatStore.getState().queuedMessages[contextKey]?.[1].id || "";

      useChatStore.getState().editQueuedMessage(contextKey, secondId, "Updated Second");

      const state = useChatStore.getState();
      expect(state.queuedMessages[contextKey]?.[0].content).toBe("First");
      expect(state.queuedMessages[contextKey]?.[1].content).toBe("Updated Second");
      expect(state.queuedMessages[contextKey]?.[2].content).toBe("Third");
    });
  });

  describe("deleteQueuedMessage", () => {
    const contextKey = "task:test-task";

    it("removes message from queue", () => {
      useChatStore.getState().queueMessage(contextKey, "Test");
      const messageId = useChatStore.getState().queuedMessages[contextKey]?.[0].id || "";

      useChatStore.getState().deleteQueuedMessage(contextKey, messageId);

      const state = useChatStore.getState();
      expect(state.queuedMessages[contextKey]).toBeUndefined();
    });

    it("cleans up empty arrays", () => {
      useChatStore.getState().queueMessage(contextKey, "Test");
      const messageId = useChatStore.getState().queuedMessages[contextKey]?.[0].id || "";

      useChatStore.getState().deleteQueuedMessage(contextKey, messageId);

      const state = useChatStore.getState();
      expect(state.queuedMessages[contextKey]).toBeUndefined();
    });

    it("does nothing if message not found", () => {
      useChatStore.getState().queueMessage(contextKey, "Test");

      useChatStore.getState().deleteQueuedMessage(contextKey, "nonexistent-id");

      const state = useChatStore.getState();
      expect(state.queuedMessages[contextKey]).toHaveLength(1);
    });

    it("only removes specified message", () => {
      useChatStore.getState().queueMessage(contextKey, "First");
      useChatStore.getState().queueMessage(contextKey, "Second");
      useChatStore.getState().queueMessage(contextKey, "Third");
      const secondId = useChatStore.getState().queuedMessages[contextKey]?.[1].id || "";

      useChatStore.getState().deleteQueuedMessage(contextKey, secondId);

      const state = useChatStore.getState();
      expect(state.queuedMessages[contextKey]).toHaveLength(2);
      expect(state.queuedMessages[contextKey]?.[0].content).toBe("First");
      expect(state.queuedMessages[contextKey]?.[1].content).toBe("Third");
    });

    it("preserves other context queues", () => {
      const taskKey = "task:task-123";
      const execKey = "task_execution:task-123";
      useChatStore.getState().queueMessage(taskKey, "First");
      useChatStore.getState().queueMessage(execKey, "Second");
      const messageId = useChatStore.getState().queuedMessages[taskKey]?.[0].id || "";

      useChatStore.getState().deleteQueuedMessage(taskKey, messageId);

      const state = useChatStore.getState();
      expect(state.queuedMessages[execKey]).toHaveLength(1);
    });
  });

  describe("startEditingQueuedMessage", () => {
    const contextKey = "task:test-task";

    it("sets isEditing to true", () => {
      useChatStore.getState().queueMessage(contextKey, "Test");
      const messageId = useChatStore.getState().queuedMessages[contextKey]?.[0].id || "";

      useChatStore.getState().startEditingQueuedMessage(contextKey, messageId);

      const state = useChatStore.getState();
      expect(state.queuedMessages[contextKey]?.[0].isEditing).toBe(true);
    });

    it("does nothing if message not found", () => {
      useChatStore.getState().queueMessage(contextKey, "Test");

      useChatStore.getState().startEditingQueuedMessage(contextKey, "nonexistent-id");

      const state = useChatStore.getState();
      expect(state.queuedMessages[contextKey]?.[0].isEditing).toBe(false);
    });
  });

  describe("stopEditingQueuedMessage", () => {
    const contextKey = "task:test-task";

    it("sets isEditing to false", () => {
      useChatStore.getState().queueMessage(contextKey, "Test");
      const messageId = useChatStore.getState().queuedMessages[contextKey]?.[0].id || "";
      useChatStore.setState({
        queuedMessages: { [contextKey]: [{ ...useChatStore.getState().queuedMessages[contextKey]![0], isEditing: true }] },
      });

      useChatStore.getState().stopEditingQueuedMessage(contextKey, messageId);

      const state = useChatStore.getState();
      expect(state.queuedMessages[contextKey]?.[0].isEditing).toBe(false);
    });

    it("does nothing if message not found", () => {
      useChatStore.getState().queueMessage(contextKey, "Test");
      useChatStore.setState({
        queuedMessages: { [contextKey]: [{ ...useChatStore.getState().queuedMessages[contextKey]![0], isEditing: true }] },
      });

      useChatStore.getState().stopEditingQueuedMessage(contextKey, "nonexistent-id");

      const state = useChatStore.getState();
      expect(state.queuedMessages[contextKey]?.[0].isEditing).toBe(true);
    });
  });

  describe("processQueue", () => {
    const contextKey = "task:test-task";

    it("removes first message from queue", async () => {
      useChatStore.getState().queueMessage(contextKey, "First");
      useChatStore.getState().queueMessage(contextKey, "Second");

      await useChatStore.getState().processQueue(contextKey);

      const state = useChatStore.getState();
      expect(state.queuedMessages[contextKey]).toHaveLength(1);
      expect(state.queuedMessages[contextKey]?.[0].content).toBe("Second");
    });

    it("does nothing if queue is empty", async () => {
      await useChatStore.getState().processQueue(contextKey);

      const state = useChatStore.getState();
      expect(state.queuedMessages[contextKey]).toBeUndefined();
    });
  });

});

describe("getContextKey", () => {
  it("returns session key for ideation context", () => {
    const context = createTestContext({
      view: "ideation",
      ideationSessionId: "session-123",
    });

    const key = getContextKey(context);

    expect(key).toBe("session:session-123");
  });

  it("returns task key for task_detail context", () => {
    const context = createTestContext({
      view: "task_detail",
      selectedTaskId: "task-456",
    });

    const key = getContextKey(context);

    expect(key).toBe("task:task-456");
  });

  it("returns project key for kanban context", () => {
    const context = createTestContext({
      view: "kanban",
      projectId: "project-789",
    });

    const key = getContextKey(context);

    expect(key).toBe("project:project-789");
  });

  it("returns project key for activity context", () => {
    const context = createTestContext({
      view: "activity",
      projectId: "project-abc",
    });

    const key = getContextKey(context);

    expect(key).toBe("project:project-abc");
  });

  it("returns project key for activity context", () => {
    const context = createTestContext({
      view: "activity",
      projectId: "project-def",
    });

    const key = getContextKey(context);

    expect(key).toBe("project:project-def");
  });
});

describe("selectors", () => {
  beforeEach(() => {
    useChatStore.setState({
      messages: {},
      context: null,
      isLoading: false,
      activeConversationIds: {},
      activeAgentRunIds: {},
      activeAgentRunHarnesses: {},
      queuedMessages: {},
      agentStatus: {},
      agentActivityLabels: {},
      isSending: {},
    });
  });

  describe("selectMessagesForContext", () => {
    it("returns messages for context", () => {
      const messages = [
        createTestMessage({ id: "msg-1", content: "Hello" }),
        createTestMessage({ id: "msg-2", content: "World" }),
      ];
      useChatStore.setState({
        messages: { "session:session-1": messages },
      });

      const result = selectMessagesForContext("session:session-1")(useChatStore.getState());

      expect(result).toHaveLength(2);
      expect(result[0].content).toBe("Hello");
      expect(result[1].content).toBe("World");
    });

    it("returns empty array for unknown context", () => {
      const result = selectMessagesForContext("session:unknown")(useChatStore.getState());

      expect(result).toHaveLength(0);
    });
  });

  describe("selectMessageCount", () => {
    it("returns message count for context", () => {
      useChatStore.setState({
        messages: {
          "session:session-1": [createTestMessage(), createTestMessage(), createTestMessage()],
        },
      });

      const result = selectMessageCount("session:session-1")(useChatStore.getState());

      expect(result).toBe(3);
    });

    it("returns 0 for unknown context", () => {
      const result = selectMessageCount("session:unknown")(useChatStore.getState());

      expect(result).toBe(0);
    });
  });

  describe("selectQueuedMessages", () => {
    const contextKey = "task:test-task";

    it("returns queued messages for context", () => {
      useChatStore.getState().queueMessage(contextKey, "First");
      useChatStore.getState().queueMessage(contextKey, "Second");

      const result = selectQueuedMessages(contextKey)(useChatStore.getState());

      expect(result).toHaveLength(2);
      expect(result[0].content).toBe("First");
      expect(result[1].content).toBe("Second");
    });

    it("returns empty array for unknown context", () => {
      const result = selectQueuedMessages("unknown-context")(useChatStore.getState());

      expect(result).toHaveLength(0);
    });

    it("returns empty array when no queued messages exist", () => {
      const result = selectQueuedMessages(contextKey)(useChatStore.getState());

      expect(result).toHaveLength(0);
    });
  });

  describe("selectIsAgentRunning", () => {
    const contextKey = "task:test-task";

    it("returns true when agent is running for context", () => {
      useChatStore.getState().setAgentRunning(contextKey, true);

      const result = selectIsAgentRunning(contextKey)(useChatStore.getState());

      expect(result).toBe(true);
    });

    it("returns false when agent is not running for context", () => {
      const result = selectIsAgentRunning(contextKey)(useChatStore.getState());

      expect(result).toBe(false);
    });

    it("returns false for unknown context", () => {
      useChatStore.getState().setAgentRunning("other-context", true);

      const result = selectIsAgentRunning(contextKey)(useChatStore.getState());

      expect(result).toBe(false);
    });
  });

  describe("selectAgentActivityLabel", () => {
    const contextKey = "project:conversation-1";

    it("returns the trimmed activity label for a context", () => {
      useChatStore.getState().setAgentActivityLabel(contextKey, "  Setup workspace  ");

      expect(selectAgentActivityLabel(contextKey)(useChatStore.getState())).toBe(
        "Setup workspace"
      );
    });

    it("returns null when a context has no activity label", () => {
      expect(selectAgentActivityLabel(contextKey)(useChatStore.getState())).toBeNull();
    });
  });

  describe("selectActiveConversationId", () => {
    const storeKeyA = "session:session-a";
    const storeKeyB = "task_execution:task-b";

    it("returns active conversation ID for storeKey", () => {
      useChatStore.setState({ activeConversationIds: { [storeKeyA]: "conv-123" } });

      const result = selectActiveConversationId(storeKeyA)(useChatStore.getState());

      expect(result).toBe("conv-123");
    });

    it("returns undefined when no active conversation for storeKey", () => {
      const result = selectActiveConversationId(storeKeyA)(useChatStore.getState());

      expect(result).toBeUndefined();
    });

    it("isolates results — storeKey A does not return storeKey B value", () => {
      useChatStore.setState({
        activeConversationIds: { [storeKeyA]: "conv-a", [storeKeyB]: "conv-b" },
      });

      expect(selectActiveConversationId(storeKeyA)(useChatStore.getState())).toBe("conv-a");
      expect(selectActiveConversationId(storeKeyB)(useChatStore.getState())).toBe("conv-b");
    });
  });

  describe("setEffectiveModel / selectEffectiveModel", () => {
    const storeKey = "session:session-x";

    it("setEffectiveModel stores model for storeKey", () => {
      const model = { id: "claude-sonnet-4-6", label: "Sonnet 4.6" };
      useChatStore.getState().setEffectiveModel(storeKey, model);

      const result = selectEffectiveModel(storeKey)(useChatStore.getState());
      expect(result).toEqual(model);
    });

    it("selectEffectiveModel returns undefined when no model set", () => {
      const result = selectEffectiveModel("session:unknown")(useChatStore.getState());
      expect(result).toBeUndefined();
    });

    it("setEffectiveModel overwrites previous model for same storeKey", () => {
      useChatStore.getState().setEffectiveModel(storeKey, { id: "model-a", label: "Model A" });
      useChatStore.getState().setEffectiveModel(storeKey, { id: "model-b", label: "Model B" });

      const result = selectEffectiveModel(storeKey)(useChatStore.getState());
      expect(result).toEqual({ id: "model-b", label: "Model B" });
    });

    it("setEffectiveModel does not affect other storeKeys", () => {
      const otherKey = "session:session-y";
      useChatStore.getState().setEffectiveModel(storeKey, { id: "model-a", label: "Model A" });

      const result = selectEffectiveModel(otherKey)(useChatStore.getState());
      expect(result).toBeUndefined();
    });
  });

});
