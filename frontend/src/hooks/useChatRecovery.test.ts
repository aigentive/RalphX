/**
 * useChatRecovery hook tests
 *
 * Tests recovery effects: agent running state sync, stuck-running cleanup,
 * and mount-time thrashing guard.
 */

import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { renderHook, act } from "@testing-library/react";

// ============================================================================
// Mock infrastructure
// ============================================================================

const mockInvalidateQueries = vi.fn();

vi.mock("@tanstack/react-query", () => ({
  useQueryClient: () => ({
    invalidateQueries: mockInvalidateQueries,
  }),
}));

vi.mock("@/hooks/useChat", () => ({
  chatKeys: {
    conversation: (id: string) => ["chat", "conversations", id],
    conversationHistory: (id: string) => ["chat", "conversations", id, "history"],
    conversationTimeline: (id: string) => ["chat", "conversations", id, "timeline"],
    conversationList: (type: string, id: string) => ["chat", "conversation-list", type, id],
  },
  invalidateConversationDataQueries: (_queryClient: unknown, conversationId: string) => {
    mockInvalidateQueries({ queryKey: ["chat", "conversations", conversationId] });
    mockInvalidateQueries({ queryKey: ["chat", "conversations", conversationId, "history"] });
    mockInvalidateQueries({ queryKey: ["chat", "conversations", conversationId, "timeline"] });
  },
}));

vi.mock("@/hooks/useTasks", () => ({
  taskKeys: {
    list: (pid: string) => ["tasks", "list", pid],
    detail: (tid: string) => ["tasks", "detail", tid],
  },
}));

vi.mock("@/types/status", () => ({
  MERGE_STATUSES: ["pending_merge", "merging", "merge_conflict", "merge_incomplete"],
}));

vi.mock("@/api/chat", () => ({
  parseToolCalls: (raw: unknown) => Array.isArray(raw)
    ? raw.map((toolCall) => {
        if (toolCall == null || typeof toolCall !== "object") return toolCall;
        const record = toolCall as Record<string, unknown>;
        return typeof record.block_index === "number"
          ? { ...record, blockIndex: record.block_index }
          : record;
      })
    : [],
  chatApi: {
    isAgentRunning: vi.fn(),
    getConversationActiveState: vi.fn(),
  },
}));

// ============================================================================
// Import hook under test (after mocks)
// ============================================================================

import { useChatRecovery } from "./useChatRecovery";
import type { ContextType } from "@/types/chat-conversation";
import type { ToolCall } from "@/components/Chat/ToolCallIndicator";
import type { StreamingContentBlock } from "@/types/streaming-task";
import { chatApi } from "@/api/chat";
import {
  LOCAL_ENVIRONMENT_ID,
  useEnvironmentStore,
} from "@/stores/environmentStore";
import { projectPersistedStreamingContentBlocks } from "./chat-transcript/projection";
import { buildLiveTranscriptRows } from "@/components/Chat/ChatMessageList.liveRows";

const mockIsAgentRunning = vi.mocked(chatApi.isAgentRunning);
const mockGetConversationActiveState = vi.mocked(chatApi.getConversationActiveState);

// ============================================================================
// Helpers
// ============================================================================

interface DefaultProps {
  activeConversationId: string | null | undefined;
  storeContextKey: string;
  currentContextType: ContextType;
  currentContextId: string;
  isHistoryMode: boolean;
  isAgentContext: boolean;
  isAgentRunning: boolean;
  isGenerating: boolean;
  isConversationInCurrentContext: boolean;
  agentRunStatus: string | undefined;
  activeAgentRunId?: string;
  isVisible: boolean;
  persistedStreamingContentBlocks?: readonly StreamingContentBlock[];
  setAgentRunning: ReturnType<typeof vi.fn>;
  setStreamingToolCalls?: ReturnType<typeof vi.fn>;
  setStreamingContentBlocks?: ReturnType<typeof vi.fn>;
  setStreamingTasks?: ReturnType<typeof vi.fn>;
  selectedTaskId: string | undefined;
  ideationSessionId: string | undefined;
  projectId: string;
  effectiveStatus: string | undefined;
}

function makeProps(overrides?: Partial<DefaultProps>): DefaultProps {
  return {
    activeConversationId: "conv-abc",
    storeContextKey: "task_execution:task-1",
    currentContextType: "task_execution" as ContextType,
    currentContextId: "task-1",
    isHistoryMode: false,
    isAgentContext: true,
    isAgentRunning: false,
    isGenerating: false,
    isConversationInCurrentContext: true,
    agentRunStatus: undefined,
    isVisible: true,
    setAgentRunning: vi.fn(),
    setStreamingTasks: undefined,
    selectedTaskId: "task-1",
    ideationSessionId: undefined,
    projectId: "project-1",
    effectiveStatus: "executing",
    ...overrides,
  };
}

// ============================================================================
// Tests
// ============================================================================

describe("useChatRecovery", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    mockInvalidateQueries.mockClear();
    // Default: process not running. Effect 2 calls chatApi.isAgentRunning()
    // which must return a Promise (not undefined) to avoid TypeError on .then().
    mockIsAgentRunning.mockResolvedValue(false);
    mockGetConversationActiveState.mockResolvedValue({
      is_active: false,
      tool_calls: [],
      streaming_tasks: [],
      partial_text: "",
    });
    useEnvironmentStore.setState({
      activeEnvironmentId: LOCAL_ENVIRONMENT_ID,
      environments: [
        { id: LOCAL_ENVIRONMENT_ID, name: "This Mac", kind: "local" },
      ],
    });
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  describe("agent running state sync", () => {
    it("should set agent running when backend reports running status", () => {
      const props = makeProps({ agentRunStatus: "running" });
      renderHook(() => useChatRecovery(props));

      expect(props.setAgentRunning).toHaveBeenCalledWith("task_execution:task-1", true);
    });

    it("should NOT set agent running when status is not running", () => {
      const props = makeProps({ agentRunStatus: "completed" });
      renderHook(() => useChatRecovery(props));

      // Effect 1 shouldn't fire (status !== running), but effect 2 should fire with false
      const trueCalls = props.setAgentRunning.mock.calls.filter(
        (call: [string, boolean]) => call[1] === true
      );
      expect(trueCalls).toHaveLength(0);
    });
  });

  describe("active-state hydration", () => {
    it("refreshes the canonical timeline when switching back to a visible generating conversation", () => {
      const props = makeProps({
        activeConversationId: "conv-active",
        isAgentRunning: true,
        isGenerating: true,
        agentRunStatus: "running",
        setStreamingContentBlocks: vi.fn(),
      });

      renderHook(() => useChatRecovery(props));

      expect(mockInvalidateQueries).toHaveBeenCalledWith({
        queryKey: ["chat", "conversations", "conv-active", "timeline"],
      });
    });

    it("hydrates project conversation active state after switching back to an in-flight Agents conversation", async () => {
      const setStreamingToolCalls = vi.fn();
      const setStreamingContentBlocks = vi.fn();
      const setStreamingTasks = vi.fn();
      mockGetConversationActiveState.mockResolvedValueOnce({
        is_active: true,
        tool_calls: [],
        streaming_tasks: [],
        partial_text: "Still working on the follow-up",
      });

      const props = makeProps({
        activeConversationId: "conversation-a",
        storeContextKey: "project:conversation-a",
        currentContextType: "project",
        currentContextId: "conversation-a",
        isAgentContext: false,
        selectedTaskId: undefined,
        effectiveStatus: undefined,
        setStreamingToolCalls,
        setStreamingContentBlocks,
        setStreamingTasks,
      });
      renderHook(() => useChatRecovery(props));

      await act(async () => {});

      expect(mockGetConversationActiveState).toHaveBeenCalledWith("conversation-a");
      expect(setStreamingContentBlocks).toHaveBeenCalledTimes(1);
      const updater = setStreamingContentBlocks.mock.calls[0][0] as (
        prev: StreamingContentBlock[]
      ) => StreamingContentBlock[];
      expect(updater([])).toEqual([
        { type: "text", text: "Still working on the follow-up" },
      ]);
    });

    it("hydrates partial text and active tool calls from active-state", async () => {
      const setStreamingToolCalls = vi.fn();
      const setStreamingContentBlocks = vi.fn();
      mockGetConversationActiveState.mockResolvedValueOnce({
        is_active: true,
        tool_calls: [
          {
            id: "toolu_read",
            name: "Read",
            arguments: { file_path: "src/main.ts" },
          },
        ],
        streaming_tasks: [],
        partial_text: "Inspecting the current implementation",
      });

      const props = makeProps({
        setStreamingToolCalls,
        setStreamingContentBlocks,
        setStreamingTasks: vi.fn(),
      });
      renderHook(() => useChatRecovery(props));

      await act(async () => {});

      expect(setStreamingToolCalls).toHaveBeenCalledTimes(1);
      const toolUpdater = setStreamingToolCalls.mock.calls[0][0] as (
        prev: ToolCall[]
      ) => ToolCall[];
      expect(toolUpdater([])).toEqual([
        {
          id: "toolu_read",
          name: "Read",
          arguments: { file_path: "src/main.ts" },
        },
      ]);

      expect(setStreamingContentBlocks).toHaveBeenCalledTimes(1);
      const blockUpdater = setStreamingContentBlocks.mock.calls[0][0] as (
        prev: StreamingContentBlock[]
      ) => StreamingContentBlock[];
      expect(blockUpdater([])).toEqual([
        { type: "text", text: "Inspecting the current implementation" },
        {
          type: "tool_use",
          toolCall: {
            id: "toolu_read",
            name: "Read",
            arguments: { file_path: "src/main.ts" },
          },
        },
      ]);
    });

    it("hydrates sparse absolute thinking segments without duplicating the persisted anchor", async () => {
      const setStreamingContentBlocks = vi.fn();
      const persistedStreamingContentBlocks = projectPersistedStreamingContentBlocks([{
        id: "message-thinking",
        sessionId: null,
        projectId: null,
        taskId: null,
        role: "assistant",
        content: "",
        metadata: null,
        parentMessageId: null,
        conversationId: "conv-abc",
        toolCalls: null,
        contentBlocks: [{ type: "thinking", text: "Reconsidering" }],
        sender: null,
        createdAt: "2026-07-30T00:00:00Z",
        runId: "run-active",
        timelineStatus: "streaming",
        timelineBlockIndex: 2,
      }], "run-active");
      mockGetConversationActiveState.mockResolvedValueOnce({
        is_active: true,
        tool_calls: [],
        streaming_tasks: [],
        partial_text: "",
        partial_thinking_segments: ["", "", "Reconsidering the recovery path"],
      });

      renderHook(() => useChatRecovery(makeProps({
        setStreamingContentBlocks,
        persistedStreamingContentBlocks,
      })));
      await act(async () => {});

      const updater = setStreamingContentBlocks.mock.calls[0][0] as (
        prev: StreamingContentBlock[]
      ) => StreamingContentBlock[];
      const reconciled = updater([
        { type: "thinking", text: "Reconsidering", blockIndex: 2 },
      ]);
      expect(reconciled.filter((block) => block.type === "thinking")).toHaveLength(1);
      expect(reconciled).toEqual([
        { type: "thinking", text: "Reconsidering the recovery path", blockIndex: 2 },
      ]);
    });

    it("keeps cache-only thinking groups separated by an indexed recovered tool call", async () => {
      const setStreamingContentBlocks = vi.fn();
      mockGetConversationActiveState.mockResolvedValueOnce({
        is_active: true,
        tool_calls: [{ id: "tool-between", name: "Read", arguments: {}, block_index: 1 }],
        streaming_tasks: [],
        partial_text: "",
        partial_thinking_segments: ["Before", "", "After"],
      });

      renderHook(() => useChatRecovery(makeProps({ setStreamingContentBlocks })));
      await act(async () => {});

      const updater = setStreamingContentBlocks.mock.calls[0][0] as (
        prev: StreamingContentBlock[],
      ) => StreamingContentBlock[];
      const reconciled = updater([]);
      expect(reconciled.map((block) => block.type)).toEqual([
        "thinking", "tool_use", "thinking",
      ]);
      expect(buildLiveTranscriptRows(reconciled, new Map()).map((row) => row.kind)).toEqual([
        "thinking_group", "tool_group", "thinking_group",
      ]);
    });

    it("hydrates delegated streaming task metadata from active-state", async () => {
      const setStreamingTasks = vi.fn();
      const setStreamingContentBlocks = vi.fn();
      mockGetConversationActiveState.mockResolvedValueOnce({
        is_active: true,
        tool_calls: [],
        streaming_tasks: [
          {
            tool_use_id: "toolu_delegate",
            description: "execution-reviewer",
            subagent_type: "delegated",
            model: "gpt-5.4",
            status: "completed",
            delegated_job_id: "job-123",
            delegated_session_id: "delegated-session-123",
            delegated_conversation_id: "conv-child-123",
            delegated_agent_run_id: "run-child-123",
            provider_harness: "codex",
            provider_session_id: "provider-session-123",
            upstream_provider: "openai",
            provider_profile: "prod",
            logical_model: "gpt-5.4",
            effective_model_id: "gpt-5.4-2026-04-01",
            logical_effort: "high",
            effective_effort: "high",
            approval_policy: "never",
            sandbox_mode: "danger-full-access",
            total_tokens: 120,
            total_tool_uses: 3,
            duration_ms: 4500,
            input_tokens: 10,
            output_tokens: 20,
            cache_creation_tokens: 30,
            cache_read_tokens: 40,
            estimated_usd: 0.12,
            text_output: "delegate done",
          },
        ],
        partial_text: "",
      });

      const props = makeProps({ setStreamingTasks, setStreamingContentBlocks });
      renderHook(() => useChatRecovery(props));

      await act(async () => {});

      expect(mockGetConversationActiveState).toHaveBeenCalledWith("conv-abc");
      expect(setStreamingTasks).toHaveBeenCalledTimes(1);
      const updater = setStreamingTasks.mock.calls[0][0] as (
        prev: Map<string, import("@/types/streaming-task").StreamingTask>
      ) => Map<string, import("@/types/streaming-task").StreamingTask>;
      const next = updater(new Map());
      const task = next.get("toolu_delegate");
      expect(task?.toolName).toBe("delegate_start");
      expect(task?.delegatedSessionId).toBe("delegated-session-123");
      expect(task?.providerHarness).toBe("codex");
      expect(task?.upstreamProvider).toBe("openai");
      expect(task?.effectiveModelId).toBe("gpt-5.4-2026-04-01");
      expect(task?.inputTokens).toBe(10);
      expect(task?.estimatedUsd).toBe(0.12);
      expect(task?.textOutput).toBe("delegate done");

      expect(setStreamingContentBlocks).toHaveBeenCalledTimes(1);
      const blockUpdater = setStreamingContentBlocks.mock.calls[0][0] as (
        prev: StreamingContentBlock[]
      ) => StreamingContentBlock[];
      expect(blockUpdater([])).toEqual([
        { type: "task", toolUseId: "toolu_delegate" },
      ]);
    });

    it("rejects a late active-state snapshot for a different parent run", async () => {
      const setStreamingTasks = vi.fn();
      mockGetConversationActiveState.mockResolvedValueOnce({
        is_active: true,
        runId: "run-old",
        tool_calls: [],
        streaming_tasks: [{
          tool_use_id: "delegate-job:old",
          status: "running",
          delegated_job_id: "job-old",
        }],
        partial_text: "stale",
      });

      renderHook(() => useChatRecovery(makeProps({
        activeAgentRunId: "run-current",
        setStreamingTasks,
        setStreamingContentBlocks: vi.fn(),
      })));
      await act(async () => {});

      expect(setStreamingTasks).not.toHaveBeenCalled();
    });

    it("hydrates provider and lifecycle aliases as one promoted delegation", async () => {
      const setStreamingTasks = vi.fn();
      const setStreamingToolCalls = vi.fn();
      const setStreamingContentBlocks = vi.fn();
      mockGetConversationActiveState.mockResolvedValueOnce({
        is_active: true,
        tool_calls: [{
          id: "provider-tool",
          name: "delegate_start",
          arguments: { title: "Trace stale Claude MCP collision handling" },
          result: { job_id: "job-1", status: "running" },
        }],
        streaming_tasks: [{
          tool_use_id: "delegate-job:job-1",
          description: "ralphx-general-explorer",
          subagent_type: "delegated",
          status: "running",
          delegated_job_id: "job-1",
          provider_harness: "codex",
          delegated_agent_run_id: "child-run-1",
        }],
        partial_text: "",
      });

      renderHook(() => useChatRecovery(makeProps({
        setStreamingTasks,
        setStreamingToolCalls,
        setStreamingContentBlocks,
      })));
      await act(async () => {});

      const taskUpdater = setStreamingTasks.mock.calls[0][0] as (
        prev: Map<string, import("@/types/streaming-task").StreamingTask>
      ) => Map<string, import("@/types/streaming-task").StreamingTask>;
      const toolUpdater = setStreamingToolCalls.mock.calls[0][0] as (
        prev: ToolCall[]
      ) => ToolCall[];
      const blockUpdater = setStreamingContentBlocks.mock.calls[0][0] as (
        prev: StreamingContentBlock[]
      ) => StreamingContentBlock[];

      const tasks = taskUpdater(new Map());
      expect([...tasks.keys()]).toEqual(["provider-tool"]);
      expect(tasks.get("provider-tool")).toMatchObject({
        description: "Trace stale Claude MCP collision handling",
        delegatedJobId: "job-1",
        providerHarness: "codex",
        delegatedAgentRunId: "child-run-1",
      });
      expect(toolUpdater([])).toEqual([]);
      expect(blockUpdater([])).toEqual([
        { type: "task", toolUseId: "provider-tool" },
      ]);
    });

    it("skips active-state hydration in history mode", () => {
      const props = makeProps({
        isHistoryMode: true,
        setStreamingTasks: vi.fn(),
      });

      renderHook(() => useChatRecovery(props));

      expect(mockGetConversationActiveState).not.toHaveBeenCalled();
    });

    it("does not apply an active-state response after the panel becomes hidden", async () => {
      let resolveActiveState: ((value: {
        is_active: boolean;
        tool_calls: unknown[];
        streaming_tasks: never[];
        partial_text: string;
      }) => void) | undefined;
      mockGetConversationActiveState.mockImplementationOnce(() => new Promise((resolve) => {
        resolveActiveState = resolve;
      }));
      const setStreamingContentBlocks = vi.fn();
      const props = makeProps({ setStreamingContentBlocks });
      const { rerender } = renderHook(
        (nextProps) => useChatRecovery(nextProps),
        { initialProps: props },
      );

      rerender({ ...props, isVisible: false });
      await act(async () => {
        resolveActiveState?.({
          is_active: true,
          tool_calls: [],
          streaming_tasks: [],
          partial_text: "stale hidden response",
        });
      });

      expect(setStreamingContentBlocks).not.toHaveBeenCalled();
    });
  });

  describe("stuck running state cleanup", () => {
    it("should clear running state when backend says completed", async () => {
      const props = makeProps({ agentRunStatus: "completed" });
      renderHook(() => useChatRecovery(props));

      // Flush the microtask from isAgentRunning Promise
      await act(async () => {});

      expect(props.setAgentRunning).toHaveBeenCalledWith("task_execution:task-1", false);
    });

    it("should NOT clear running state when agentRunStatus is undefined (loading)", () => {
      const props = makeProps({ agentRunStatus: undefined });
      renderHook(() => useChatRecovery(props));

      // Effect 2 should early-return when agentRunStatus === undefined
      const falseCalls = props.setAgentRunning.mock.calls.filter(
        (call: [string, boolean]) => call[1] === false
      );
      expect(falseCalls).toHaveLength(0);
    });

    it("should NOT clear when conversation is not in current context", () => {
      const props = makeProps({
        agentRunStatus: "completed",
        isConversationInCurrentContext: false,
      });
      renderHook(() => useChatRecovery(props));

      // Both effects should early-return
      expect(props.setAgentRunning).not.toHaveBeenCalled();
    });

    it("should NOT clear when no active conversation", () => {
      const props = makeProps({
        activeConversationId: null,
        agentRunStatus: "completed",
      });
      renderHook(() => useChatRecovery(props));

      // Effect 2 should early-return
      const falseCalls = props.setAgentRunning.mock.calls.filter(
        (call: [string, boolean]) => call[1] === false
      );
      expect(falseCalls).toHaveLength(0);
    });
  });

  describe("reconciliation poll (1.5s interval)", () => {
    beforeEach(() => {
      mockIsAgentRunning.mockClear();
    });

    it("polls selected conversation liveness even when UI state is idle", async () => {
      mockIsAgentRunning.mockResolvedValue(true);
      const props = makeProps({ isAgentRunning: false });
      renderHook(() => useChatRecovery(props));

      await act(async () => {});

      expect(mockIsAgentRunning).toHaveBeenCalledWith("task_execution", "task-1");
      expect(props.setAgentRunning).toHaveBeenCalledWith("task_execution:task-1", true);
    });

    it("never probes is_agent_running under remote over 20 seconds", async () => {
      useEnvironmentStore.setState({
        activeEnvironmentId: "env-remote",
        environments: [
          { id: "env-remote", name: "Remote", kind: "remote" },
        ],
      });
      const props = makeProps({
        agentRunStatus: "completed",
        isAgentRunning: true,
      });

      renderHook(() => useChatRecovery(props));
      await act(async () => {
        vi.advanceTimersByTime(20_000);
      });

      expect(mockIsAgentRunning).not.toHaveBeenCalled();
      expect(props.setAgentRunning).not.toHaveBeenCalledWith(
        "task_execution:task-1",
        false,
      );
    });

    it("does not poll selected conversation liveness when panel is hidden", async () => {
      const props = makeProps({ isAgentRunning: false, isVisible: false });
      renderHook(() => useChatRecovery(props));

      await act(async () => {
        vi.advanceTimersByTime(3000);
      });

      expect(mockIsAgentRunning).not.toHaveBeenCalled();
    });

    it("should poll is_agent_running every 1500ms when isAgentRunning is true", async () => {
      mockIsAgentRunning.mockResolvedValue(true);
      const props = makeProps({ isAgentRunning: true });
      renderHook(() => useChatRecovery(props));

      await act(async () => {});
      expect(mockIsAgentRunning).toHaveBeenCalledTimes(1);

      await act(async () => {
        vi.advanceTimersByTime(1500);
      });
      expect(mockIsAgentRunning).toHaveBeenCalledTimes(2);
      expect(mockIsAgentRunning).toHaveBeenCalledWith("task_execution", "task-1");

      await act(async () => {
        vi.advanceTimersByTime(1500);
      });
      expect(mockIsAgentRunning).toHaveBeenCalledTimes(3);
    });

    it("should clear stuck state when poll returns false", async () => {
      mockIsAgentRunning.mockResolvedValue(false);
      const props = makeProps({ isAgentRunning: true });
      renderHook(() => useChatRecovery(props));

      await act(async () => {});

      expect(props.setAgentRunning).toHaveBeenCalledWith("task_execution:task-1", false);
    });

    it("should NOT clear state when poll returns true (agent still running)", async () => {
      mockIsAgentRunning.mockResolvedValue(true);
      const props = makeProps({ isAgentRunning: true });
      renderHook(() => useChatRecovery(props));

      await act(async () => {});

      const falseCalls = props.setAgentRunning.mock.calls.filter(
        (call: [string, boolean]) => call[1] === false
      );
      expect(falseCalls).toHaveLength(0);
    });

    it("should clean up interval on unmount", async () => {
      mockIsAgentRunning.mockResolvedValue(true);
      const props = makeProps({ isAgentRunning: true });
      const { unmount } = renderHook(() => useChatRecovery(props));

      unmount();
      mockIsAgentRunning.mockClear();

      vi.advanceTimersByTime(3000);
      expect(mockIsAgentRunning).not.toHaveBeenCalled();
    });
  });

  describe("IPR process check (double-execution fix)", () => {
    beforeEach(() => {
      mockIsAgentRunning.mockClear();
    });

    it("should NOT clear running state when process is still alive (IPR returns true)", async () => {
      mockIsAgentRunning.mockResolvedValue(true);
      const props = makeProps({ agentRunStatus: "completed" });
      renderHook(() => useChatRecovery(props));

      // Flush the microtask from the isAgentRunning promise
      await act(async () => {});

      expect(mockIsAgentRunning).toHaveBeenCalledWith("task_execution", "task-1");
      // Process is alive → must NOT set false
      const falseCalls = props.setAgentRunning.mock.calls.filter(
        (call: [string, boolean]) => call[1] === false
      );
      expect(falseCalls).toHaveLength(0);
    });

    it("should clear running state when process is dead (IPR returns false)", async () => {
      mockIsAgentRunning.mockResolvedValue(false);
      const props = makeProps({ agentRunStatus: "completed" });
      renderHook(() => useChatRecovery(props));

      await act(async () => {});

      expect(mockIsAgentRunning).toHaveBeenCalledWith("task_execution", "task-1");
      expect(props.setAgentRunning).toHaveBeenCalledWith("task_execution:task-1", false);
    });

    it("should clear running state on IPR check error (fallback to DB truth)", async () => {
      mockIsAgentRunning.mockRejectedValue(new Error("IPR check failed"));
      const props = makeProps({ agentRunStatus: "completed" });
      renderHook(() => useChatRecovery(props));

      await act(async () => {});

      // Error in process check → fall back to DB truth (completed) → clear
      expect(props.setAgentRunning).toHaveBeenCalledWith("task_execution:task-1", false);
    });

    it("should use correct context type and id for IPR check", async () => {
      mockIsAgentRunning.mockResolvedValue(true);
      const props = makeProps({
        agentRunStatus: "completed",
        currentContextType: "merge" as ContextType,
        currentContextId: "task-merge-42",
        storeContextKey: "merge:task-merge-42",
      });
      renderHook(() => useChatRecovery(props));

      await act(async () => {});

      // Must check with the correct context type and id
      expect(mockIsAgentRunning).toHaveBeenCalledWith("merge", "task-merge-42");
    });
  });

  describe("isStreamingHydrated flag", () => {
    it("returns false before hydration completes", () => {
      let resolveActiveState!: (val: unknown) => void;
      mockGetConversationActiveState.mockReturnValueOnce(
        new Promise((resolve) => { resolveActiveState = resolve; })
      );
      const props = makeProps({
        setStreamingToolCalls: vi.fn(),
        setStreamingContentBlocks: vi.fn(),
        setStreamingTasks: vi.fn(),
      });
      const { result } = renderHook(() => useChatRecovery(props));

      expect(result.current.isStreamingHydrated).toBe(false);

      // cleanup: resolve to avoid hanging
      resolveActiveState({ is_active: false, tool_calls: [], streaming_tasks: [], partial_text: "" });
    });

    it("returns true after hydration completes", async () => {
      mockGetConversationActiveState.mockResolvedValueOnce({
        is_active: true,
        tool_calls: [],
        streaming_tasks: [],
        partial_text: "some text",
      });
      const props = makeProps({
        setStreamingToolCalls: vi.fn(),
        setStreamingContentBlocks: vi.fn(),
        setStreamingTasks: vi.fn(),
      });
      const { result } = renderHook(() => useChatRecovery(props));

      await act(async () => {});

      expect(result.current.isStreamingHydrated).toBe(true);
    });

    it("returns true immediately when hydration is not needed (history mode)", () => {
      const props = makeProps({ isHistoryMode: true });
      const { result } = renderHook(() => useChatRecovery(props));

      expect(result.current.isStreamingHydrated).toBe(true);
    });

    it("resets to false on conversation switch then completes to true", async () => {
      mockGetConversationActiveState.mockResolvedValue({
        is_active: false,
        tool_calls: [],
        streaming_tasks: [],
        partial_text: "",
      });
      const props = makeProps({
        activeConversationId: "conv-1",
        setStreamingToolCalls: vi.fn(),
        setStreamingContentBlocks: vi.fn(),
        setStreamingTasks: vi.fn(),
      });

      const { result, rerender } = renderHook(
        (p: DefaultProps) => useChatRecovery(p),
        { initialProps: props },
      );

      await act(async () => {});
      expect(result.current.isStreamingHydrated).toBe(true);

      // Switch conversation
      const newProps = { ...props, activeConversationId: "conv-2" };
      rerender(newProps);

      expect(result.current.isStreamingHydrated).toBe(false);

      await act(async () => {});
      expect(result.current.isStreamingHydrated).toBe(true);
    });

    it("returns true when hydration fetch fails", async () => {
      mockGetConversationActiveState.mockRejectedValueOnce(new Error("network"));
      const props = makeProps({
        setStreamingToolCalls: vi.fn(),
        setStreamingContentBlocks: vi.fn(),
        setStreamingTasks: vi.fn(),
      });
      const { result } = renderHook(() => useChatRecovery(props));

      await act(async () => {});

      expect(result.current.isStreamingHydrated).toBe(true);
    });
  });

  describe("visibilitychange fast path", () => {
    beforeEach(() => {
      mockIsAgentRunning.mockClear();
    });

    it("should attach listener for selected conversation even when UI state is idle", () => {
      const addEventSpy = vi.spyOn(document, "addEventListener");
      const props = makeProps({ isAgentRunning: false });
      renderHook(() => useChatRecovery(props));

      const visibilityCalls = addEventSpy.mock.calls.filter(
        ([event]) => event === "visibilitychange"
      );
      expect(visibilityCalls.length).toBeGreaterThan(0);
      addEventSpy.mockRestore();
    });

    it("should reconcile immediately when app becomes visible and agent running", async () => {
      mockIsAgentRunning.mockResolvedValue(false);
      const props = makeProps({ isAgentRunning: true });
      renderHook(() => useChatRecovery(props));

      await act(async () => {
        Object.defineProperty(document, "visibilityState", {
          value: "visible",
          writable: true,
          configurable: true,
        });
        document.dispatchEvent(new Event("visibilitychange"));
      });

      expect(mockIsAgentRunning).toHaveBeenCalledWith("task_execution", "task-1");
      expect(props.setAgentRunning).toHaveBeenCalledWith("task_execution:task-1", false);
    });

    it("should rehydrate idle state when app becomes visible and agent is still running", async () => {
      mockIsAgentRunning.mockResolvedValue(true);
      const props = makeProps({ isAgentRunning: false });
      renderHook(() => useChatRecovery(props));

      await act(async () => {});
      mockIsAgentRunning.mockClear();
      props.setAgentRunning.mockClear();

      await act(async () => {
        Object.defineProperty(document, "visibilityState", {
          value: "visible",
          writable: true,
          configurable: true,
        });
        document.dispatchEvent(new Event("visibilitychange"));
      });

      expect(mockIsAgentRunning).toHaveBeenCalledWith("task_execution", "task-1");
      expect(props.setAgentRunning).toHaveBeenCalledWith("task_execution:task-1", true);
    });

    it("should remove listener on unmount", () => {
      mockIsAgentRunning.mockResolvedValue(true);
      const removeEventSpy = vi.spyOn(document, "removeEventListener");
      const props = makeProps({ isAgentRunning: true });
      const { unmount } = renderHook(() => useChatRecovery(props));

      unmount();

      const visibilityCalls = removeEventSpy.mock.calls.filter(
        ([event]) => event === "visibilitychange"
      );
      expect(visibilityCalls.length).toBeGreaterThan(0);
      removeEventSpy.mockRestore();
    });
  });
});
