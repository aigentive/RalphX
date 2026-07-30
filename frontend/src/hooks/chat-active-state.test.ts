import { describe, expect, it } from "vitest";

import type { ActiveStreamingTaskResponse, ChatMessageResponse } from "@/api/chat";
import type { ToolCall } from "@/components/Chat/ToolCallIndicator";
import type { StreamingContentBlock, StreamingTask } from "@/types/streaming-task";
import {
  applyTranscriptInput,
  createLiveTranscriptState,
  mergeActiveStreamingContentBlocks as mergeActiveStreamingTaskAndToolBlocks,
  mergeActiveStreamingTasks,
  mergeActiveStreamingToolCalls,
  preserveBlocksIfUnchanged,
  projectPersistedStreamingContentBlocks,
  renderTranscriptBlocks,
  renderTranscriptSlots,
} from "./chat-active-state";

/**
 * The three text writers these tests used to exercise separately are now one
 * identity-keyed owner. These shims express the old entry points as the exact
 * owner compositions the production seams use, so the original assertions keep
 * their meaning.
 */

/** Was `removePersistedStreamingPrefix(live, persisted)` — the render seam. */
function releaseLiveTail(
  liveBlocks: readonly StreamingContentBlock[],
  persistedBlocks: readonly StreamingContentBlock[],
): StreamingContentBlock[] {
  const seeded = applyTranscriptInput(createLiveTranscriptState(), {
    kind: "persisted", runId: null, blocks: persistedBlocks,
  });
  return renderTranscriptBlocks(
    applyTranscriptInput(seeded, { kind: "live", runId: null, blocks: liveBlocks }),
  );
}

/** Was `mergePersistedStreamingAnchors(persisted, live)` — the recovery seam. */
function seedPersistedAnchors(
  persistedBlocks: readonly StreamingContentBlock[],
  liveBlocks: readonly StreamingContentBlock[],
): StreamingContentBlock[] {
  let state = applyTranscriptInput(createLiveTranscriptState(), {
    kind: "persisted", runId: null, blocks: persistedBlocks,
  });
  state = applyTranscriptInput(state, { kind: "live", runId: null, blocks: persistedBlocks });
  state = applyTranscriptInput(state, { kind: "live", runId: null, blocks: liveBlocks });
  return renderTranscriptSlots(state);
}

/** Was `mergeActiveStreamingContentBlocks` when the active state carried text. */
function mergeActiveStreamingContentBlocks(
  previous: StreamingContentBlock[],
  activeState: {
    partial_text: string;
    partial_text_segments?: string[];
    tool_calls: unknown[];
    streaming_tasks: ActiveStreamingTaskResponse[];
  },
): StreamingContentBlock[] {
  let state = applyTranscriptInput(createLiveTranscriptState(), {
    kind: "live", runId: null, blocks: previous,
  });
  state = activeState.partial_text_segments?.length
    ? applyTranscriptInput(state, {
        kind: "segments", runId: null, segments: activeState.partial_text_segments,
      })
    : applyTranscriptInput(state, { kind: "partialText", runId: null, text: activeState.partial_text });
  return preserveBlocksIfUnchanged(previous, mergeActiveStreamingTaskAndToolBlocks(
    renderTranscriptSlots(state),
    { ...activeState, partial_text: "", partial_text_segments: [] },
  ));
}

describe("chat-active-state helpers", () => {
  function taskFixture(overrides: Partial<StreamingTask> = {}): StreamingTask {
    return {
      toolUseId: "toolu_task",
      toolName: "delegate_start",
      description: "delegated task",
      subagentType: "delegated",
      model: "unknown",
      status: "running",
      startedAt: 100,
      childToolCalls: [],
      ...overrides,
    };
  }

  function messageFixture(overrides: Partial<ChatMessageResponse>): ChatMessageResponse {
    return {
      id: "message-1",
      sessionId: null,
      projectId: null,
      taskId: null,
      role: "assistant",
      content: "",
      metadata: null,
      parentMessageId: null,
      conversationId: "conversation-1",
      toolCalls: null,
      contentBlocks: null,
      sender: null,
      createdAt: "2026-07-29T00:00:00Z",
      ...overrides,
    };
  }

  it("keeps previous streaming tasks and tool calls when active-state inputs are empty", () => {
    const task: StreamingTask = {
      toolUseId: "toolu_task",
      toolName: "Agent",
      description: "existing task",
      subagentType: "Explore",
      model: "existing-model",
      status: "running",
      startedAt: 123,
      childToolCalls: [],
    };
    const tasks = new Map([["toolu_task", task]]);
    const toolCalls: ToolCall[] = [
      { id: "toolu_read", name: "Read", arguments: { file_path: "src/main.ts" } },
    ];

    expect(mergeActiveStreamingTasks(tasks, [])).toBe(tasks);
    expect(mergeActiveStreamingToolCalls(toolCalls, [])).toBe(toolCalls);
  });

  it("preserves existing task metadata when the active-state task only has live status", () => {
    const childToolCall: ToolCall = {
      id: "toolu_child",
      name: "Read",
      arguments: { file_path: "src/main.ts" },
    };
    const existingTask: StreamingTask = {
      toolUseId: "toolu_task",
      toolName: "Agent",
      description: "existing description",
      subagentType: "Explore",
      model: "existing-model",
      status: "running",
      startedAt: 123,
      completedAt: 456,
      totalDurationMs: 789,
      totalTokens: 321,
      totalToolUseCount: 4,
      agentId: "agent-1",
      delegatedJobId: "job-1",
      delegatedSessionId: "delegated-session-1",
      delegatedConversationId: "delegated-conversation-1",
      delegatedAgentRunId: "delegated-run-1",
      providerHarness: "codex",
      providerSessionId: "provider-session-1",
      upstreamProvider: "openai",
      providerProfile: "prod",
      logicalModel: "gpt-5.4",
      effectiveModelId: "gpt-5.4-2026-04-01",
      logicalEffort: "high",
      effectiveEffort: "high",
      approvalPolicy: "never",
      sandboxMode: "danger-full-access",
      inputTokens: 10,
      outputTokens: 20,
      cacheCreationTokens: 30,
      cacheReadTokens: 40,
      estimatedUsd: 0.12,
      textOutput: "done",
      childToolCalls: [childToolCall],
      seq: 7,
    };
    const activeTask: ActiveStreamingTaskResponse = {
      tool_use_id: "toolu_task",
      status: "completed",
    };

    const next = mergeActiveStreamingTasks(
      new Map([["toolu_task", existingTask]]),
      [activeTask],
    );

    expect(next.get("toolu_task")).toEqual({
      ...existingTask,
      status: "completed",
      delegationTerminalSource: "active-state",
    });
  });

  it("updates existing streaming tool calls instead of duplicating them", () => {
    const previous: ToolCall[] = [
      {
        id: "toolu_read",
        name: "Read",
        arguments: { file_path: "old.ts" },
        result: "old",
      },
    ];

    const next = mergeActiveStreamingToolCalls(previous, [
      {
        id: "toolu_read",
        name: "Read",
        arguments: { file_path: "new.ts" },
        result: "new",
      },
      {
        id: "toolu_write",
        name: "Write",
        arguments: { file_path: "new.ts" },
      },
    ]);

    expect(next).toEqual([
      {
        id: "toolu_read",
        name: "Read",
        arguments: { file_path: "new.ts" },
        result: "new",
      },
      {
        id: "toolu_write",
        name: "Write",
        arguments: { file_path: "new.ts" },
      },
    ]);
  });

  it("updates existing content blocks and skips tool calls represented by task cards", () => {
    const previous: StreamingContentBlock[] = [
      { type: "text", text: "Inspecting" },
      {
        type: "tool_use",
        toolCall: {
          id: "toolu_shell",
          name: "bash",
          arguments: { command: "pwd" },
          result: "old",
        },
      },
    ];

    const next = mergeActiveStreamingContentBlocks(previous, {
      partial_text: "Inspecting the app",
      streaming_tasks: [
        {
          tool_use_id: "toolu_delegate",
          status: "running",
          delegated_job_id: "job-1",
        },
      ],
      tool_calls: [
        {
          id: "toolu_delegate",
          name: "delegate_start",
          arguments: { description: "review" },
        },
        {
          id: "toolu_shell",
          name: "bash",
          arguments: { command: "pwd" },
          result: "/repo",
        },
      ],
    });

    expect(next).toEqual([
      { type: "text", text: "Inspecting the app" },
      {
        type: "tool_use",
        toolCall: {
          id: "toolu_shell",
          name: "bash",
          arguments: { command: "pwd" },
          result: "/repo",
        },
      },
      { type: "task", toolUseId: "toolu_delegate" },
    ]);
  });

  it("keeps recovered text segments on their original sides of an interleaved tool call", () => {
    const previous: StreamingContentBlock[] = [
      { type: "text", text: "Inspecting the active state. " },
      {
        type: "tool_use",
        toolCall: {
          id: "toolu_read",
          name: "Read",
          arguments: { file_path: "src/chat.ts" },
        },
      },
      { type: "text", text: "Patching the recovery path." },
    ];

    const next = mergeActiveStreamingContentBlocks(previous, {
      partial_text: "Inspecting the active state. Patching the recovery path. Keeping the tool order.",
      streaming_tasks: [],
      tool_calls: [],
    });

    expect(next).toEqual([
      { type: "text", text: "Inspecting the active state. " },
      {
        type: "tool_use",
        toolCall: {
          id: "toolu_read",
          name: "Read",
          arguments: { file_path: "src/chat.ts" },
        },
      },
      { type: "text", text: "Patching the recovery path. Keeping the tool order." },
    ]);
  });

  it("projects partial_text_segments into distinct text blocks when recovery has no live blocks", () => {
    const next = mergeActiveStreamingContentBlocks([], {
      partial_text: "Alpha before tool. Beta after tool.",
      partial_text_segments: ["Alpha before tool. ", "Beta after tool."],
      streaming_tasks: [],
      tool_calls: [],
    } as Parameters<typeof mergeActiveStreamingContentBlocks>[1]);

    expect(next).toEqual([
      { type: "text", text: "Alpha before tool. ", blockIndex: 0 },
      { type: "text", text: "Beta after tool.", blockIndex: 1 },
    ]);
  });

  it("uses partial_text_segments to retain anchored text around a tool and a mid-segment live tail", () => {
    const next = mergeActiveStreamingContentBlocks([
      { type: "text", text: "Alpha before tool. ", blockIndex: 0 },
      { type: "tool_use", toolCall: { id: "grep-1", name: "Grep", arguments: {} } },
      { type: "text", text: "tail.", blockIndex: 1 },
    ] as StreamingContentBlock[], {
      partial_text: "Alpha before tool. Beta after tool.",
      partial_text_segments: ["Alpha before tool. ", "Beta after tool."],
      streaming_tasks: [],
      tool_calls: [],
    } as Parameters<typeof mergeActiveStreamingContentBlocks>[1]);

    expect(next).toEqual([
      { type: "text", text: "Alpha before tool. ", blockIndex: 0 },
      { type: "tool_use", toolCall: { id: "grep-1", name: "Grep", arguments: {} } },
      { type: "text", text: "Beta after tool.", blockIndex: 1 },
    ]);
  });

  it("creates a gap-safe segment when partial_text_segments advances beyond recovered anchors", () => {
    const next = mergeActiveStreamingContentBlocks([
      { type: "text", text: "Alpha", blockIndex: 0 },
    ] as StreamingContentBlock[], {
      partial_text: "AlphaGamma",
      partial_text_segments: ["Alpha", "", "Gamma"],
      streaming_tasks: [],
      tool_calls: [],
    } as Parameters<typeof mergeActiveStreamingContentBlocks>[1]);

    expect(next).toEqual([
      { type: "text", text: "Alpha", blockIndex: 0 },
      { type: "text", text: "Gamma", blockIndex: 2 },
    ]);
  });

  it("keeps the legacy cumulative partial_text merge when segment metadata is absent", () => {
    const next = mergeActiveStreamingContentBlocks([
      { type: "text", text: "Alpha" },
    ], {
      partial_text: "AlphaBeta",
      streaming_tasks: [],
      tool_calls: [],
    });

    expect(next).toEqual([{ type: "text", text: "AlphaBeta" }]);
  });

  it("hydrates provider and synthetic lifecycle aliases into one provider-keyed delegation", () => {
    const toolCalls = [{
      id: "provider-tool",
      name: "delegate_start",
      arguments: { title: "Trace stale Claude MCP collision handling" },
      result: { job_id: "job-1", status: "running" },
    }];
    const activeTasks: ActiveStreamingTaskResponse[] = [{
      tool_use_id: "delegate-job:job-1",
      description: "ralphx-general-explorer",
      status: "running",
      delegated_job_id: "job-1",
      provider_harness: "codex",
      delegated_agent_run_id: "child-run-1",
    }];

    const tasks = mergeActiveStreamingTasks(new Map(), activeTasks, toolCalls);
    const calls = mergeActiveStreamingToolCalls([], toolCalls, activeTasks);
    const blocks = mergeActiveStreamingContentBlocks([], {
      partial_text: "",
      tool_calls: toolCalls,
      streaming_tasks: activeTasks,
    });

    expect([...tasks.keys()]).toEqual(["provider-tool"]);
    expect(tasks.get("provider-tool")).toMatchObject({
      description: "Trace stale Claude MCP collision handling",
      delegatedJobId: "job-1",
      providerHarness: "codex",
      delegatedAgentRunId: "child-run-1",
    });
    expect(calls).toEqual([]);
    expect(blocks).toEqual([{ type: "task", toolUseId: "provider-tool" }]);
  });

  it("does not revive a terminal live delegation from a running recovery snapshot", () => {
    const live = taskFixture({
      toolUseId: "provider-tool",
      status: "completed",
      completedAt: 500,
      textOutput: "done",
      delegatedJobId: "job-1",
      seq: 9,
    });
    const next = mergeActiveStreamingTasks(
      new Map([["provider-tool", live]]),
      [{
        tool_use_id: "provider-tool",
        status: "running",
      }],
      [{
        id: "provider-tool",
        name: "delegate_start",
        arguments: {},
        result: { job_id: "job-1" },
      }],
    );

    expect(next.get("provider-tool")).toMatchObject({
      status: "completed",
      completedAt: 500,
      textOutput: "done",
      seq: 9,
    });
  });

  it("leaves content unchanged when active-state has no partial text", () => {
    const previous: StreamingContentBlock[] = [
      { type: "text", text: "Already visible" },
    ];

    const next = mergeActiveStreamingContentBlocks(previous, {
      partial_text: "   ",
      streaming_tasks: [],
      tool_calls: [],
    });

    expect(next).toBe(previous);
  });

  it("adds active-state text when no live text block exists yet", () => {
    const next = mergeActiveStreamingContentBlocks([], {
      partial_text: "Recovered active text",
      streaming_tasks: [],
      tool_calls: [],
    });

    expect(next).toEqual([
      { type: "text", text: "Recovered active text" },
    ]);
  });

  it("does not duplicate active-state task markers", () => {
    const previous: StreamingContentBlock[] = [
      { type: "task", toolUseId: "toolu_delegate" },
    ];

    const next = mergeActiveStreamingContentBlocks(previous, {
      partial_text: "",
      streaming_tasks: [
        {
          tool_use_id: "toolu_delegate",
          status: "running",
        },
      ],
      tool_calls: [],
    });

    expect(next).toEqual([
      { type: "task", toolUseId: "toolu_delegate" },
    ]);
  });

  it("adds active-state tool calls when no matching live block exists", () => {
    const next = mergeActiveStreamingContentBlocks([], {
      partial_text: "",
      streaming_tasks: [],
      tool_calls: [
        {
          id: "toolu_shell",
          name: "bash",
          arguments: { command: "pwd" },
        },
      ],
    });

    expect(next).toEqual([
      {
        type: "tool_use",
        toolCall: {
          id: "toolu_shell",
          name: "bash",
          arguments: { command: "pwd" },
        },
      },
    ]);
  });

  it("restores active-state text when the first post-remount chunk arrived before hydration", () => {
    const previous: StreamingContentBlock[] = [
      { type: "text", text: "now checking the event merge" },
    ];

    const next = mergeActiveStreamingContentBlocks(previous, {
      partial_text: "I read the existing chat hooks and now checking the event merge",
      streaming_tasks: [],
      tool_calls: [],
    });

    expect(next).toEqual([
      { type: "text", text: "I read the existing chat hooks and now checking the event merge" },
    ]);
  });

  it("keeps the first post-remount chunk when hydration returns an older active-state prefix", () => {
    const previous: StreamingContentBlock[] = [
      { type: "text", text: "now checking the event merge" },
    ];

    const next = mergeActiveStreamingContentBlocks(previous, {
      partial_text: "I read the existing chat hooks and ",
      streaming_tasks: [],
      tool_calls: [],
    });

    expect(next).toEqual([
      { type: "text", text: "I read the existing chat hooks and now checking the event merge" },
    ]);
  });

  it("keeps chronological text when the live chunk precedes the active-state suffix", () => {
    const previous: StreamingContentBlock[] = [
      { type: "text", text: "I read the existing chat hooks and now checking the event merge" },
    ];

    const next = mergeActiveStreamingContentBlocks(previous, {
      partial_text: "event merge before patching",
      streaming_tasks: [],
      tool_calls: [],
    });

    expect(next).toEqual([
      { type: "text", text: "I read the existing chat hooks and now checking the event merge before patching" },
    ]);
  });

  it("hydrates an empty live text block from active-state text", () => {
    const next = mergeActiveStreamingContentBlocks(
      [{ type: "text", text: "" }],
      {
        partial_text: "Hydrated active-state text",
        streaming_tasks: [],
        tool_calls: [],
      },
    );

    expect(next).toEqual([
      { type: "text", text: "Hydrated active-state text" },
    ]);
  });

  it("keeps live text when it already contains the active-state snapshot", () => {
    const exact = mergeActiveStreamingContentBlocks(
      [{ type: "text", text: "already synced" }],
      {
        partial_text: "already synced",
        streaming_tasks: [],
        tool_calls: [],
      },
    );
    const prefix = mergeActiveStreamingContentBlocks(
      [{ type: "text", text: "I read the existing chat hooks and kept streaming" }],
      {
        partial_text: "I read the existing chat hooks",
        streaming_tasks: [],
        tool_calls: [],
      },
    );

    expect(exact).toEqual([
      { type: "text", text: "already synced" },
    ]);
    expect(prefix).toEqual([
      { type: "text", text: "I read the existing chat hooks and kept streaming" },
    ]);
  });

  it("renders only the active-state tail beyond persisted streaming anchors", () => {
    const persisted: StreamingContentBlock[] = [
      { type: "text", text: "Opening. " },
      {
        type: "tool_use",
        toolCall: { id: "toolu_grep", name: "Grep", arguments: {} },
      },
      { type: "text", text: "After grep. " },
      {
        type: "tool_use",
        toolCall: { id: "toolu_delegate", name: "delegate_start", arguments: {} },
      },
    ];
    const recovered: StreamingContentBlock[] = [
      ...persisted,
      { type: "task", toolUseId: "toolu_delegate" },
      {
        type: "tool_use",
        toolCall: { id: "toolu_edit", name: "Edit", arguments: {} },
      },
    ];

    expect(releaseLiveTail(recovered, persisted)).toEqual([
      {
        type: "tool_use",
        toolCall: { id: "toolu_edit", name: "Edit", arguments: {} },
      },
    ]);
    expect(releaseLiveTail(
      [
        { type: "text", text: "Opening. " },
        { type: "text", text: "After grep. Done." },
      ],
      [
        { type: "text", text: "Opening. " },
        { type: "text", text: "After grep. " },
      ],
    )).toEqual([{ type: "text", text: "Done." }]);
  });

  it("keeps a live tail that arrives before persisted-anchor hydration", () => {
    const persisted: StreamingContentBlock[] = [
      { type: "text", text: "Opening. " },
      {
        type: "tool_use",
        toolCall: { id: "toolu_grep", name: "Grep", arguments: {} },
      },
      { type: "text", text: "After grep. " },
    ];

    expect(seedPersistedAnchors(
      persisted,
      [
        { type: "text", text: "Late tail." },
        {
          type: "tool_use",
          toolCall: { id: "toolu_edit", name: "Edit", arguments: {} },
        },
      ],
    )).toEqual([
      ...persisted,
      { type: "text", text: "Late tail." },
      {
        type: "tool_use",
        toolCall: { id: "toolu_edit", name: "Edit", arguments: {} },
      },
    ]);
  });

  it("releases live text when persisted streaming text is at or ahead of it", () => {
    expect(releaseLiveTail(
      [{ type: "text", text: "Hello wor", blockIndex: 0 }],
      [{ type: "text", text: "Hello world!", blockIndex: 0 }],
    )).toEqual([]);

    expect(releaseLiveTail(
      [
        { type: "text", text: "Hello wor", blockIndex: 0 },
        {
          type: "tool_use",
          toolCall: { id: "toolu_novel", name: "Read", arguments: {} },
        },
      ],
      [{ type: "text", text: "Hello world!", blockIndex: 0 }],
    )).toEqual([
      {
        type: "tool_use",
        toolCall: { id: "toolu_novel", name: "Read", arguments: {} },
      },
    ]);
  });

  it("releases unindexed live text against persisted text by ordinal position", () => {
    expect(releaseLiveTail(
      [{ type: "text", text: "Opening. More" }],
      [{ type: "text", text: "Opening. ", blockIndex: 0 }],
    )).toEqual([{ type: "text", text: "More" }]);
    expect(releaseLiveTail(
      [{ type: "text", text: "Opening. " }],
      [{ type: "text", text: "Opening. More", blockIndex: 0 }],
    )).toEqual([]);
    expect(releaseLiveTail(
      [
        { type: "text", text: "First tail" },
        {
          type: "tool_use",
          toolCall: { id: "toolu_between", name: "Read", arguments: {} },
        },
        { type: "text", text: "Second tail" },
      ],
      [
        { type: "text", text: "First ", blockIndex: 0 },
        {
          type: "tool_use",
          toolCall: { id: "toolu_persisted", name: "Grep", arguments: {} },
        },
        { type: "text", text: "Second ", blockIndex: 1 },
      ],
    )).toEqual([
      { type: "text", text: "tail" },
      {
        type: "tool_use",
        toolCall: { id: "toolu_between", name: "Read", arguments: {} },
      },
      { type: "text", text: "tail" },
    ]);
  });

  it("keeps divergent live text verbatim instead of falsely releasing it", () => {
    const live: StreamingContentBlock[] = [
      { type: "text", text: "completely different", blockIndex: 0 },
    ];

    expect(releaseLiveTail(
      live,
      [{ type: "text", text: "Opening. ", blockIndex: 0 }],
    )).toEqual(live);
  });

  it("projects streaming anchors only for the active run while retaining legacy rows", () => {
    const messages = [
      messageFixture({
        id: "message-old",
        runId: "run-old",
        timelineStatus: "streaming",
        contentBlocks: [
          { type: "text", text: "Old text" },
          { type: "tool_use", id: "toolu_old", name: "Read", arguments: {} },
        ],
      }),
      messageFixture({
        id: "message-new",
        runId: "run-new",
        timelineStatus: "streaming",
        timelineBlockIndex: 0,
        contentBlocks: [
          { type: "text", text: "New text" },
          { type: "tool_use", id: "toolu_new", name: "Write", arguments: {} },
        ],
      }),
      messageFixture({
        id: "message-new-after-tool",
        runId: "run-new",
        timelineStatus: "streaming",
        timelineBlockIndex: 2,
        contentBlocks: [{ type: "text", text: "New text after tool" }],
      }),
    ];

    expect(projectPersistedStreamingContentBlocks(messages, "run-new")).toEqual([
      { type: "text", text: "New text", blockIndex: 0 },
      {
        type: "tool_use",
        toolCall: { id: "toolu_new", name: "Write", arguments: {} },
      },
      { type: "text", text: "New text after tool", blockIndex: 2 },
    ]);
    expect(projectPersistedStreamingContentBlocks([
      messages[1],
      messageFixture({
        id: "message-legacy",
        runId: null,
        timelineStatus: "streaming",
        contentBlocks: [{ type: "text", text: "Legacy text" }],
      }),
    ], "run-new")).toEqual([
      { type: "text", text: "New text", blockIndex: 0 },
      {
        type: "tool_use",
        toolCall: { id: "toolu_new", name: "Write", arguments: {} },
      },
      { type: "text", text: "Legacy text", blockIndex: 1 },
    ]);
    expect(projectPersistedStreamingContentBlocks(messages)).toEqual([
      { type: "text", text: "Old text", blockIndex: 0 },
      {
        type: "tool_use",
        toolCall: { id: "toolu_old", name: "Read", arguments: {} },
      },
      { type: "text", text: "New text", blockIndex: 0 },
      {
        type: "tool_use",
        toolCall: { id: "toolu_new", name: "Write", arguments: {} },
      },
      { type: "text", text: "New text after tool", blockIndex: 2 },
    ]);
  });

  it("merges indexed persisted text anchors but preserves unindexed append behavior", () => {
    expect(seedPersistedAnchors(
      [{ type: "text", text: "Hello wor", blockIndex: 0 }],
      [{ type: "text", text: "Hello world", blockIndex: 0 }],
    )).toEqual([{ type: "text", text: "Hello world", blockIndex: 0 }]);
    expect(seedPersistedAnchors(
      [{ type: "text", text: "Hello wor" }],
      [
        { type: "text", text: "Hello wor" },
        { type: "text", text: "Hello world" },
      ],
    )).toEqual([
      { type: "text", text: "Hello wor" },
      { type: "text", text: "Hello world" },
    ]);
  });

  it("lets the persisted anchor lead when a same-index live text is disjoint", () => {
    const merged = seedPersistedAnchors(
      [{ type: "text", text: "Persisted", blockIndex: 0 }],
      [{ type: "text", text: "Live", blockIndex: 0 }],
    );

    expect(merged).toEqual([{ type: "text", text: "PersistedLive", blockIndex: 0 }]);
    // The merged anchor must stay trimmable against the durable row, otherwise
    // the persisted copy renders again as a live supplement.
    expect(releaseLiveTail(
      merged,
      [{ type: "text", text: "Persisted", blockIndex: 0 }],
    )).toEqual([{ type: "text", text: "Live", blockIndex: 0 }]);
  });
});
