/**
 * useGlobalAgentLifecycle — always-on global agent lifecycle hook tests.
 *
 * Tests verify:
 * - run_started sets agentStatus to generating globally
 * - run_completed, stopped, error: guardedTermination sets idle / re-asserts generating
 * - turn_completed: sets waiting_for_input with verification child guard
 * - Verification child reverse link cleanup on child termination
 * - ask-user-question state survives terminal lifecycle events
 * - Error toasts with deterministic id for task_execution/review/merge
 * - heartbeat/task events update lastAgentEventTimestamp
 * - watchdog guard: run_started does NOT update lastAgentEvent when already generating
 * - Cross-session integration: agentStatus populated without IntegratedChatPanel
 * - Verification cache invalidated on abnormal child termination
 */

import { describe, it, expect, vi, beforeEach } from "vitest";
import { renderHook, act } from "@testing-library/react";

// ============================================================================
// Hoisted mocks
// ============================================================================

const chatStoreMocks = vi.hoisted(() => ({
  setAgentStatus: vi.fn(),
  agentStatus: {} as Record<string, string>,
  activeAgentRunIds: {} as Record<string, string>,
  activeAgentRunHarnesses: {} as Record<string, string | null>,
  lastAgentEventTimestamp: {} as Record<string, number>,
  updateLastAgentEvent: vi.fn(),
  setAgentActivityLabel: vi.fn(),
  activeConversationIds: {} as Record<string, string | null>,
  setActiveConversation: vi.fn(),
  setActiveAgentRun: vi.fn(),
  clearActiveAgentRun: vi.fn(),
  setEffectiveModel: vi.fn(),
  isSending: {} as Record<string, boolean>,
  setAgentRunning: vi.fn(),
}));

vi.mock("@/stores/chatStore", () => ({
  selectAgentStatus:
    (storeKey: string) =>
    (state: typeof chatStoreMocks): string =>
      state.agentStatus[storeKey] ?? "idle",
  useChatStore: Object.assign(
    vi.fn((selector: (s: typeof chatStoreMocks) => unknown) => selector(chatStoreMocks)),
    { getState: () => chatStoreMocks }
  ),
}));

const uiStoreMocks = vi.hoisted(() => ({
  clearActiveQuestion: vi.fn(),
}));

vi.mock("@/stores/uiStore", () => ({
  useUiStore: Object.assign(
    vi.fn((selector: (s: typeof uiStoreMocks) => unknown) => selector(uiStoreMocks)),
    { getState: () => uiStoreMocks }
  ),
}));

vi.mock("sonner", () => ({
  toast: { success: vi.fn(), warning: vi.fn(), error: vi.fn(), info: vi.fn() },
}));

const runtimeReconcileMocks = vi.hoisted(() => ({
  getRuntimeIndex: vi.fn(),
  knownConversations: vi.fn(),
}));

vi.mock("@/api/chat", () => ({
  chatApi: {
    getAgentConversationRuntimeIndex: runtimeReconcileMocks.getRuntimeIndex,
  },
}));

vi.mock("@/components/agents/useAgentSidebarRunningStates", () => ({
  getKnownAgentSidebarConversations: runtimeReconcileMocks.knownConversations,
}));

// Capture event bus subscriptions so tests can fire events manually
const subscriptions = new Map<string, ((...args: unknown[]) => void)[]>();

function fireEvent<T>(event: string, payload: T) {
  const handlers = subscriptions.get(event);
  if (handlers) for (const h of handlers) h(payload as unknown);
}

vi.mock("@/providers/EventProvider", () => ({
  useEventBus: () => ({
    subscribe: (event: string, handler: (...args: unknown[]) => void) => {
      if (!subscriptions.has(event)) subscriptions.set(event, []);
      subscriptions.get(event)!.push(handler);
      return () => {
        const hs = subscriptions.get(event);
        if (hs) {
          const i = hs.indexOf(handler);
          if (i >= 0) hs.splice(i, 1);
        }
      };
    },
  }),
}));

const mockInvalidateQueries = vi.fn().mockResolvedValue(undefined);
const mockGetQueryData = vi.fn().mockReturnValue(undefined);

vi.mock("@tanstack/react-query", () => ({
  useQueryClient: () => ({
    invalidateQueries: mockInvalidateQueries,
    getQueryData: (...args: unknown[]) => mockGetQueryData(...args),
  }),
}));

vi.mock("@/lib/logger", () => ({
  logger: { debug: vi.fn(), warn: vi.fn(), error: vi.fn(), info: vi.fn() },
}));

// ============================================================================
// Import hook and stores after mocks
// ============================================================================

import { useGlobalAgentLifecycle } from "./useGlobalAgentLifecycle";
import { useIdeationStore } from "@/stores/ideationStore";
import { toast } from "sonner";
import { requestRuntimeIndexReconcile } from "@/lib/remote/runtime-index-reconcile";
import { RemoteTransportError } from "@/lib/remote/transport-errors";
import { selectAgentStatus } from "@/stores/chatStore";
import { useEnvironmentStore } from "@/stores/environmentStore";

// ============================================================================
// Helpers
// ============================================================================

const PARENT_SESSION_ID = "parent-session-abc";
const CHILD_SESSION_ID = "child-session-xyz";

function mkRunStarted(contextType: string, contextId: string) {
  return {
    run_id: "run-1",
    context_type: contextType,
    context_id: contextId,
    conversation_id: "conv-" + contextId,
  };
}

function mkRunCompleted(contextType: string, contextId: string) {
  return {
    context_type: contextType,
    context_id: contextId,
    conversation_id: "conv-" + contextId,
    status: "completed",
  };
}

function mkTurnCompleted(contextType: string, contextId: string) {
  return {
    context_type: contextType,
    context_id: contextId,
    conversation_id: "conv-" + contextId,
    status: "turn_completed",
  };
}

function mkStopped(contextType: string, contextId: string) {
  return {
    context_type: contextType,
    context_id: contextId,
    conversation_id: "conv-" + contextId,
    agent_run_id: "run-" + contextId,
  };
}

function mkError(contextType: string, contextId: string, error = "process crashed") {
  return {
    context_type: contextType,
    context_id: contextId,
    conversation_id: "conv-" + contextId,
    error,
  };
}

// ============================================================================
// Test suite
// ============================================================================

describe("useGlobalAgentLifecycle", () => {
  beforeEach(() => {
    subscriptions.clear();
    mockInvalidateQueries.mockClear();
    mockGetQueryData.mockReturnValue(undefined);
    chatStoreMocks.setAgentStatus.mockClear();
    chatStoreMocks.updateLastAgentEvent.mockClear();
    chatStoreMocks.setAgentActivityLabel.mockClear();
    chatStoreMocks.setActiveConversation.mockClear();
    chatStoreMocks.setActiveAgentRun.mockClear();
    chatStoreMocks.clearActiveAgentRun.mockClear();
    chatStoreMocks.setEffectiveModel.mockClear();
    chatStoreMocks.setAgentRunning.mockClear();
    chatStoreMocks.agentStatus = {};
    chatStoreMocks.activeAgentRunIds = {};
    chatStoreMocks.activeAgentRunHarnesses = {};
    chatStoreMocks.lastAgentEventTimestamp = {};
    chatStoreMocks.activeConversationIds = {};
    chatStoreMocks.isSending = {};
    chatStoreMocks.setAgentStatus.mockImplementation((key, status) => {
      chatStoreMocks.agentStatus[key] = status;
    });
    chatStoreMocks.setAgentRunning.mockImplementation((key, running) => {
      if (running) chatStoreMocks.agentStatus[key] = "generating";
      else delete chatStoreMocks.agentStatus[key];
    });
    runtimeReconcileMocks.getRuntimeIndex.mockReset();
    runtimeReconcileMocks.knownConversations.mockReset();
    runtimeReconcileMocks.knownConversations.mockReturnValue([]);
    uiStoreMocks.clearActiveQuestion.mockClear();
    vi.mocked(toast.error).mockClear();

    useIdeationStore.setState({
      sessions: {},
      activeSessionId: null,
      isLoading: false,
      error: null,
      planArtifact: null,
      activeVerificationChildId: {},
    });
  });

  it("authoritatively reconciles known remote conversations and fails closed", async () => {
    useEnvironmentStore.setState({
      activeEnvironmentId: "env-remote",
      environments: [{ id: "env-remote", name: "Remote", kind: "remote" }],
    });
    runtimeReconcileMocks.knownConversations.mockReturnValue([
      {
        id: "conv-live",
        contextType: "project",
        contextId: "conv-live",
      },
    ]);
    const runningRow = {
      id: "run-1",
      group: "main",
      kind: "workspace",
      lifecycle: "running",
      statusLabel: "Agent working",
      title: "Agent run",
      mode: "agent",
      orderIndex: 0,
      orderStartedAt: null,
      completedAt: null,
      conversationId: "conv-live",
      contextType: "project",
      contextId: "conv-live",
      taskId: null,
      agentRunId: "run-1",
      parentSessionId: null,
      childSessionId: null,
      providerHarness: null,
      providerSessionId: null,
      errorMessage: null,
    } as const;
    runtimeReconcileMocks.getRuntimeIndex
      .mockResolvedValueOnce({ conversationId: "conv-live", rows: [runningRow] })
      .mockResolvedValueOnce({ conversationId: "conv-live", rows: [] })
      .mockRejectedValueOnce(new Error("host read failed"));
    renderHook(() => useGlobalAgentLifecycle());

    await act(async () => {
      requestRuntimeIndexReconcile("env-remote");
    });
    expect(
      selectAgentStatus("project:conv-live")(chatStoreMocks),
    ).toBe("generating");

    await act(async () => {
      requestRuntimeIndexReconcile("env-remote");
    });
    expect(selectAgentStatus("project:conv-live")(chatStoreMocks)).toBe("idle");

    chatStoreMocks.agentStatus["project:conv-live"] = "generating";
    await act(async () => {
      requestRuntimeIndexReconcile("env-remote");
    });
    expect(
      selectAgentStatus("project:conv-live")(chatStoreMocks),
    ).toBe("generating");
  });

  it("stops fetching and writing when the environment switches mid-reconcile", async () => {
    useEnvironmentStore.setState({
      activeEnvironmentId: "env-remote",
      environments: [
        { id: "local", name: "This Mac", kind: "local" },
        { id: "env-remote", name: "Remote", kind: "remote" },
      ],
    });
    runtimeReconcileMocks.knownConversations.mockReturnValue([
      { id: "conv-first", contextType: "project", contextId: "conv-first" },
      { id: "conv-second", contextType: "project", contextId: "conv-second" },
    ]);
    runtimeReconcileMocks.getRuntimeIndex.mockResolvedValue({
      conversationId: "conv-first",
      rows: [{
        id: "run-1",
        group: "main",
        kind: "workspace",
        lifecycle: "running",
        statusLabel: "Agent working",
        title: "Agent run",
        mode: "agent",
        orderIndex: 0,
        orderStartedAt: null,
        completedAt: null,
        conversationId: "conv-first",
        contextType: "project",
        contextId: "conv-first",
        taskId: null,
        agentRunId: "run-1",
        parentSessionId: null,
        childSessionId: null,
        providerHarness: null,
        providerSessionId: null,
        errorMessage: null,
      }],
    });
    chatStoreMocks.setActiveConversation.mockImplementationOnce(() => {
      useEnvironmentStore.setState({ activeEnvironmentId: "local" });
    });
    renderHook(() => useGlobalAgentLifecycle());

    await act(async () => {
      requestRuntimeIndexReconcile("env-remote");
    });

    expect(runtimeReconcileMocks.getRuntimeIndex).toHaveBeenCalledTimes(1);
    expect(chatStoreMocks.setActiveConversation).not.toHaveBeenCalledWith(
      "project:conv-second",
      "conv-second",
    );
  });

  it("lets a newer reconcile generation prevent an older snapshot write", async () => {
    useEnvironmentStore.setState({
      activeEnvironmentId: "env-remote",
      environments: [{ id: "env-remote", name: "Remote", kind: "remote" }],
    });
    runtimeReconcileMocks.knownConversations.mockReturnValue([
      { id: "conv-live", contextType: "project", contextId: "conv-live" },
    ]);
    let resolveOld!: (value: unknown) => void;
    runtimeReconcileMocks.getRuntimeIndex
      .mockImplementationOnce(
        () => new Promise((resolve) => { resolveOld = resolve; }),
      )
      .mockResolvedValueOnce({ conversationId: "conv-live", rows: [] });
    renderHook(() => useGlobalAgentLifecycle());

    act(() => requestRuntimeIndexReconcile("env-remote"));
    await act(async () => {
      requestRuntimeIndexReconcile("env-remote");
    });
    chatStoreMocks.setActiveConversation.mockClear();

    await act(async () => {
      resolveOld({
        conversationId: "conv-live",
        rows: [{
          id: "old-run",
          group: "main",
          kind: "workspace",
          lifecycle: "running",
          statusLabel: "Agent working",
          title: "Old run",
          mode: "agent",
          orderIndex: 0,
          orderStartedAt: null,
          completedAt: null,
          conversationId: "conv-live",
          contextType: "project",
          contextId: "conv-live",
          taskId: null,
          agentRunId: "old-run",
          parentSessionId: null,
          childSessionId: null,
          providerHarness: null,
          providerSessionId: null,
          errorMessage: null,
        }],
      });
    });

    expect(chatStoreMocks.setActiveConversation).not.toHaveBeenCalled();
    expect(chatStoreMocks.setAgentStatus).not.toHaveBeenCalledWith(
      "project:conv-live",
      "generating",
    );
  });

  it("ignores background environments and stops fan-out after command unavailable", async () => {
    useEnvironmentStore.setState({
      activeEnvironmentId: "env-active",
      environments: [{ id: "env-active", name: "Remote", kind: "remote" }],
    });
    runtimeReconcileMocks.knownConversations.mockReturnValue([
      { id: "conv-1", contextType: "project", contextId: "conv-1" },
      { id: "conv-2", contextType: "project", contextId: "conv-2" },
    ]);
    runtimeReconcileMocks.getRuntimeIndex.mockRejectedValue(
      new RemoteTransportError({
        code: "REMOTE_COMMAND_UNAVAILABLE",
        message: "not registered",
        environmentId: "env-active",
        cmd: "get_agent_conversation_runtime_index",
      }),
    );
    renderHook(() => useGlobalAgentLifecycle());

    await act(async () => {
      requestRuntimeIndexReconcile("env-background");
    });
    expect(runtimeReconcileMocks.getRuntimeIndex).not.toHaveBeenCalled();

    await act(async () => {
      requestRuntimeIndexReconcile("env-active");
    });
    expect(runtimeReconcileMocks.getRuntimeIndex).toHaveBeenCalledTimes(1);
    expect(runtimeReconcileMocks.getRuntimeIndex).toHaveBeenCalledWith("conv-1");
  });

  it("shows one toast when multiple conversations fail in one reconcile", async () => {
    useEnvironmentStore.setState({
      activeEnvironmentId: "env-active",
      environments: [{ id: "env-active", name: "Remote", kind: "remote" }],
    });
    runtimeReconcileMocks.knownConversations.mockReturnValue([
      { id: "conv-1", contextType: "project", contextId: "conv-1" },
      { id: "conv-2", contextType: "project", contextId: "conv-2" },
    ]);
    runtimeReconcileMocks.getRuntimeIndex.mockRejectedValue(
      new Error("host read failed"),
    );
    renderHook(() => useGlobalAgentLifecycle());

    await act(async () => {
      requestRuntimeIndexReconcile("env-active");
    });

    expect(runtimeReconcileMocks.getRuntimeIndex).toHaveBeenCalledTimes(2);
    expect(toast.error).toHaveBeenCalledTimes(1);
  });

  // --------------------------------------------------------------------------
  // run_started
  // --------------------------------------------------------------------------

  it("run_started sets agentStatus to generating", () => {
    renderHook(() => useGlobalAgentLifecycle());

    act(() => {
      fireEvent("agent:run_started", mkRunStarted("ideation", "session-1"));
    });

    expect(chatStoreMocks.setAgentStatus).toHaveBeenCalledWith("session:session-1", "generating");
  });

  it("run_started marks active turns as agent working", () => {
    renderHook(() => useGlobalAgentLifecycle());

    act(() => {
      fireEvent("agent:run_started", {
        run_id: "run-1",
        context_type: "project",
        context_id: "project-1",
        conversation_id: "conversation-1",
      });
    });

    expect(chatStoreMocks.setAgentActivityLabel).toHaveBeenCalledWith(
      "project:conversation-1",
      "Agent working"
    );
  });

  it("startup_progress hydrates the project conversation activity label", () => {
    renderHook(() => useGlobalAgentLifecycle());

    act(() => {
      fireEvent("agent:startup_progress", {
        context_type: "project",
        context_id: "project-1",
        conversation_id: "conversation-1",
        stage: "prepare_workspace",
        label: "Setup workspace",
      });
    });

    expect(chatStoreMocks.updateLastAgentEvent).toHaveBeenCalledWith(
      "project:conversation-1"
    );
    expect(chatStoreMocks.setAgentActivityLabel).toHaveBeenCalledWith(
      "project:conversation-1",
      "Setup workspace"
    );
  });

  it("startup_progress falls back to the stage label when the payload label is not allowed", () => {
    renderHook(() => useGlobalAgentLifecycle());

    act(() => {
      fireEvent("agent:startup_progress", {
        context_type: "project",
        context_id: "project-1",
        conversation_id: "conversation-1",
        stage: "send_message",
        label: "Launching the selected runtime",
      });
    });

    expect(chatStoreMocks.setAgentActivityLabel).toHaveBeenCalledWith(
      "project:conversation-1",
      "Starting agent"
    );
  });

  it("startup_progress ignores unknown stages without an allowed label", () => {
    renderHook(() => useGlobalAgentLifecycle());

    act(() => {
      fireEvent("agent:startup_progress", {
        context_type: "project",
        context_id: "project-1",
        conversation_id: "conversation-1",
        stage: "unsupported_stage",
        label: "Launching the selected runtime",
      });
    });

    expect(chatStoreMocks.updateLastAgentEvent).not.toHaveBeenCalled();
    expect(chatStoreMocks.setAgentActivityLabel).not.toHaveBeenCalled();
  });

  it("run_started calls updateLastAgentEvent when not already generating", () => {
    chatStoreMocks.agentStatus = {};
    renderHook(() => useGlobalAgentLifecycle());

    act(() => {
      fireEvent("agent:run_started", mkRunStarted("task_execution", "task-1"));
    });

    expect(chatStoreMocks.updateLastAgentEvent).toHaveBeenCalledWith("task_execution:task-1");
  });

  it("run_started skips updateLastAgentEvent when already generating (queue re-run guard)", () => {
    chatStoreMocks.agentStatus = { "task_execution:task-1": "generating" };
    renderHook(() => useGlobalAgentLifecycle());

    act(() => {
      fireEvent("agent:run_started", mkRunStarted("task_execution", "task-1"));
    });

    expect(chatStoreMocks.updateLastAgentEvent).not.toHaveBeenCalled();
  });


  it("run_started populates activeConversationIds tracking for the context", () => {
    renderHook(() => useGlobalAgentLifecycle());

    act(() => {
      fireEvent("agent:run_started", mkRunStarted("ideation", "session-b"));
    });

    expect(chatStoreMocks.setActiveConversation).toHaveBeenCalledWith(
      "session:session-b",
      "conv-session-b"
    );
  });

  it("run_started records the active agent run id for stale terminal event guards", () => {
    renderHook(() => useGlobalAgentLifecycle());

    act(() => {
      fireEvent("agent:run_started", {
        ...mkRunStarted("project", "project-1"),
        conversation_id: "conv-project-1",
        run_id: "run-new",
        provider_harness: "claude",
      });
    });

    expect(chatStoreMocks.setActiveAgentRun).toHaveBeenCalledWith(
      "project:conv-project-1",
      "run-new",
      "claude",
    );
  });

  // --------------------------------------------------------------------------
  // Cross-session integration (core bug scenario)
  // --------------------------------------------------------------------------

  it("(cross-session) run_started populates agentStatus for a session without a mounted chat panel", () => {
    // This is the core bug fix — no IntegratedChatPanel mounted, only GlobalEventListeners
    renderHook(() => useGlobalAgentLifecycle());

    act(() => {
      fireEvent("agent:run_started", mkRunStarted("ideation", "session-b"));
    });

    expect(chatStoreMocks.setAgentStatus).toHaveBeenCalledWith("session:session-b", "generating");
  });

  // --------------------------------------------------------------------------
  // run_started — effectiveModel wiring (acceptance criteria 2 & 4)
  // --------------------------------------------------------------------------

  it("run_started calls setEffectiveModel when effectiveModelId and effectiveModelLabel are present", () => {
    renderHook(() => useGlobalAgentLifecycle());

    act(() => {
      fireEvent("agent:run_started", {
        ...mkRunStarted("ideation", "session-1"),
        effective_model_id: "claude-sonnet-4-6",
        effective_model_label: "Sonnet 4.6",
      });
    });

    expect(chatStoreMocks.setEffectiveModel).toHaveBeenCalledWith(
      "session:session-1",
      { id: "claude-sonnet-4-6", label: "Sonnet 4.6" }
    );
  });

  it("run_started does NOT call setEffectiveModel when effectiveModelId is absent", () => {
    renderHook(() => useGlobalAgentLifecycle());

    act(() => {
      fireEvent("agent:run_started", mkRunStarted("ideation", "session-1"));
    });

    expect(chatStoreMocks.setEffectiveModel).not.toHaveBeenCalled();
  });

  it("run_started does NOT call setEffectiveModel when effectiveModelLabel is absent", () => {
    renderHook(() => useGlobalAgentLifecycle());

    act(() => {
      fireEvent("agent:run_started", {
        ...mkRunStarted("task_execution", "task-99"),
        effective_model_id: "claude-sonnet-4-6",
        // effective_model_label intentionally absent
      });
    });

    expect(chatStoreMocks.setEffectiveModel).not.toHaveBeenCalled();
  });

  // --------------------------------------------------------------------------
  // run_completed
  // --------------------------------------------------------------------------

  it("run_completed sets status to idle (no verification child)", () => {
    renderHook(() => useGlobalAgentLifecycle());

    act(() => {
      fireEvent("agent:run_completed", mkRunCompleted("task_execution", "task-1"));
    });

    expect(chatStoreMocks.setAgentStatus).toHaveBeenCalledWith("task_execution:task-1", "idle");
  });

  it("run_completed calls updateLastAgentEvent (final heartbeat)", () => {
    renderHook(() => useGlobalAgentLifecycle());

    act(() => {
      fireEvent("agent:run_completed", mkRunCompleted("task_execution", "task-1"));
    });

    expect(chatStoreMocks.updateLastAgentEvent).toHaveBeenCalledWith("task_execution:task-1");
  });


  it("run_completed re-asserts generating when parent has active verification child", () => {
    useIdeationStore.setState({
      activeVerificationChildId: { [PARENT_SESSION_ID]: CHILD_SESSION_ID },
    });
    renderHook(() => useGlobalAgentLifecycle());

    act(() => {
      fireEvent("agent:run_completed", mkRunCompleted("ideation", PARENT_SESSION_ID));
    });

    expect(chatStoreMocks.setAgentStatus).toHaveBeenCalledWith(
      "session:" + PARENT_SESSION_ID,
      "generating"
    );
  });

  it("run_completed: stale conversation_id is ignored — status stays unchanged", () => {
    // Set active conversation to a different (newer) conversation
    chatStoreMocks.activeConversationIds["task_execution:task-1"] = "conv-NEW";
    renderHook(() => useGlobalAgentLifecycle());

    // Fire run_completed with old conversation_id
    act(() => {
      fireEvent("agent:run_completed", {
        ...mkRunCompleted("task_execution", "task-1"),
        conversation_id: "conv-OLD",
      });
    });

    expect(chatStoreMocks.setAgentStatus).not.toHaveBeenCalledWith(
      "task_execution:task-1",
      "idle"
    );
  });

  it("run_completed: matching conversation_id clears status to idle", () => {
    chatStoreMocks.activeConversationIds["task_execution:task-1"] = "conv-task-1";
    renderHook(() => useGlobalAgentLifecycle());

    act(() => {
      fireEvent("agent:run_completed", mkRunCompleted("task_execution", "task-1"));
    });

    expect(chatStoreMocks.setAgentStatus).toHaveBeenCalledWith("task_execution:task-1", "idle");
  });

  it("run_completed ignores stale terminal events from an older run on the same conversation", () => {
    chatStoreMocks.activeConversationIds = { "project:conv-project-1": "conv-project-1" };
    chatStoreMocks.activeAgentRunIds = { "project:conv-project-1": "run-new" };
    chatStoreMocks.activeAgentRunHarnesses = { "project:conv-project-1": "codex" };
    chatStoreMocks.agentStatus = { "project:conv-project-1": "generating" };
    renderHook(() => useGlobalAgentLifecycle());

    act(() => {
      fireEvent("agent:run_completed", {
        context_type: "project",
        context_id: "project-1",
        conversation_id: "conv-project-1",
        run_id: "run-old",
      });
    });

    expect(chatStoreMocks.setAgentStatus).not.toHaveBeenCalledWith(
      "project:conv-project-1",
      "idle"
    );
    expect(chatStoreMocks.clearActiveAgentRun).not.toHaveBeenCalled();
    expect(chatStoreMocks.activeAgentRunIds["project:conv-project-1"]).toBe("run-new");
    expect(chatStoreMocks.activeAgentRunHarnesses["project:conv-project-1"]).toBe("codex");
  });

  it("run_completed without an id preserves a newer run and harness pair", () => {
    chatStoreMocks.activeConversationIds = { "project:conv-project-1": "conv-project-1" };
    chatStoreMocks.activeAgentRunIds = { "project:conv-project-1": "run-new" };
    chatStoreMocks.activeAgentRunHarnesses = { "project:conv-project-1": "codex" };
    chatStoreMocks.agentStatus = { "project:conv-project-1": "generating" };
    renderHook(() => useGlobalAgentLifecycle());

    act(() => {
      fireEvent("agent:run_completed", {
        context_type: "project",
        context_id: "project-1",
        conversation_id: "conv-project-1",
      });
    });

    expect(chatStoreMocks.setAgentStatus).not.toHaveBeenCalledWith(
      "project:conv-project-1",
      "idle"
    );
    expect(chatStoreMocks.clearActiveAgentRun).not.toHaveBeenCalled();
    expect(chatStoreMocks.activeAgentRunIds["project:conv-project-1"]).toBe("run-new");
    expect(chatStoreMocks.activeAgentRunHarnesses["project:conv-project-1"]).toBe("codex");
  });

  it("run_completed clears the active run id when the terminal event matches", () => {
    chatStoreMocks.activeConversationIds = { "project:conv-project-1": "conv-project-1" };
    chatStoreMocks.activeAgentRunIds = { "project:conv-project-1": "run-new" };
    renderHook(() => useGlobalAgentLifecycle());

    act(() => {
      fireEvent("agent:run_completed", {
        context_type: "project",
        context_id: "project-1",
        conversation_id: "conv-project-1",
        run_id: "run-new",
      });
    });

    expect(chatStoreMocks.setAgentStatus).toHaveBeenCalledWith(
      "project:conv-project-1",
      "idle"
    );
    expect(chatStoreMocks.clearActiveAgentRun).toHaveBeenCalledWith(
      "project:conv-project-1",
      "run-new"
    );
  });

  // --------------------------------------------------------------------------
  // turn_completed
  // --------------------------------------------------------------------------

  it("turn_completed sets status to waiting_for_input for non-ideation context", () => {
    renderHook(() => useGlobalAgentLifecycle());

    act(() => {
      fireEvent("agent:turn_completed", mkTurnCompleted("task_execution", "task-1"));
    });

    expect(chatStoreMocks.setAgentStatus).toHaveBeenCalledWith("task_execution:task-1", "waiting_for_input");
  });

  it("turn_completed sets waiting_for_input for ideation without verification child", () => {
    useIdeationStore.setState({ activeVerificationChildId: {} });
    renderHook(() => useGlobalAgentLifecycle());

    act(() => {
      fireEvent("agent:turn_completed", mkTurnCompleted("ideation", "session-1"));
    });

    expect(chatStoreMocks.setAgentStatus).toHaveBeenCalledWith("session:session-1", "waiting_for_input");
  });

  it("turn_completed re-asserts generating for ideation with active verification child", () => {
    useIdeationStore.setState({
      activeVerificationChildId: { [PARENT_SESSION_ID]: CHILD_SESSION_ID },
    });
    renderHook(() => useGlobalAgentLifecycle());

    act(() => {
      fireEvent("agent:turn_completed", mkTurnCompleted("ideation", PARENT_SESSION_ID));
    });

    expect(chatStoreMocks.setAgentStatus).toHaveBeenCalledWith(
      "session:" + PARENT_SESSION_ID,
      "generating"
    );
  });


  it("turn_completed: stale conversation_id is ignored — status not updated", () => {
    chatStoreMocks.activeConversationIds["task_execution:task-1"] = "conv-NEW";
    renderHook(() => useGlobalAgentLifecycle());

    act(() => {
      fireEvent("agent:turn_completed", {
        ...mkTurnCompleted("task_execution", "task-1"),
        conversation_id: "conv-OLD",
      });
    });

    expect(chatStoreMocks.setAgentStatus).not.toHaveBeenCalled();
  });

  it("turn_completed ignores stale terminal events from an older run on the same conversation", () => {
    chatStoreMocks.activeConversationIds = { "project:conv-project-1": "conv-project-1" };
    chatStoreMocks.activeAgentRunIds = { "project:conv-project-1": "run-new" };
    chatStoreMocks.agentStatus = { "project:conv-project-1": "generating" };
    renderHook(() => useGlobalAgentLifecycle());

    act(() => {
      fireEvent("agent:turn_completed", {
        context_type: "project",
        context_id: "project-1",
        conversation_id: "conv-project-1",
        run_id: "run-old",
      });
    });

    expect(chatStoreMocks.setAgentStatus).not.toHaveBeenCalledWith(
      "project:conv-project-1",
      "waiting_for_input"
    );
  });

  // --------------------------------------------------------------------------
  // stopped
  // --------------------------------------------------------------------------

  it("stopped sets status to idle", () => {
    renderHook(() => useGlobalAgentLifecycle());

    act(() => {
      fireEvent("agent:stopped", mkStopped("review", "task-2"));
    });

    expect(chatStoreMocks.setAgentStatus).toHaveBeenCalledWith("review:task-2", "idle");
  });


  it("stopped ignores stale events using agent_run_id", () => {
    chatStoreMocks.activeConversationIds = { "project:conv-project-1": "conv-project-1" };
    chatStoreMocks.activeAgentRunIds = { "project:conv-project-1": "run-new" };
    renderHook(() => useGlobalAgentLifecycle());

    act(() => {
      fireEvent("agent:stopped", {
        context_type: "project",
        context_id: "project-1",
        conversation_id: "conv-project-1",
        agent_run_id: "run-old",
      });
    });

    expect(chatStoreMocks.setAgentStatus).not.toHaveBeenCalledWith(
      "project:conv-project-1",
      "idle"
    );
    expect(chatStoreMocks.clearActiveAgentRun).not.toHaveBeenCalled();
  });

  // --------------------------------------------------------------------------
  // error
  // --------------------------------------------------------------------------

  it("error sets status to idle", () => {
    renderHook(() => useGlobalAgentLifecycle());

    act(() => {
      fireEvent("agent:error", mkError("task_execution", "task-1"));
    });

    expect(chatStoreMocks.setAgentStatus).toHaveBeenCalledWith("task_execution:task-1", "idle");
  });

  it("error shows toast for task_execution with deterministic id", () => {
    renderHook(() => useGlobalAgentLifecycle());

    act(() => {
      fireEvent("agent:error", mkError("task_execution", "task-1", "OOM error"));
    });

    expect(toast.error).toHaveBeenCalledWith(
      expect.stringContaining("Worker agent error"),
      expect.objectContaining({ id: "error:task_execution:task-1" })
    );
  });

  it("error shows toast for review context with deterministic id", () => {
    renderHook(() => useGlobalAgentLifecycle());

    act(() => {
      fireEvent("agent:error", mkError("review", "task-2", "timeout"));
    });

    expect(toast.error).toHaveBeenCalledWith(
      expect.stringContaining("Reviewer agent error"),
      expect.objectContaining({ id: "error:review:task-2" })
    );
  });

  it("error shows toast for merge context with deterministic id", () => {
    renderHook(() => useGlobalAgentLifecycle());

    act(() => {
      fireEvent("agent:error", mkError("merge", "task-3", "conflict"));
    });

    expect(toast.error).toHaveBeenCalledWith(
      expect.stringContaining("Merger agent error"),
      expect.objectContaining({ id: "error:merge:task-3" })
    );
  });

  it("error does NOT show toast for ideation context", () => {
    renderHook(() => useGlobalAgentLifecycle());

    act(() => {
      fireEvent("agent:error", mkError("ideation", "session-1", "crash"));
    });

    expect(toast.error).not.toHaveBeenCalled();
  });


  it("error ignores stale terminal events from an older run", () => {
    chatStoreMocks.activeConversationIds = { "task_execution:task-1": "conv-task-1" };
    chatStoreMocks.activeAgentRunIds = { "task_execution:task-1": "run-new" };
    renderHook(() => useGlobalAgentLifecycle());

    act(() => {
      fireEvent("agent:error", {
        context_type: "task_execution",
        context_id: "task-1",
        conversation_id: "conv-task-1",
        agent_run_id: "run-old",
        error: "old run failed after resume",
      });
    });

    expect(chatStoreMocks.setAgentStatus).not.toHaveBeenCalledWith(
      "task_execution:task-1",
      "idle"
    );
    expect(toast.error).not.toHaveBeenCalled();
  });

  // --------------------------------------------------------------------------
  // durable question preservation
  // --------------------------------------------------------------------------

  it("clearActiveQuestion is not called for ideation context on termination", () => {
    renderHook(() => useGlobalAgentLifecycle());

    act(() => {
      fireEvent("agent:run_completed", mkRunCompleted("ideation", "session-1"));
    });

    expect(uiStoreMocks.clearActiveQuestion).not.toHaveBeenCalled();
  });

  it("clearActiveQuestion NOT called for task_execution context", () => {
    renderHook(() => useGlobalAgentLifecycle());

    act(() => {
      fireEvent("agent:run_completed", mkRunCompleted("task_execution", "task-1"));
    });

    expect(uiStoreMocks.clearActiveQuestion).not.toHaveBeenCalled();
  });


  // --------------------------------------------------------------------------
  // Verification child reverse link
  // --------------------------------------------------------------------------

  it("child run_completed clears parent activeVerificationChildId and sets parent idle", () => {
    useIdeationStore.setState({
      activeVerificationChildId: { [PARENT_SESSION_ID]: CHILD_SESSION_ID },
    });
    renderHook(() => useGlobalAgentLifecycle());

    act(() => {
      fireEvent("agent:run_completed", mkRunCompleted("ideation", CHILD_SESSION_ID));
    });

    expect(useIdeationStore.getState().activeVerificationChildId[PARENT_SESSION_ID]).toBeNull();
    expect(chatStoreMocks.setAgentStatus).toHaveBeenCalledWith(
      "session:" + PARENT_SESSION_ID,
      "idle"
    );
  });

  it("child error event triggers reverse link and sets parent idle", () => {
    useIdeationStore.setState({
      activeVerificationChildId: { [PARENT_SESSION_ID]: CHILD_SESSION_ID },
    });
    renderHook(() => useGlobalAgentLifecycle());

    act(() => {
      fireEvent("agent:error", mkError("ideation", CHILD_SESSION_ID));
    });

    expect(useIdeationStore.getState().activeVerificationChildId[PARENT_SESSION_ID]).toBeNull();
    expect(chatStoreMocks.setAgentStatus).toHaveBeenCalledWith(
      "session:" + PARENT_SESSION_ID,
      "idle"
    );
  });

  it("child stopped triggers reverse link and sets parent idle", () => {
    useIdeationStore.setState({
      activeVerificationChildId: { [PARENT_SESSION_ID]: CHILD_SESSION_ID },
    });
    renderHook(() => useGlobalAgentLifecycle());

    act(() => {
      fireEvent("agent:stopped", mkStopped("ideation", CHILD_SESSION_ID));
    });

    expect(useIdeationStore.getState().activeVerificationChildId[PARENT_SESSION_ID]).toBeNull();
    expect(chatStoreMocks.setAgentStatus).toHaveBeenCalledWith(
      "session:" + PARENT_SESSION_ID,
      "idle"
    );
  });

  it("multi-parent isolation: only matching parent cleared; unrelated untouched", () => {
    const OTHER_PARENT = "other-parent";
    const OTHER_CHILD = "other-child";
    useIdeationStore.setState({
      activeVerificationChildId: {
        [PARENT_SESSION_ID]: CHILD_SESSION_ID,
        [OTHER_PARENT]: OTHER_CHILD,
      },
    });
    renderHook(() => useGlobalAgentLifecycle());

    act(() => {
      fireEvent("agent:run_completed", mkRunCompleted("ideation", CHILD_SESSION_ID));
    });

    expect(useIdeationStore.getState().activeVerificationChildId[PARENT_SESSION_ID]).toBeNull();
    expect(useIdeationStore.getState().activeVerificationChildId[OTHER_PARENT]).toBe(OTHER_CHILD);
  });

  // --------------------------------------------------------------------------
  // Verification cache invalidation on abnormal termination
  // --------------------------------------------------------------------------

  it("invalidates verification query when child terminates with inProgress=true", () => {
    mockGetQueryData.mockReturnValue({ inProgress: true });
    useIdeationStore.setState({
      activeVerificationChildId: { [PARENT_SESSION_ID]: CHILD_SESSION_ID },
    });
    renderHook(() => useGlobalAgentLifecycle());

    act(() => {
      fireEvent("agent:run_completed", mkRunCompleted("ideation", CHILD_SESSION_ID));
    });

    expect(mockInvalidateQueries).toHaveBeenCalledWith(
      expect.objectContaining({ queryKey: ["verification", PARENT_SESSION_ID] })
    );
  });

  it("does NOT invalidate verification when inProgress=false", () => {
    mockGetQueryData.mockReturnValue({ inProgress: false });
    useIdeationStore.setState({
      activeVerificationChildId: { [PARENT_SESSION_ID]: CHILD_SESSION_ID },
    });
    renderHook(() => useGlobalAgentLifecycle());

    act(() => {
      fireEvent("agent:run_completed", mkRunCompleted("ideation", CHILD_SESSION_ID));
    });

    const verificationInvalidation = mockInvalidateQueries.mock.calls.find(
      (c: unknown[]) =>
        JSON.stringify(c[0]) ===
        JSON.stringify({ queryKey: ["verification", PARENT_SESSION_ID] })
    );
    expect(verificationInvalidation).toBeUndefined();
  });

  // --------------------------------------------------------------------------
  // heartbeat / task events
  // --------------------------------------------------------------------------

  it("heartbeat updates lastAgentEventTimestamp via findStoreKeyForContextId scan", () => {
    // Pre-populate agentStatus so findStoreKeyForContextId can find the key
    chatStoreMocks.agentStatus = { "session:session-1": "generating" };
    renderHook(() => useGlobalAgentLifecycle());

    act(() => {
      fireEvent("agent:heartbeat", {
        conversation_id: "conv-1",
        context_id: "session-1",
        reason: "pid_alive",
      });
    });

    expect(chatStoreMocks.updateLastAgentEvent).toHaveBeenCalledWith("session:session-1");
  });

  it("heartbeat is no-op when no agentStatus entry exists for context_id", () => {
    chatStoreMocks.agentStatus = {};
    renderHook(() => useGlobalAgentLifecycle());

    act(() => {
      fireEvent("agent:heartbeat", {
        conversation_id: "conv-1",
        context_id: "unknown-session",
        reason: "pid_alive",
      });
    });

    expect(chatStoreMocks.updateLastAgentEvent).not.toHaveBeenCalled();
  });

  it("task_started updates lastAgentEventTimestamp via buildStoreKey when context_type provided", () => {
    renderHook(() => useGlobalAgentLifecycle());

    act(() => {
      fireEvent("agent:task_started", {
        conversation_id: "conv-1",
        context_id: "task-1",
        context_type: "task_execution",
      });
    });

    expect(chatStoreMocks.updateLastAgentEvent).toHaveBeenCalledWith("task_execution:task-1");
  });

  it("task_completed updates lastAgentEventTimestamp via buildStoreKey when context_type provided", () => {
    renderHook(() => useGlobalAgentLifecycle());

    act(() => {
      fireEvent("agent:task_completed", {
        conversation_id: "conv-1",
        context_id: "task-1",
        context_type: "task_execution",
      });
    });

    expect(chatStoreMocks.updateLastAgentEvent).toHaveBeenCalledWith("task_execution:task-1");
  });

  // --------------------------------------------------------------------------
  // conversation_created tracking
  // --------------------------------------------------------------------------

  it("conversation_created populates activeConversationIds when no existing entry", () => {
    renderHook(() => useGlobalAgentLifecycle());

    act(() => {
      fireEvent("agent:conversation_created", {
        conversation_id: "conv-new-123",
        context_type: "ideation",
        context_id: "session-x",
      });
    });

    expect(chatStoreMocks.setActiveConversation).toHaveBeenCalledWith(
      "session:session-x",
      "conv-new-123"
    );
  });

  it("conversation_created does NOT overwrite existing active conversation entry", () => {
    chatStoreMocks.activeConversationIds["session:session-x"] = "conv-existing";
    renderHook(() => useGlobalAgentLifecycle());

    act(() => {
      fireEvent("agent:conversation_created", {
        conversation_id: "conv-new-456",
        context_type: "ideation",
        context_id: "session-x",
      });
    });

    expect(chatStoreMocks.setActiveConversation).not.toHaveBeenCalled();
  });

  it("conversation_created works for non-ideation contexts (task_execution)", () => {
    renderHook(() => useGlobalAgentLifecycle());

    act(() => {
      fireEvent("agent:conversation_created", {
        conversation_id: "conv-exec-1",
        context_type: "task_execution",
        context_id: "task-abc",
      });
    });

    expect(chatStoreMocks.setActiveConversation).toHaveBeenCalledWith(
      "task_execution:task-abc",
      "conv-exec-1"
    );
  });
});
