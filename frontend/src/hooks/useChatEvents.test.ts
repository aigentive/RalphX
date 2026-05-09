/**
 * useChatEvents hook tests
 *
 * Tests event subscription behavior: tool call accumulation, subagent routing,
 * streaming text, lifecycle clearing, error handling, and context filtering.
 */

import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { renderHook, act } from "@testing-library/react";
import type { ContextType } from "@/types/chat-conversation";
import type { ToolCall } from "@/components/Chat/ToolCallIndicator";
import type { StreamingTask, StreamingContentBlock } from "@/types/streaming-task";

// ============================================================================
// Mock infrastructure
// ============================================================================

// Capture subscriptions so tests can fire events manually
const subscriptions = new Map<string, ((...args: unknown[]) => void)[]>();

function fireEvent<T>(event: string, payload: T) {
  const handlers = subscriptions.get(event);
  if (handlers) {
    for (const handler of handlers) {
      handler(payload);
    }
  }
}

const mockInvalidateQueries = vi.fn();
let mockQueryData: { messages: Array<{ id: string }> } | undefined = undefined;
const mockGetQueryData = vi.fn(() => mockQueryData);
const cacheSubscribers: Array<(event: { type: string; query: { queryKey: unknown[] } }) => void> = [];
function fireCacheEvent(event: { type: string; query: { queryKey: unknown[] } }) {
  for (const fn of cacheSubscribers) fn(event);
}

vi.mock("@/providers/EventProvider", () => ({
  useEventBus: () => ({
    subscribe: (event: string, handler: (...args: unknown[]) => void) => {
      if (!subscriptions.has(event)) subscriptions.set(event, []);
      subscriptions.get(event)!.push(handler);
      return () => {
        const handlers = subscriptions.get(event);
        if (handlers) {
          const idx = handlers.indexOf(handler);
          if (idx >= 0) handlers.splice(idx, 1);
        }
      };
    },
  }),
}));

vi.mock("@tanstack/react-query", () => ({
  useQueryClient: () => ({
    invalidateQueries: mockInvalidateQueries,
    getQueryData: mockGetQueryData,
    getQueryCache: () => ({
      subscribe: (fn: (event: { type: string; query: { queryKey: unknown[] } }) => void) => {
        cacheSubscribers.push(fn);
        return () => {
          const idx = cacheSubscribers.indexOf(fn);
          if (idx >= 0) cacheSubscribers.splice(idx, 1);
        };
      },
    }),
  }),
}));

vi.mock("@/hooks/useChat", () => ({
  chatKeys: {
    conversation: (id: string) => ["chat", "conversations", id],
    conversationHistory: (id: string) => ["chat", "conversations", id, "history"],
  },
  getCachedConversationMessages: () => mockQueryData?.messages ?? [],
  invalidateConversationDataQueries: (_queryClient: unknown, conversationId: string) => {
    mockInvalidateQueries({ queryKey: ["chat", "conversations", conversationId] });
    mockInvalidateQueries({ queryKey: ["chat", "conversations", conversationId, "history"] });
  },
}));

// Dynamic mock for chat-context-registry — tests override via mockContextConfig
let mockContextConfig: {
  supportsStreamingText: boolean;
  supportsSubagentTasks: boolean;
  supportsDiffViews: boolean;
} | null = null;

vi.mock("@/lib/chat-context-registry", () => ({
  getContextConfig: () => mockContextConfig,
}));

// ============================================================================
// Import hook under test (after mocks)
// ============================================================================

import { useChatEvents } from "./useChatEvents";
import { useChatStore } from "@/stores/chatStore";

// ============================================================================
// Helpers
// ============================================================================

const CONV_ID = "conv-abc";
const CTX_ID = "ctx-123";

interface DefaultProps {
  activeConversationId: string | null;
  contextId: string | null;
  contextType: ContextType | null;
  setStreamingToolCalls: ReturnType<typeof vi.fn>;
  setStreamingContentBlocks: ReturnType<typeof vi.fn>;
  setStreamingTasks: ReturnType<typeof vi.fn>;
  setIsFinalizing: ReturnType<typeof vi.fn>;
  storeKey?: string;
}

function makeProps(overrides?: Partial<DefaultProps>): DefaultProps {
  return {
    activeConversationId: CONV_ID,
    contextId: CTX_ID,
    contextType: "task_execution" as ContextType,
    setStreamingToolCalls: vi.fn(),
    setStreamingContentBlocks: vi.fn(),
    setStreamingTasks: vi.fn(),
    setIsFinalizing: vi.fn(),
    storeKey: undefined,
    ...overrides,
  };
}

/**
 * Renders the hook and clears the initial mount calls on all setters.
 * The effect fires on mount and clears streaming state (3 calls + setIsFinalizing).
 * This helper lets tests focus on event-driven behavior without counting mount effects.
 */
function renderAndClear(props: DefaultProps) {
  const result = renderHook(() => useChatEvents(props));
  props.setStreamingToolCalls.mockClear();
  props.setStreamingContentBlocks.mockClear();
  props.setStreamingTasks.mockClear();
  props.setIsFinalizing.mockClear();
  return result;
}

/**
 * Helper to execute a state updater function captured by vi.fn().
 * setStreamingX mocks are called with updater functions (prev => next).
 * This executes the updater with a given prev value and returns the result.
 */
function executeUpdater<T>(mockFn: ReturnType<typeof vi.fn>, prev: T, callIndex = 0): T {
  const call = mockFn.mock.calls[callIndex];
  if (!call) throw new Error(`No call at index ${callIndex}`);
  const updater = call[0];
  if (typeof updater === "function") {
    return updater(prev) as T;
  }
  return updater as T;
}

// ============================================================================
// Tests
// ============================================================================

describe("useChatEvents", () => {
  beforeEach(() => {
    subscriptions.clear();
    mockInvalidateQueries.mockClear();
    mockGetQueryData.mockClear();
    mockQueryData = undefined;
    cacheSubscribers.length = 0;
    useChatStore.setState({
      toolCallStartTimes: {},
      lastToolCallCompletionTimestamp: {},
      toolCallCompletionTimestamps: {},
      lastAgentEventTimestamp: {},
    });
    // Default: full feature flags (task_execution context)
    mockContextConfig = {
      supportsStreamingText: true,
      supportsSubagentTasks: true,
      supportsDiffViews: true,
    };
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  // --------------------------------------------------------------------------
  // 1. Tool call accumulation
  // --------------------------------------------------------------------------
  describe("tool call accumulation", () => {
    it("should accumulate tool calls via setStreamingToolCalls on agent:tool_call", () => {
      const props = makeProps();
      renderAndClear(props);

      act(() => {
        fireEvent("agent:tool_call", {
          tool_name: "Read",
          tool_id: "toolu_001",
          arguments: { file_path: "/src/main.ts" },
          conversation_id: CONV_ID,
          context_id: CTX_ID,
        });
      });

      expect(props.setStreamingToolCalls).toHaveBeenCalledTimes(1);

      // Execute the updater to verify the appended tool call
      const result = executeUpdater<ToolCall[]>(props.setStreamingToolCalls, []);
      expect(result).toHaveLength(1);
      expect(result[0]).toMatchObject({
        id: "toolu_001",
        name: "Read",
        arguments: { file_path: "/src/main.ts" },
      });
    });

    it("should update existing tool call when same tool_id arrives with result", () => {
      const props = makeProps();
      renderAndClear(props);

      // First event: tool call started
      act(() => {
        fireEvent("agent:tool_call", {
          tool_name: "Read",
          tool_id: "toolu_002",
          arguments: { file_path: "/src/main.ts" },
          conversation_id: CONV_ID,
          context_id: CTX_ID,
        });
      });

      // Second event: same tool_id with result
      act(() => {
        fireEvent("agent:tool_call", {
          tool_name: "Read",
          tool_id: "toolu_002",
          arguments: { file_path: "/src/main.ts" },
          result: "file content here",
          conversation_id: CONV_ID,
          context_id: CTX_ID,
        });
      });

      expect(props.setStreamingToolCalls).toHaveBeenCalledTimes(2);

      // The second call should update the existing entry, not append
      const existing: ToolCall[] = [
        { id: "toolu_002", name: "Read", arguments: { file_path: "/src/main.ts" } },
      ];
      const result = executeUpdater<ToolCall[]>(props.setStreamingToolCalls, existing, 1);
      expect(result).toHaveLength(1);
      expect(result[0]!.result).toBe("file content here");
    });

    it("should update an existing no-id tool call by stable name and arguments", () => {
      vi.useFakeTimers();
      vi.setSystemTime(new Date("2026-04-25T13:00:00Z"));
      const props = makeProps();
      renderAndClear(props);

      act(() => {
        fireEvent("agent:tool_call", {
          tool_name: "ralphx::fs_read_file",
          arguments: { path: "frontend/src/App.tsx" },
          conversation_id: CONV_ID,
          context_id: CTX_ID,
        });
      });

      const startedTools = executeUpdater<ToolCall[]>(props.setStreamingToolCalls, []);
      expect(startedTools).toHaveLength(1);

      vi.setSystemTime(new Date("2026-04-25T13:00:05Z"));

      act(() => {
        fireEvent("agent:tool_call", {
          tool_name: "ralphx::fs_read_file",
          arguments: { path: "frontend/src/App.tsx" },
          result: [{ type: "text", text: "file contents" }],
          conversation_id: CONV_ID,
          context_id: CTX_ID,
        });
      });

      const completedTools = executeUpdater<ToolCall[]>(
        props.setStreamingToolCalls,
        startedTools,
        1,
      );
      expect(completedTools).toHaveLength(1);
      expect(completedTools[0]!.id).toBe(startedTools[0]!.id);
      expect(completedTools[0]!.result).toEqual([{ type: "text", text: "file contents" }]);

      const startedBlocks = executeUpdater<StreamingContentBlock[]>(
        props.setStreamingContentBlocks,
        [],
      );
      const completedBlocks = executeUpdater<StreamingContentBlock[]>(
        props.setStreamingContentBlocks,
        startedBlocks,
        1,
      );
      expect(completedBlocks).toHaveLength(1);
      expect(completedBlocks[0]).toMatchObject({
        type: "tool_use",
        toolCall: {
          id: startedTools[0]!.id,
          result: [{ type: "text", text: "file contents" }],
        },
      });
    });

    it("clears active tool timing when a direct tool call completes on the same id", () => {
      const props = makeProps({ storeKey: "task_execution:ctx-123" });
      renderAndClear(props);

      act(() => {
        fireEvent("agent:tool_call", {
          tool_name: "bash",
          tool_id: "toolu_codex_001",
          arguments: { command: "/bin/zsh -lc pwd" },
          conversation_id: CONV_ID,
          context_id: CTX_ID,
        });
      });

      const startedAt = useChatStore.getState().toolCallStartTimes["task_execution:ctx-123"]?.toolu_codex_001;
      expect(typeof startedAt).toBe("number");

      act(() => {
        fireEvent("agent:tool_call", {
          tool_name: "bash",
          tool_id: "toolu_codex_001",
          arguments: { command: "/bin/zsh -lc pwd" },
          result: { text: "/Users/example/Code/ralphx\n", exit_code: 0, status: "completed" },
          conversation_id: CONV_ID,
          context_id: CTX_ID,
        });
      });

      const store = useChatStore.getState();
      expect(store.toolCallStartTimes["task_execution:ctx-123"]?.toolu_codex_001).toBeUndefined();
      expect(store.toolCallCompletionTimestamps["task_execution:ctx-123"]?.toolu_codex_001).toEqual(expect.any(Number));
      expect(store.lastToolCallCompletionTimestamp["task_execution:ctx-123"]).toEqual(expect.any(Number));
    });

    it("should update existing tool calls with result payload when result:toolu events arrive", () => {
      const props = makeProps();
      renderAndClear(props);

      // First: simulate an existing tool call in the streaming state
      const existingToolCalls: ToolCall[] = [
        { id: "toolu_001", name: "Read", arguments: { file_path: "/src/main.ts" } },
      ];

      // Fire a result event for that tool call
      act(() => {
        fireEvent("agent:tool_call", {
          tool_name: "result:toolu_001",
          tool_id: "toolu_001",
          arguments: {},
          result: "file content here",
          conversation_id: CONV_ID,
          context_id: CTX_ID,
        });
      });

      // Should update streamingToolCalls with the result
      expect(props.setStreamingToolCalls).toHaveBeenCalledTimes(1);
      const updatedCalls = executeUpdater<ToolCall[]>(props.setStreamingToolCalls, existingToolCalls);
      expect(updatedCalls).toHaveLength(1);
      expect(updatedCalls[0]!.result).toBe("file content here");

      // Should also update streamingContentBlocks
      expect(props.setStreamingContentBlocks).toHaveBeenCalledTimes(1);
      const existingBlocks: StreamingContentBlock[] = [
        { type: "tool_use", toolCall: { id: "toolu_001", name: "Read", arguments: { file_path: "/src/main.ts" } } },
      ];
      const updatedBlocks = executeUpdater<StreamingContentBlock[]>(props.setStreamingContentBlocks, existingBlocks);
      expect(updatedBlocks).toHaveLength(1);
      expect(updatedBlocks[0]!.type).toBe("tool_use");
      expect((updatedBlocks[0] as { type: "tool_use"; toolCall: ToolCall }).toolCall.result).toBe("file content here");
    });

    it("should update child tool calls in streamingTasks when result:toolu events arrive", () => {
      const props = makeProps();
      renderAndClear(props);

      // Setup: create a parent task with child tool calls
      const parentId = "toolu_parent";
      const parentTask: StreamingTask = {
        toolUseId: parentId,
        toolName: "Task",
        description: "Test task",
        subagentType: "Bash",
        model: "sonnet",
        status: "running",
        startedAt: Date.now(),
        childToolCalls: [
          { id: "toolu_child_001", name: "Read", arguments: { file_path: "/src/test.ts" } },
        ],
      };
      const prevMap = new Map<string, StreamingTask>([[parentId, parentTask]]);

      // Fire a result event for the child tool call
      act(() => {
        fireEvent("agent:tool_call", {
          tool_name: "result:toolu_child_001",
          tool_id: "toolu_child_001",
          arguments: {},
          result: "child tool result",
          conversation_id: CONV_ID,
          context_id: CTX_ID,
        });
      });

      // Should update the child tool call in streamingTasks
      expect(props.setStreamingTasks).toHaveBeenCalledTimes(1);
      const updatedMap = executeUpdater<Map<string, StreamingTask>>(props.setStreamingTasks, prevMap);
      const updatedTask = updatedMap.get(parentId)!;
      expect(updatedTask.childToolCalls).toHaveLength(1);
      expect(updatedTask.childToolCalls[0]!.result).toBe("child tool result");
    });

    it("should not modify tool calls when result event has no matching id", () => {
      const props = makeProps();
      renderAndClear(props);

      // Fire a result event for a tool call that doesn't exist
      act(() => {
        fireEvent("agent:tool_call", {
          tool_name: "result:toolu_nonexistent",
          tool_id: "toolu_nonexistent",
          arguments: {},
          result: "some result",
          conversation_id: CONV_ID,
          context_id: CTX_ID,
        });
      });

      // All setters should be called
      expect(props.setStreamingToolCalls).toHaveBeenCalledTimes(1);
      expect(props.setStreamingContentBlocks).toHaveBeenCalledTimes(1);
      expect(props.setStreamingTasks).toHaveBeenCalledTimes(1);

      // Verify tool calls are unchanged (no result added to unrelated entries)
      const existingCalls: ToolCall[] = [{ id: "other_id", name: "Read", arguments: {} }];
      const callsResult = executeUpdater<ToolCall[]>(props.setStreamingToolCalls, existingCalls);
      expect(callsResult).toHaveLength(1);
      expect(callsResult[0]!.result).toBeUndefined(); // No result added

      const existingBlocks: StreamingContentBlock[] = [{ type: "tool_use", toolCall: { id: "other_id", name: "Read", arguments: {} } }];
      const blocksResult = executeUpdater<StreamingContentBlock[]>(props.setStreamingContentBlocks, existingBlocks);
      expect(blocksResult).toHaveLength(1);
      expect((blocksResult[0] as { type: "tool_use"; toolCall: ToolCall }).toolCall.result).toBeUndefined();

      // streamingTasks returns same reference when no child matches
      const existingTasks = new Map([["t1", { toolUseId: "t1", toolName: "Task", description: "", subagentType: "", model: "", status: "running" as const, startedAt: 0, childToolCalls: [] }]]);
      const tasksResult = executeUpdater<Map<string, StreamingTask>>(props.setStreamingTasks, existingTasks);
      expect(tasksResult).toBe(existingTasks); // Same reference since no child matched
    });
  });

  // --------------------------------------------------------------------------
  // 2. Child tool call routing to subagent tasks
  // --------------------------------------------------------------------------
  describe("child tool call routing", () => {
    it("should route tool calls with parent_tool_use_id to setStreamingTasks", () => {
      const props = makeProps();
      renderAndClear(props);

      const parentId = "toolu_parent";

      act(() => {
        fireEvent("agent:tool_call", {
          tool_name: "Bash",
          tool_id: "toolu_child_001",
          arguments: { command: "ls" },
          conversation_id: CONV_ID,
          context_id: CTX_ID,
          parent_tool_use_id: parentId,
        });
      });

      // Should route to setStreamingTasks, not setStreamingToolCalls
      expect(props.setStreamingTasks).toHaveBeenCalledTimes(1);
      expect(props.setStreamingToolCalls).not.toHaveBeenCalled();

      // Execute the updater with a parent task already in the map
      const parentTask: StreamingTask = {
        toolUseId: parentId,
        toolName: "Task",
        description: "Test task",
        subagentType: "Bash",
        model: "sonnet",
        status: "running",
        startedAt: Date.now(),
        childToolCalls: [],
      };
      const prevMap = new Map<string, StreamingTask>([[parentId, parentTask]]);
      const nextMap = executeUpdater<Map<string, StreamingTask>>(props.setStreamingTasks, prevMap);

      const updated = nextMap.get(parentId)!;
      expect(updated.childToolCalls).toHaveLength(1);
      expect(updated.childToolCalls[0]).toMatchObject({
        id: "toolu_child_001",
        name: "Bash",
      });
    });

    it("should NOT route to tasks when supportsSubagentTasks is false", () => {
      mockContextConfig = {
        supportsStreamingText: false,
        supportsSubagentTasks: false,
        supportsDiffViews: false,
      };

      const props = makeProps({ contextType: "task" as ContextType });
      renderAndClear(props);

      act(() => {
        fireEvent("agent:tool_call", {
          tool_name: "Read",
          tool_id: "toolu_010",
          arguments: {},
          conversation_id: CONV_ID,
          context_id: CTX_ID,
          parent_tool_use_id: "toolu_parent",
        });
      });

      // Should fall through to setStreamingToolCalls since subagent routing is off
      expect(props.setStreamingToolCalls).toHaveBeenCalledTimes(1);
      expect(props.setStreamingTasks).not.toHaveBeenCalled();
    });
  });

  // --------------------------------------------------------------------------
  // 3. Streaming text (via content blocks)
  // --------------------------------------------------------------------------
  describe("streaming text", () => {
    it("should append text chunks when the backend marks them as continuations", () => {
      const props = makeProps();
      renderAndClear(props);

      act(() => {
        fireEvent("agent:chunk", {
          text: "Hello ",
          conversation_id: CONV_ID,
          context_id: CTX_ID,
          append_to_previous: true,
        });
      });

      expect(props.setStreamingContentBlocks).toHaveBeenCalledTimes(1);

      // Execute the updater — appends to last text block
      const result = executeUpdater<StreamingContentBlock[]>(
        props.setStreamingContentBlocks,
        [{ type: "text", text: "Previous: " }],
      );
      expect(result).toHaveLength(1);
      expect(result[0]).toEqual({ type: "text", text: "Previous: Hello " });
    });

    it("should start a new text block when the backend marks a chunk as a new block", () => {
      const props = makeProps();
      renderAndClear(props);

      act(() => {
        fireEvent("agent:chunk", {
          text: "Second block",
          conversation_id: CONV_ID,
          context_id: CTX_ID,
          append_to_previous: false,
        });
      });

      const result = executeUpdater<StreamingContentBlock[]>(
        props.setStreamingContentBlocks,
        [{ type: "text", text: "First block" }],
      );
      expect(result).toEqual([
        { type: "text", text: "First block" },
        { type: "text", text: "Second block" },
      ]);
    });
  });

  // --------------------------------------------------------------------------
  // 4. Message created clears streaming state and sets isFinalizing
  // --------------------------------------------------------------------------
  describe("agent:message_created", () => {
    it("should clear streaming content blocks, tool calls, and tasks on assistant message", () => {
      const props = makeProps();
      renderAndClear(props);

      act(() => {
        fireEvent("agent:message_created", {
          conversation_id: CONV_ID,
          context_id: CTX_ID,
          role: "assistant",
        });
      });

      // All three use functional updaters
      expect(props.setStreamingContentBlocks).toHaveBeenCalledTimes(1);
      expect(props.setStreamingToolCalls).toHaveBeenCalledTimes(1);
      expect(props.setStreamingTasks).toHaveBeenCalledTimes(1);

      // Verify functional updater returns empty when prev has items
      const contentResult = executeUpdater<StreamingContentBlock[]>(
        props.setStreamingContentBlocks,
        [{ type: "text", text: "some text" }],
      );
      expect(contentResult).toEqual([]);

      const toolCallResult = executeUpdater<ToolCall[]>(props.setStreamingToolCalls, [
        { id: "tc1", name: "Read", arguments: {} },
      ]);
      expect(toolCallResult).toEqual([]);

      const taskResult = executeUpdater<Map<string, StreamingTask>>(
        props.setStreamingTasks,
        new Map([["t1", { toolUseId: "t1", toolName: "Task", description: "", subagentType: "", model: "", status: "running" as const, startedAt: 0, childToolCalls: [] }]]),
      );
      expect(taskResult.size).toBe(0);
    });

    it("should set isFinalizing=true on assistant message_created (same batch as clearing state)", () => {
      const props = makeProps();
      renderAndClear(props);

      act(() => {
        fireEvent("agent:message_created", {
          conversation_id: CONV_ID,
          context_id: CTX_ID,
          role: "assistant",
        });
      });

      // setIsFinalizing(true) should be called once immediately
      expect(props.setIsFinalizing).toHaveBeenCalledTimes(1);
      expect(props.setIsFinalizing).toHaveBeenCalledWith(true);
    });

    it("should set isFinalizing=true on orchestrator message_created (same batch as clearing state)", () => {
      const props = makeProps();
      renderAndClear(props);

      act(() => {
        fireEvent("agent:message_created", {
          conversation_id: CONV_ID,
          context_id: CTX_ID,
          role: "orchestrator",
        });
      });

      expect(props.setStreamingContentBlocks).toHaveBeenCalledTimes(1);
      expect(props.setStreamingToolCalls).toHaveBeenCalledTimes(1);
      expect(props.setStreamingTasks).toHaveBeenCalledTimes(1);
      expect(props.setIsFinalizing).toHaveBeenCalledTimes(1);
      expect(props.setIsFinalizing).toHaveBeenCalledWith(true);
    });

    it("should set isFinalizing=false after 3s safety timeout when no message_id provided", () => {
      vi.useFakeTimers();
      const props = makeProps();
      renderAndClear(props);

      act(() => {
        fireEvent("agent:message_created", {
          conversation_id: CONV_ID,
          context_id: CTX_ID,
          role: "assistant",
          // No message_id — falls back to safety timeout only
        });
      });

      // Initially called with true
      expect(props.setIsFinalizing).toHaveBeenCalledTimes(1);
      expect(props.setIsFinalizing).toHaveBeenLastCalledWith(true);

      // Advance 2999ms — timeout has NOT yet fired
      act(() => {
        vi.advanceTimersByTime(2999);
      });
      expect(props.setIsFinalizing).toHaveBeenCalledTimes(1);

      // Advance 1ms more — 3s total, safety timeout fires
      act(() => {
        vi.advanceTimersByTime(1);
      });

      // Now called with false (the safety timeout cleared it)
      expect(props.setIsFinalizing).toHaveBeenCalledTimes(2);
      expect(props.setIsFinalizing).toHaveBeenLastCalledWith(false);
    });

    it("should clear isFinalizing when query cache returns data containing the message_id", () => {
      const props = makeProps();
      renderAndClear(props);

      // No initial query data
      mockQueryData = undefined;

      act(() => {
        fireEvent("agent:message_created", {
          conversation_id: CONV_ID,
          context_id: CTX_ID,
          role: "assistant",
          message_id: "msg-assistant-new",
        });
      });

      // isFinalizing set to true; cache subscriber registered
      expect(props.setIsFinalizing).toHaveBeenCalledTimes(1);
      expect(props.setIsFinalizing).toHaveBeenCalledWith(true);
      expect(cacheSubscribers).toHaveLength(1);

      // Simulate query refetch completing with the new assistant message
      mockQueryData = { messages: [{ id: "msg-assistant-new" }] };

      act(() => {
        fireCacheEvent({
          type: "updated",
          query: { queryKey: ["chat", "conversations", CONV_ID] },
        });
      });

      // isFinalizing cleared (no timers needed)
      expect(props.setIsFinalizing).toHaveBeenCalledTimes(2);
      expect(props.setIsFinalizing).toHaveBeenLastCalledWith(false);
      // Cache subscriber unregistered
      expect(cacheSubscribers).toHaveLength(0);
    });

    it("should ignore cache events for a different conversation_id", () => {
      const props = makeProps();
      renderAndClear(props);

      mockQueryData = undefined;

      act(() => {
        fireEvent("agent:message_created", {
          conversation_id: CONV_ID,
          context_id: CTX_ID,
          role: "assistant",
          message_id: "msg-xyz",
        });
      });

      expect(props.setIsFinalizing).toHaveBeenCalledTimes(1);

      // Fire cache event for a different conversation
      act(() => {
        fireCacheEvent({
          type: "updated",
          query: { queryKey: ["chat", "conversations", "other-conv-id"] },
        });
      });

      // isFinalizing should NOT be cleared yet
      expect(props.setIsFinalizing).toHaveBeenCalledTimes(1);
    });

    it("should ignore cache events with type other than 'updated'", () => {
      vi.useFakeTimers();
      const props = makeProps();
      renderAndClear(props);

      // No initial query data — so race guard doesn't trigger, cache subscriber is set up
      mockQueryData = undefined;

      act(() => {
        fireEvent("agent:message_created", {
          conversation_id: CONV_ID,
          context_id: CTX_ID,
          role: "assistant",
          message_id: "msg-xyz",
        });
      });

      expect(props.setIsFinalizing).toHaveBeenCalledTimes(1);
      expect(props.setIsFinalizing).toHaveBeenCalledWith(true);

      // Populate the data, but fire a non-'updated' event — should be ignored
      mockQueryData = { messages: [{ id: "msg-xyz" }] };
      act(() => {
        fireCacheEvent({
          type: "added",
          query: { queryKey: ["chat", "conversations", CONV_ID] },
        });
      });

      // isFinalizing should NOT be cleared by a non-'updated' event
      expect(props.setIsFinalizing).toHaveBeenCalledTimes(1);
    });

    it("should clear isFinalizing immediately via race guard if query already has the message", () => {
      const props = makeProps();
      renderAndClear(props);

      // Data is already present before the event fires (race condition)
      mockQueryData = { messages: [{ id: "msg-already-there" }] };

      act(() => {
        fireEvent("agent:message_created", {
          conversation_id: CONV_ID,
          context_id: CTX_ID,
          role: "assistant",
          message_id: "msg-already-there",
        });
      });

      // setIsFinalizing(true) then setIsFinalizing(false) in the same act()
      expect(props.setIsFinalizing).toHaveBeenCalledTimes(2);
      expect(props.setIsFinalizing).toHaveBeenNthCalledWith(1, true);
      expect(props.setIsFinalizing).toHaveBeenNthCalledWith(2, false);
      // No cache subscriber needed since race guard caught it
      expect(cacheSubscribers).toHaveLength(0);
    });

    it("should still set 3s safety timeout even when message_id is present", () => {
      vi.useFakeTimers();
      const props = makeProps();
      renderAndClear(props);

      mockQueryData = undefined; // Query never returns the message

      act(() => {
        fireEvent("agent:message_created", {
          conversation_id: CONV_ID,
          context_id: CTX_ID,
          role: "assistant",
          message_id: "msg-never-arrives",
        });
      });

      expect(props.setIsFinalizing).toHaveBeenCalledTimes(1);
      expect(props.setIsFinalizing).toHaveBeenCalledWith(true);

      // Advance 3s — safety timeout fires since query never returned the message
      act(() => {
        vi.advanceTimersByTime(3000);
      });

      expect(props.setIsFinalizing).toHaveBeenCalledTimes(2);
      expect(props.setIsFinalizing).toHaveBeenLastCalledWith(false);
    });

    it("should NOT set isFinalizing on user message", () => {
      const props = makeProps();
      renderAndClear(props);

      act(() => {
        fireEvent("agent:message_created", {
          conversation_id: CONV_ID,
          context_id: CTX_ID,
          role: "user",
        });
      });

      // User messages should not trigger clearing of streaming state or finalizing
      expect(props.setStreamingContentBlocks).not.toHaveBeenCalled();
      expect(props.setStreamingToolCalls).not.toHaveBeenCalled();
      expect(props.setStreamingTasks).not.toHaveBeenCalled();
      expect(props.setIsFinalizing).not.toHaveBeenCalled();
      expect(mockInvalidateQueries).toHaveBeenCalledWith({
        queryKey: ["chat", "conversation-stats", CONV_ID],
      });
    });

    it("should NOT clear streaming state on user message", () => {
      const props = makeProps();
      renderAndClear(props);

      act(() => {
        fireEvent("agent:message_created", {
          conversation_id: CONV_ID,
          context_id: CTX_ID,
          role: "user",
        });
      });

      // User messages should not trigger clearing of streaming state
      expect(props.setStreamingContentBlocks).not.toHaveBeenCalled();
      expect(props.setStreamingToolCalls).not.toHaveBeenCalled();
      expect(props.setStreamingTasks).not.toHaveBeenCalled();
      expect(mockInvalidateQueries).toHaveBeenCalledWith({
        queryKey: ["chat", "conversation-stats", CONV_ID],
      });
    });

    it("should invalidate conversation stats on assistant message", () => {
      const props = makeProps();
      renderAndClear(props);

      act(() => {
        fireEvent("agent:message_created", {
          conversation_id: CONV_ID,
          context_id: CTX_ID,
          role: "assistant",
          message_id: "msg-123",
        });
      });

      expect(mockInvalidateQueries).toHaveBeenCalledWith({
        queryKey: ["chat", "conversation-stats", CONV_ID],
      });
    });
  });

  // --------------------------------------------------------------------------
  // 5. Run completed preserves visible streaming until message_created swaps in DB data
  // --------------------------------------------------------------------------
  describe("agent:run_completed", () => {
    it("should not clear streaming state on run completion before message_created", () => {
      const props = makeProps();
      renderAndClear(props);

      act(() => {
        fireEvent("agent:run_completed", {
          conversation_id: CONV_ID,
          context_id: CTX_ID,
        });
      });

      expect(props.setStreamingToolCalls).not.toHaveBeenCalled();
      expect(props.setStreamingContentBlocks).not.toHaveBeenCalled();
      expect(props.setStreamingTasks).not.toHaveBeenCalled();
      expect(mockInvalidateQueries).toHaveBeenCalledWith({
        queryKey: ["chat", "conversation-stats", CONV_ID],
      });
    });
  });

  // --------------------------------------------------------------------------
  // 5b. Turn completed preserves visible streaming until message_created swaps in DB data
  // --------------------------------------------------------------------------
  describe("agent:turn_completed", () => {
    it("should not clear streaming state on turn completion before message_created", () => {
      const props = makeProps();
      renderAndClear(props);

      act(() => {
        fireEvent("agent:turn_completed", {
          conversation_id: CONV_ID,
          context_id: CTX_ID,
        });
      });

      expect(props.setStreamingToolCalls).not.toHaveBeenCalled();
      expect(props.setStreamingContentBlocks).not.toHaveBeenCalled();
      expect(props.setStreamingTasks).not.toHaveBeenCalled();
      expect(mockInvalidateQueries).toHaveBeenCalledWith({
        queryKey: ["chat", "conversation-stats", CONV_ID],
      });
    });
  });

  describe("agent:usage_updated", () => {
    it("invalidates conversation stats without refetching the transcript during a live turn", () => {
      const props = makeProps();
      renderAndClear(props);

      act(() => {
        fireEvent("agent:usage_updated", {
          conversation_id: CONV_ID,
          context_id: CTX_ID,
        });
      });

      expect(mockInvalidateQueries).toHaveBeenCalledWith({
        queryKey: ["chat", "conversation-stats", CONV_ID],
      });
      expect(mockInvalidateQueries).not.toHaveBeenCalledWith({
        queryKey: ["chat", "conversations", CONV_ID],
      });
      expect(mockInvalidateQueries).not.toHaveBeenCalledWith({
        queryKey: ["chat", "conversations", CONV_ID, "history"],
      });
    });
  });

  // --------------------------------------------------------------------------
  // 6. Error clears streaming tool calls
  // --------------------------------------------------------------------------
  describe("agent:error", () => {
    it("should clear streaming tool calls on error", () => {
      const props = makeProps();
      renderAndClear(props);

      act(() => {
        fireEvent("agent:error", {
          conversation_id: CONV_ID,
          context_id: CTX_ID,
          error: "Something went wrong",
        });
      });

      // All three streaming state setters are called (full clear on error)
      expect(props.setStreamingToolCalls).toHaveBeenCalledTimes(1);
      expect(props.setStreamingContentBlocks).toHaveBeenCalledTimes(1);
      expect(props.setStreamingTasks).toHaveBeenCalledTimes(1);
      const result = executeUpdater<ToolCall[]>(props.setStreamingToolCalls, [
        { id: "tc1", name: "Read", arguments: {} },
      ]);
      expect(result).toEqual([]);
      // Query invalidation is now owned by useAgentEvents — not called here
      expect(mockInvalidateQueries).not.toHaveBeenCalled();
    });
  });

  // --------------------------------------------------------------------------
  // 7. Context relevance filtering
  // --------------------------------------------------------------------------
  describe("context relevance filtering", () => {
    it("should ignore events with a different conversation_id", () => {
      const props = makeProps();
      renderAndClear(props);

      act(() => {
        fireEvent("agent:tool_call", {
          tool_name: "Read",
          tool_id: "toolu_wrong",
          arguments: {},
          conversation_id: "other-conv-id",
          context_id: CTX_ID,
        });
      });

      expect(props.setStreamingToolCalls).not.toHaveBeenCalled();
    });

    it("should ignore events with a different context_id", () => {
      const props = makeProps();
      renderAndClear(props);

      act(() => {
        fireEvent("agent:chunk", {
          text: "ignored",
          conversation_id: CONV_ID,
          context_id: "other-ctx-id",
        });
      });

      expect(props.setStreamingContentBlocks).not.toHaveBeenCalled();
    });
  });

  // --------------------------------------------------------------------------
  // 8. Feature flags — agent:chunk only subscribed when supportsStreamingText
  // --------------------------------------------------------------------------
  describe("feature flags", () => {
    it("should NOT subscribe to agent:chunk when supportsStreamingText is false", () => {
      mockContextConfig = {
        supportsStreamingText: false,
        supportsSubagentTasks: false,
        supportsDiffViews: false,
      };

      const props = makeProps({ contextType: "task" as ContextType });
      renderHook(() => useChatEvents(props));

      // agent:chunk should have no handlers
      const chunkHandlers = subscriptions.get("agent:chunk") ?? [];
      expect(chunkHandlers).toHaveLength(0);
    });

    it("should subscribe to agent:chunk when supportsStreamingText is true", () => {
      const props = makeProps();
      renderHook(() => useChatEvents(props));

      const chunkHandlers = subscriptions.get("agent:chunk") ?? [];
      expect(chunkHandlers.length).toBeGreaterThan(0);
    });
  });

  // --------------------------------------------------------------------------
  // 9. Task started/completed lifecycle for subagent tasks
  // --------------------------------------------------------------------------
  describe("subagent task lifecycle", () => {
    it("should create a streaming task on agent:task_started", () => {
      const props = makeProps();
      renderAndClear(props);

      act(() => {
        fireEvent("agent:task_started", {
          tool_use_id: "toolu_task_001",
          description: "Analyze file structure",
          subagent_type: "Explore",
          model: "sonnet",
          conversation_id: CONV_ID,
          context_id: CTX_ID,
        });
      });

      expect(props.setStreamingTasks).toHaveBeenCalledTimes(1);
      expect(props.setStreamingContentBlocks).toHaveBeenCalledTimes(1);

      const nextMap = executeUpdater<Map<string, StreamingTask>>(
        props.setStreamingTasks,
        new Map(),
      );
      const task = nextMap.get("toolu_task_001");
      expect(task).toBeDefined();
      expect(task!.toolUseId).toBe("toolu_task_001");
      expect(task!.description).toBe("Analyze file structure");
      expect(task!.subagentType).toBe("Explore");
      expect(task!.model).toBe("sonnet");
      expect(task!.status).toBe("running");
      expect(task!.childToolCalls).toEqual([]);

      const blocks = executeUpdater<StreamingContentBlock[]>(
        props.setStreamingContentBlocks,
        [],
      );
      expect(blocks).toEqual([{ type: "task", toolUseId: "toolu_task_001" }]);
    });

    it("should enrich delegated streaming tasks from backend-native agent:task_started payloads", () => {
      const props = makeProps();
      renderAndClear(props);

      act(() => {
        fireEvent("agent:task_started", {
          tool_use_id: "toolu_delegate_live_001",
          tool_name: "delegate_start",
          description: "ralphx-execution-reviewer",
          subagent_type: "delegated",
          model: "gpt-5.4",
          delegated_job_id: "job-live-123",
          delegated_session_id: "delegated-session-123",
          delegated_conversation_id: "delegated-conv-123",
          delegated_agent_run_id: "run-123",
          provider_harness: "codex",
          logical_model: "gpt-5.4",
          logical_effort: "high",
          approval_policy: "never",
          sandbox_mode: "danger-full-access",
          conversation_id: CONV_ID,
          context_id: CTX_ID,
        });
      });

      expect(props.setStreamingTasks).toHaveBeenCalledTimes(1);

      const prevMap = new Map<string, StreamingTask>([
        ["toolu_delegate_live_001", {
          toolUseId: "toolu_delegate_live_001",
          toolName: "delegate_start",
          description: "Delegated specialist",
          subagentType: "delegated",
          model: "unknown",
          status: "running",
          startedAt: 12345,
          delegatedJobId: "job-live-123",
          childToolCalls: [],
        }],
      ]);
      const nextMap = executeUpdater<Map<string, StreamingTask>>(
        props.setStreamingTasks,
        prevMap,
      );

      const delegated = nextMap.get("toolu_delegate_live_001");
      expect(delegated).toBeDefined();
      expect(delegated!.startedAt).toBe(12345);
      expect(delegated!.description).toBe("ralphx-execution-reviewer");
      expect(delegated!.model).toBe("gpt-5.4");
      expect(delegated!.providerHarness).toBe("codex");
      expect(delegated!.logicalModel).toBe("gpt-5.4");
      expect(delegated!.logicalEffort).toBe("high");
      expect(delegated!.approvalPolicy).toBe("never");
      expect(delegated!.sandboxMode).toBe("danger-full-access");
      expect(delegated!.delegatedSessionId).toBe("delegated-session-123");
      expect(delegated!.delegatedConversationId).toBe("delegated-conv-123");
      expect(delegated!.delegatedAgentRunId).toBe("run-123");
    });

    it("should mark a streaming task as completed on agent:task_completed", () => {
      const props = makeProps();
      renderAndClear(props);

      act(() => {
        fireEvent("agent:task_completed", {
          tool_use_id: "toolu_task_002",
          agent_id: "agent-xyz",
          total_duration_ms: 5000,
          total_tokens: 1200,
          total_tool_use_count: 3,
          conversation_id: CONV_ID,
          context_id: CTX_ID,
        });
      });

      expect(props.setStreamingTasks).toHaveBeenCalledTimes(1);

      // Run updater with existing running task
      const existingTask: StreamingTask = {
        toolUseId: "toolu_task_002",
        toolName: "Task",
        description: "Some task",
        subagentType: "Plan",
        model: "opus",
        status: "running",
        startedAt: Date.now() - 5000,
        childToolCalls: [{ id: "tc1", name: "Read", arguments: {} }],
      };
      const prevMap = new Map<string, StreamingTask>([
        ["toolu_task_002", existingTask],
      ]);
      const nextMap = executeUpdater<Map<string, StreamingTask>>(
        props.setStreamingTasks,
        prevMap,
      );

      const completed = nextMap.get("toolu_task_002")!;
      expect(completed.status).toBe("completed");
      expect(completed.completedAt).toBeDefined();
      expect(completed.agentId).toBe("agent-xyz");
      expect(completed.totalDurationMs).toBe(5000);
      expect(completed.totalTokens).toBe(1200);
      expect(completed.totalToolUseCount).toBe(3);
      // Child tool calls should be preserved
      expect(completed.childToolCalls).toHaveLength(1);
    });

    it("should fold delegated terminal metadata from backend-native agent:task_completed payloads", () => {
      const props = makeProps();
      renderAndClear(props);

      act(() => {
        fireEvent("agent:task_completed", {
          tool_use_id: "toolu_delegate_live_002",
          agent_id: "run-xyz",
          status: "failed",
          delegated_job_id: "job-live-456",
          delegated_session_id: "delegated-session-456",
          delegated_conversation_id: "delegated-conv-456",
          delegated_agent_run_id: "run-xyz",
          provider_harness: "codex",
          provider_session_id: "provider-thread-1",
          upstream_provider: "openai",
          provider_profile: "openai",
          logical_model: "gpt-5.4",
          effective_model_id: "gpt-5.4",
          logical_effort: "high",
          effective_effort: "high",
          approval_policy: "never",
          sandbox_mode: "danger-full-access",
          total_duration_ms: 5000,
          total_tokens: 148,
          input_tokens: 100,
          output_tokens: 40,
          cache_creation_tokens: 6,
          cache_read_tokens: 2,
          estimated_usd: 0.12,
          text_output: "Delegated reviewer found a blocking issue",
          error: "Delegated reviewer failed validation",
          conversation_id: CONV_ID,
          context_id: CTX_ID,
        });
      });

      expect(props.setStreamingTasks).toHaveBeenCalledTimes(1);

      const prevMap = new Map<string, StreamingTask>([
        ["toolu_delegate_live_002", {
          toolUseId: "toolu_delegate_live_002",
          toolName: "delegate_start",
          description: "ralphx-execution-reviewer",
          subagentType: "delegated",
          model: "gpt-5.4",
          status: "running",
          startedAt: Date.now() - 5000,
          delegatedJobId: "job-live-456",
          childToolCalls: [],
        }],
      ]);
      const nextMap = executeUpdater<Map<string, StreamingTask>>(
        props.setStreamingTasks,
        prevMap,
      );

      const delegated = nextMap.get("toolu_delegate_live_002");
      expect(delegated).toBeDefined();
      expect(delegated!.status).toBe("failed");
      expect(delegated!.agentId).toBe("run-xyz");
      expect(delegated!.providerHarness).toBe("codex");
      expect(delegated!.providerSessionId).toBe("provider-thread-1");
      expect(delegated!.upstreamProvider).toBe("openai");
      expect(delegated!.providerProfile).toBe("openai");
      expect(delegated!.logicalModel).toBe("gpt-5.4");
      expect(delegated!.effectiveModelId).toBe("gpt-5.4");
      expect(delegated!.logicalEffort).toBe("high");
      expect(delegated!.effectiveEffort).toBe("high");
      expect(delegated!.approvalPolicy).toBe("never");
      expect(delegated!.sandboxMode).toBe("danger-full-access");
      expect(delegated!.totalDurationMs).toBe(5000);
      expect(delegated!.totalTokens).toBe(148);
      expect(delegated!.inputTokens).toBe(100);
      expect(delegated!.outputTokens).toBe(40);
      expect(delegated!.cacheCreationTokens).toBe(6);
      expect(delegated!.cacheReadTokens).toBe(2);
      expect(delegated!.estimatedUsd).toBe(0.12);
      expect(delegated!.textOutput).toBe("Delegated reviewer found a blocking issue");
      expect(delegated!.delegatedSessionId).toBe("delegated-session-456");
      expect(delegated!.delegatedConversationId).toBe("delegated-conv-456");
      expect(delegated!.delegatedAgentRunId).toBe("run-xyz");
      expect(delegated!.completedAt).toBeDefined();
    });

    it("should NOT subscribe to task events when supportsSubagentTasks is false", () => {
      mockContextConfig = {
        supportsStreamingText: false,
        supportsSubagentTasks: false,
        supportsDiffViews: false,
      };

      const props = makeProps({ contextType: "task" as ContextType });
      renderHook(() => useChatEvents(props));

      const startedHandlers = subscriptions.get("agent:task_started") ?? [];
      const completedHandlers = subscriptions.get("agent:task_completed") ?? [];
      expect(startedHandlers).toHaveLength(0);
      expect(completedHandlers).toHaveLength(0);
    });

    it("should create a delegated placeholder task immediately on delegate_start tool calls", () => {
      const props = makeProps();
      renderAndClear(props);

      act(() => {
        fireEvent("agent:tool_call", {
          tool_name: "delegate_start",
          tool_id: "toolu_delegate_001",
          arguments: {
            agent_name: "ralphx-execution-reviewer",
            prompt: "Review the patch",
            harness: "codex",
            model: "gpt-5.4",
          },
          result: [{ type: "text", text: JSON.stringify({ job_id: "job-123", status: "running" }) }],
          conversation_id: CONV_ID,
          context_id: CTX_ID,
        });
      });

      expect(props.setStreamingToolCalls).not.toHaveBeenCalled();
      expect(props.setStreamingTasks).toHaveBeenCalledTimes(1);
      expect(props.setStreamingContentBlocks).toHaveBeenCalledTimes(1);

      const blocks = executeUpdater<StreamingContentBlock[]>(
        props.setStreamingContentBlocks,
        [],
      );
      expect(blocks).toEqual([{ type: "task", toolUseId: "toolu_delegate_001" }]);

      const tasks = executeUpdater<Map<string, StreamingTask>>(
        props.setStreamingTasks,
        new Map(),
      );
      expect(tasks.get("toolu_delegate_001")).toMatchObject({
        toolUseId: "toolu_delegate_001",
        toolName: "delegate_start",
        description: "Review the patch",
        subagentType: "delegated",
        logicalModel: "gpt-5.4",
      });
    });

    it("should create a delegated placeholder task for namespaced delegate_start tool calls", () => {
      const props = makeProps();
      renderAndClear(props);

      act(() => {
        fireEvent("agent:tool_call", {
          tool_name: "ralphx::delegate_start",
          tool_id: "toolu_delegate_002",
          arguments: {
            agent_name: "ralphx-plan-critic-completeness",
            prompt: "Review the plan",
          },
          result: [{ type: "text", text: JSON.stringify({ job_id: "job-456", status: "running" }) }],
          conversation_id: CONV_ID,
          context_id: CTX_ID,
        });
      });

      const blocks = executeUpdater<StreamingContentBlock[]>(
        props.setStreamingContentBlocks,
        [],
      );
      expect(blocks).toEqual([{ type: "task", toolUseId: "toolu_delegate_002" }]);

      const tasks = executeUpdater<Map<string, StreamingTask>>(
        props.setStreamingTasks,
        new Map(),
      );
      expect(tasks.get("toolu_delegate_002")).toMatchObject({
        toolUseId: "toolu_delegate_002",
        toolName: "ralphx::delegate_start",
        subagentType: "delegated",
      });
    });

    it("should mark delegated placeholder tasks as failed when delegate_start returns an error result", () => {
      const props = makeProps();
      renderAndClear(props);

      act(() => {
        fireEvent("agent:tool_call", {
          tool_name: "delegate_start",
          tool_id: "toolu_delegate_fail_001",
          arguments: {
            agent_name: "ralphx-ideation-specialist-backend",
            prompt: "Investigate merge validation defaults",
          },
          conversation_id: CONV_ID,
          context_id: CTX_ID,
        });
      });

      act(() => {
        fireEvent("agent:tool_call", {
          tool_name: "result:toolu_delegate_fail_001",
          conversation_id: CONV_ID,
          context_id: CTX_ID,
          arguments: {},
          result: {
            content: [
              {
                type: "text",
                text: "ERROR: Unknown canonical caller agent 'ralphx-ideation'",
              },
            ],
          },
        });
      });

      let tasks = new Map<string, StreamingTask>();
      for (const call of props.setStreamingTasks.mock.calls) {
        const updater = call[0];
        tasks = typeof updater === "function" ? updater(tasks) : updater;
      }

      expect(tasks.get("toolu_delegate_fail_001")).toMatchObject({
        toolUseId: "toolu_delegate_fail_001",
        status: "failed",
        textOutput: "ERROR: Unknown canonical caller agent 'ralphx-ideation'",
      });
    });

    it("should ignore delegate_wait tool calls for delegated task state", () => {
      const props = makeProps();
      renderAndClear(props);

      act(() => {
        fireEvent("agent:tool_call", {
          tool_name: "delegate_wait",
          tool_id: "toolu_wait_001",
          arguments: { job_id: "job-123" },
          result: [{
            type: "text",
            text: JSON.stringify({
              job_id: "job-123",
              status: "completed",
              content: "Delegated review finished",
              delegated_status: {
                latest_run: {
                  harness: "codex",
                  upstream_provider: "openai",
                  provider_profile: "openai",
                  logical_model: "gpt-5.4",
                  effective_model_id: "gpt-5.4",
                  logical_effort: "high",
                  input_tokens: 100,
                  output_tokens: 40,
                  estimated_usd: 0.12,
                  started_at: "2026-04-12T10:00:00Z",
                  completed_at: "2026-04-12T10:00:05Z",
                },
              },
            }),
          }],
          conversation_id: CONV_ID,
          context_id: CTX_ID,
        });
      });

      expect(props.setStreamingToolCalls).not.toHaveBeenCalled();
      expect(props.setStreamingTasks).not.toHaveBeenCalled();
      expect(props.setStreamingContentBlocks).not.toHaveBeenCalled();
    });
  });

  // --------------------------------------------------------------------------
  // 10. Task position marker ordering (streaming Task card fix)
  // --------------------------------------------------------------------------
  describe("Task card position marker ordering", () => {
    it("should interleave task position marker between text blocks (text → task → text)", () => {
      const props = makeProps();
      renderAndClear(props);

      // 1. Text arrives first
      act(() => {
        fireEvent("agent:chunk", {
          text: "About to launch a task: ",
          conversation_id: CONV_ID,
          context_id: CTX_ID,
        });
      });

      // 2. Task tool_call arrives — should insert a position marker, not skip
      act(() => {
        fireEvent("agent:tool_call", {
          tool_name: "Task",
          tool_id: "toolu_task_abc",
          arguments: { description: "Explore codebase", subagent_type: "Explore" },
          conversation_id: CONV_ID,
          context_id: CTX_ID,
        });
      });

      // 3. More text arrives after the task
      act(() => {
        fireEvent("agent:chunk", {
          text: "Task launched.",
          conversation_id: CONV_ID,
          context_id: CTX_ID,
        });
      });

      // Simulate the accumulated streamingContentBlocks state by replaying updaters
      // against an initially empty array.
      let blocks: StreamingContentBlock[] = [];
      for (const call of props.setStreamingContentBlocks.mock.calls) {
        const updater = call[0];
        blocks = typeof updater === "function" ? updater(blocks) : updater;
      }

      // Expect: [text, task-marker, text] in chronological order
      expect(blocks).toHaveLength(3);
      expect(blocks[0]).toEqual({ type: "text", text: "About to launch a task: " });
      expect(blocks[1]).toEqual({ type: "task", toolUseId: "toolu_task_abc" });
      expect(blocks[2]).toEqual({ type: "text", text: "Task launched." });
    });

    it("should deduplicate task position markers when same tool_id arrives twice", () => {
      const props = makeProps();
      renderAndClear(props);

      // Fire the same Task tool_call twice (can happen with result events)
      act(() => {
        fireEvent("agent:tool_call", {
          tool_name: "Task",
          tool_id: "toolu_task_dup",
          arguments: { description: "Dup task", subagent_type: "Plan" },
          conversation_id: CONV_ID,
          context_id: CTX_ID,
        });
      });
      act(() => {
        fireEvent("agent:tool_call", {
          tool_name: "Task",
          tool_id: "toolu_task_dup",
          arguments: { description: "Dup task", subagent_type: "Plan" },
          result: "task done",
          conversation_id: CONV_ID,
          context_id: CTX_ID,
        });
      });

      let blocks: StreamingContentBlock[] = [];
      for (const call of props.setStreamingContentBlocks.mock.calls) {
        const updater = call[0];
        blocks = typeof updater === "function" ? updater(blocks) : updater;
      }

      // Only one task marker should exist
      const taskMarkers = blocks.filter((b) => b.type === "task");
      expect(taskMarkers).toHaveLength(1);
      expect(taskMarkers[0]).toEqual({ type: "task", toolUseId: "toolu_task_dup" });
    });

    it("should render null (no crash) when task position marker exists but agent:task_started has not yet arrived", () => {
      const props = makeProps();
      renderAndClear(props);

      // agent:tool_call for Task arrives — position marker added to streamingContentBlocks
      act(() => {
        fireEvent("agent:tool_call", {
          tool_name: "Task",
          tool_id: "toolu_task_late",
          arguments: { description: "Late task", subagent_type: "Bash" },
          conversation_id: CONV_ID,
          context_id: CTX_ID,
        });
      });

      // Verify the marker was added to streamingContentBlocks
      let blocks: StreamingContentBlock[] = [];
      for (const call of props.setStreamingContentBlocks.mock.calls) {
        const updater = call[0];
        blocks = typeof updater === "function" ? updater(blocks) : updater;
      }
      expect(blocks).toHaveLength(1);
      expect(blocks[0]).toEqual({ type: "task", toolUseId: "toolu_task_late" });

      // The streamingTasks map is still empty — agent:task_started has not arrived yet.
      // Simulate what ChatMessageList does: Map.get(toolUseId) returns undefined.
      const emptyTasksMap = new Map<string, StreamingTask>();
      const taskMetadata = emptyTasksMap.get("toolu_task_late");
      // Graceful: returns undefined — component renders null, no crash.
      expect(taskMetadata).toBeUndefined();

      // agent:task_started arrives late — task metadata now available
      act(() => {
        fireEvent("agent:task_started", {
          tool_use_id: "toolu_task_late",
          description: "Late task",
          subagent_type: "Bash",
          model: "haiku",
          conversation_id: CONV_ID,
          context_id: CTX_ID,
        });
      });

      // The task should now be in the map
      let tasksMap = new Map<string, StreamingTask>();
      for (const call of props.setStreamingTasks.mock.calls) {
        const updater = call[0];
        tasksMap = typeof updater === "function" ? updater(tasksMap) : updater;
      }
      const task = tasksMap.get("toolu_task_late");
      expect(task).toBeDefined();
      expect(task!.subagentType).toBe("Bash");
      expect(task!.status).toBe("running");
    });

    it("should clear all streaming state including task position markers on agent:error", () => {
      const props = makeProps();
      renderAndClear(props);

      // Stream a task during execution
      act(() => {
        fireEvent("agent:tool_call", {
          tool_name: "Task",
          tool_id: "toolu_task_err",
          arguments: { description: "Error task", subagent_type: "Plan" },
          conversation_id: CONV_ID,
          context_id: CTX_ID,
        });
      });
      act(() => {
        fireEvent("agent:task_started", {
          tool_use_id: "toolu_task_err",
          description: "Error task",
          subagent_type: "Plan",
          model: "sonnet",
          conversation_id: CONV_ID,
          context_id: CTX_ID,
        });
      });

      props.setStreamingContentBlocks.mockClear();
      props.setStreamingTasks.mockClear();

      // Error fires mid-task
      act(() => {
        fireEvent("agent:error", {
          conversation_id: CONV_ID,
          context_id: CTX_ID,
          error: "Agent crashed",
        });
      });

      // agent:error clears ALL streaming state (tool calls, content blocks, tasks)
      expect(props.setStreamingToolCalls).toHaveBeenCalled();
      expect(props.setStreamingContentBlocks).toHaveBeenCalled();
      expect(props.setStreamingTasks).toHaveBeenCalled();
    });
  });

  // --------------------------------------------------------------------------
  // 11. Agent tool call streaming (Agent == Task for rendering purposes)
  // --------------------------------------------------------------------------
  describe("Agent tool call streaming", () => {
    it("should create a task position marker when tool_name is 'Agent' (capitalized)", () => {
      const props = makeProps();
      renderAndClear(props);

      act(() => {
        fireEvent("agent:tool_call", {
          tool_name: "Agent",
          tool_id: "toolu_agent_001",
          arguments: {
            description: "Explore the codebase",
            subagent_type: "Explore",
            model: "sonnet",
          },
          conversation_id: CONV_ID,
          context_id: CTX_ID,
        });
      });

      // Agent tool call should create a task position marker in streamingContentBlocks
      expect(props.setStreamingContentBlocks).toHaveBeenCalledTimes(1);

      let blocks: StreamingContentBlock[] = [];
      for (const call of props.setStreamingContentBlocks.mock.calls) {
        const updater = call[0];
        blocks = typeof updater === "function" ? updater(blocks) : updater;
      }
      expect(blocks).toHaveLength(1);
      expect(blocks[0]).toEqual({ type: "task", toolUseId: "toolu_agent_001" });
    });

    it("should create a task position marker when tool_name is 'agent' (lowercase)", () => {
      const props = makeProps();
      renderAndClear(props);

      act(() => {
        fireEvent("agent:tool_call", {
          tool_name: "agent",
          tool_id: "toolu_agent_002",
          arguments: { description: "Run tests", subagent_type: "general-purpose" },
          conversation_id: CONV_ID,
          context_id: CTX_ID,
        });
      });

      let blocks: StreamingContentBlock[] = [];
      for (const call of props.setStreamingContentBlocks.mock.calls) {
        const updater = call[0];
        blocks = typeof updater === "function" ? updater(blocks) : updater;
      }
      expect(blocks).toHaveLength(1);
      expect(blocks[0]).toEqual({ type: "task", toolUseId: "toolu_agent_002" });
    });

    it("should create a task position marker when tool_name is 'AGENT' (uppercase)", () => {
      const props = makeProps();
      renderAndClear(props);

      act(() => {
        fireEvent("agent:tool_call", {
          tool_name: "AGENT",
          tool_id: "toolu_agent_003",
          arguments: { description: "Plan implementation", subagent_type: "Plan" },
          conversation_id: CONV_ID,
          context_id: CTX_ID,
        });
      });

      let blocks: StreamingContentBlock[] = [];
      for (const call of props.setStreamingContentBlocks.mock.calls) {
        const updater = call[0];
        blocks = typeof updater === "function" ? updater(blocks) : updater;
      }
      expect(blocks).toHaveLength(1);
      expect(blocks[0]).toEqual({ type: "task", toolUseId: "toolu_agent_003" });
    });

    it("should deduplicate Agent position markers when same tool_id fires twice", () => {
      const props = makeProps();
      renderAndClear(props);

      // First event: Agent tool call started
      act(() => {
        fireEvent("agent:tool_call", {
          tool_name: "Agent",
          tool_id: "toolu_agent_dup",
          arguments: { description: "Dup agent", subagent_type: "Explore" },
          conversation_id: CONV_ID,
          context_id: CTX_ID,
        });
      });
      // Second event: same tool_id with result (shouldn't add duplicate marker)
      act(() => {
        fireEvent("agent:tool_call", {
          tool_name: "Agent",
          tool_id: "toolu_agent_dup",
          arguments: { description: "Dup agent", subagent_type: "Explore" },
          result: "done",
          conversation_id: CONV_ID,
          context_id: CTX_ID,
        });
      });

      let blocks: StreamingContentBlock[] = [];
      for (const call of props.setStreamingContentBlocks.mock.calls) {
        const updater = call[0];
        blocks = typeof updater === "function" ? updater(blocks) : updater;
      }

      // Only one task marker
      const taskMarkers = blocks.filter((b) => b.type === "task");
      expect(taskMarkers).toHaveLength(1);
      expect(taskMarkers[0]).toEqual({ type: "task", toolUseId: "toolu_agent_dup" });
    });

    it("should create a StreamingTask on agent:task_started for an Agent tool_use_id", () => {
      const props = makeProps();
      renderAndClear(props);

      // Agent tool call fires first (creates position marker)
      act(() => {
        fireEvent("agent:tool_call", {
          tool_name: "Agent",
          tool_id: "toolu_agent_stream_1",
          arguments: {
            description: "Explore codebase",
            subagent_type: "Explore",
            model: "sonnet",
          },
          conversation_id: CONV_ID,
          context_id: CTX_ID,
        });
      });

      // task_started fires (creates StreamingTask for the Agent tool use)
      act(() => {
        fireEvent("agent:task_started", {
          tool_use_id: "toolu_agent_stream_1",
          description: "Explore codebase",
          subagent_type: "Explore",
          model: "sonnet",
          conversation_id: CONV_ID,
          context_id: CTX_ID,
        });
      });

      expect(props.setStreamingTasks).toHaveBeenCalledTimes(1);

      let tasksMap = new Map<string, StreamingTask>();
      for (const call of props.setStreamingTasks.mock.calls) {
        const updater = call[0];
        tasksMap = typeof updater === "function" ? updater(tasksMap) : updater;
      }

      const task = tasksMap.get("toolu_agent_stream_1");
      expect(task).toBeDefined();
      expect(task!.toolUseId).toBe("toolu_agent_stream_1");
      expect(task!.description).toBe("Explore codebase");
      expect(task!.subagentType).toBe("Explore");
      expect(task!.model).toBe("sonnet");
      expect(task!.status).toBe("running");
    });

    it("should interleave Agent position marker between text blocks (text → agent → text)", () => {
      const props = makeProps();
      renderAndClear(props);

      // 1. Text arrives first
      act(() => {
        fireEvent("agent:chunk", {
          text: "Spawning an agent: ",
          conversation_id: CONV_ID,
          context_id: CTX_ID,
        });
      });

      // 2. Agent tool call arrives — should insert a position marker
      act(() => {
        fireEvent("agent:tool_call", {
          tool_name: "Agent",
          tool_id: "toolu_agent_interleave",
          arguments: { description: "Do research", subagent_type: "general-purpose" },
          conversation_id: CONV_ID,
          context_id: CTX_ID,
        });
      });

      // 3. More text arrives after the agent
      act(() => {
        fireEvent("agent:chunk", {
          text: "Agent spawned.",
          conversation_id: CONV_ID,
          context_id: CTX_ID,
        });
      });

      let blocks: StreamingContentBlock[] = [];
      for (const call of props.setStreamingContentBlocks.mock.calls) {
        const updater = call[0];
        blocks = typeof updater === "function" ? updater(blocks) : updater;
      }

      // Expect: [text, task-marker, text] in chronological order
      expect(blocks).toHaveLength(3);
      expect(blocks[0]).toEqual({ type: "text", text: "Spawning an agent: " });
      expect(blocks[1]).toEqual({ type: "task", toolUseId: "toolu_agent_interleave" });
      expect(blocks[2]).toEqual({ type: "text", text: "Agent spawned." });
    });
  });

  // --------------------------------------------------------------------------
  // 12. Cleanup on unmount
  // --------------------------------------------------------------------------
  describe("cleanup", () => {
    it("should clear streaming state and unsubscribe on unmount", () => {
      const props = makeProps();
      const { unmount } = renderHook(() => useChatEvents(props));

      // Verify subscriptions exist
      expect(subscriptions.get("agent:tool_call")?.length).toBeGreaterThan(0);

      unmount();

      // Cleanup uses functional updaters for all three streaming state setters.
      // setIsFinalizing is NOT called on unmount — finalization is only cancelled on
      // genuine context switch via the dedicated [activeConversationId, contextId] effect.
      expect(props.setStreamingToolCalls).toHaveBeenCalled();
      expect(props.setStreamingContentBlocks).toHaveBeenCalled();
      expect(props.setStreamingTasks).toHaveBeenCalled();
      expect(props.setIsFinalizing).not.toHaveBeenCalledWith(false);

      // All subscriptions should be removed
      for (const [, handlers] of subscriptions) {
        expect(handlers).toHaveLength(0);
      }
    });

    it("should return same reference when streaming state is already empty (no-op)", () => {
      const props = makeProps();
      const { unmount } = renderHook(() => useChatEvents(props));

      unmount();

      // Verify functional updaters return same ref when already empty
      const emptyToolCalls: ToolCall[] = [];
      const toolCallResult = executeUpdater<ToolCall[]>(
        props.setStreamingToolCalls,
        emptyToolCalls,
        props.setStreamingToolCalls.mock.calls.length - 1,
      );
      expect(toolCallResult).toBe(emptyToolCalls); // Same reference!

      const emptyBlocks: StreamingContentBlock[] = [];
      const blockResult = executeUpdater<StreamingContentBlock[]>(
        props.setStreamingContentBlocks,
        emptyBlocks,
        props.setStreamingContentBlocks.mock.calls.length - 1,
      );
      expect(blockResult).toBe(emptyBlocks); // Same reference!

      const emptyTasks = new Map<string, StreamingTask>();
      const taskResult = executeUpdater<Map<string, StreamingTask>>(
        props.setStreamingTasks,
        emptyTasks,
        props.setStreamingTasks.mock.calls.length - 1,
      );
      expect(taskResult).toBe(emptyTasks); // Same reference!
    });
  });
});
