import { describe, expect, it } from "vitest";

import type { ActiveStreamingTaskResponse } from "@/api/chat";
import type { ToolCall } from "@/components/Chat/ToolCallIndicator";
import type { StreamingContentBlock, StreamingTask } from "@/types/streaming-task";
import {
  mergeActiveStreamingContentBlocks,
  mergeActiveStreamingTasks,
  mergeActiveStreamingToolCalls,
} from "./chat-active-state";

describe("chat-active-state helpers", () => {
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
});
