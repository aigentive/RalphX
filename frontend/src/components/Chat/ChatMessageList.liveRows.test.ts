import { describe, expect, it } from "vitest";
import type { StreamingContentBlock, StreamingTask } from "@/types/streaming-task";
import {
  buildLiveTranscriptRows,
  isLiveThinkingGroupKey,
  liveThinkingGroupKey,
} from "./ChatMessageList.liveRows";

function textBlock(index: number, text = `Live update ${index}`): StreamingContentBlock {
  return { type: "text", text, seq: index };
}

function toolBlock(index: number, name = "Read"): StreamingContentBlock {
  return {
    type: "tool_use",
    toolCall: {
      id: `tool-${index}`,
      name,
      arguments: { index },
    },
    seq: index,
  };
}

function runningTask(toolUseId: string): StreamingTask {
  return {
    toolUseId,
    toolName: "Task",
    description: "Investigate the issue",
    subagentType: "Explore",
    model: "sonnet",
    status: "running",
    startedAt: 1,
    childToolCalls: [],
  };
}

function delegatedTask(toolUseId: string): StreamingTask {
  return {
    ...runningTask(toolUseId),
    toolName: "ralphx::delegate_start",
    subagentType: "delegated",
    delegatedJobId: `job-${toolUseId}`,
  };
}

describe("ChatMessageList live transcript rows", () => {
  it("identifies keys built for live thinking rows", () => {
    const key = liveThinkingGroupKey({ type: "thinking", text: "Reasoning", blockIndex: 2 }, 0);

    expect(isLiveThinkingGroupKey(key)).toBe(true);
    expect(isLiveThinkingGroupKey("streaming-text:block-2")).toBe(false);
  });

  it("returns no rows for empty live blocks", () => {
    expect(buildLiveTranscriptRows([], new Map())).toEqual([]);
  });

  it("keeps short live streams as visible rows", () => {
    const blocks = [textBlock(1), textBlock(2)];

    expect(buildLiveTranscriptRows(blocks, new Map()).map((row) => row.kind)).toEqual([
      "text",
      "text",
    ]);
  });

  it("keeps thinking as its own row between text and tool activity", () => {
    const rows = buildLiveTranscriptRows([
      textBlock(1, "Before"),
      { type: "thinking", text: "Reasoning", blockIndex: 2, seq: 2 },
      toolBlock(3),
    ], new Map());

    expect(rows.map((row) => row.kind)).toEqual(["text", "thinking", "tool_group"]);
    expect(rows[1]).toMatchObject({ kind: "thinking", block: { text: "Reasoning", blockIndex: 2 } });
  });

  it("carries live block receipt timestamps onto visible rows", () => {
    const blocks = [
      { type: "text", text: "Before user send", receivedAt: 1_000 },
      { type: "text", text: "After user send", receivedAt: 3_000 },
    ] satisfies StreamingContentBlock[];

    const rows = buildLiveTranscriptRows(blocks, new Map());

    expect(rows[0]).toMatchObject({ kind: "text", receivedAt: 1_000 });
    expect(rows[1]).toMatchObject({ kind: "text", receivedAt: 3_000 });
  });

  it("keeps recovered rows in their source-array order when only some rows carry receipt times", () => {
    const rows = buildLiveTranscriptRows([
      { type: "text", text: "Recovered first", seq: 1 },
      { type: "tool_use", toolCall: { id: "grep", name: "Grep", arguments: {} }, receivedAt: 50_000 },
      { type: "text", text: "Recovered after tool", seq: 3 },
      { type: "tool_use", toolCall: { id: "late", name: "Write", arguments: {} }, receivedAt: 60_000 },
    ], new Map());

    // TimelineItem sorting must preserve this projection order; wall-clock
    // receipt times are not chronology for hydration-recovered rows.
    expect(rows.map((row) => row.kind)).toEqual([
      "text", "tool_group", "text", "tool_group",
    ]);
  });

  it("keeps every live text row available instead of tail-clipping raw blocks", () => {
    const blocks = Array.from({ length: 45 }, (_, index) =>
      textBlock(index + 1)
    );

    const rows = buildLiveTranscriptRows(blocks, new Map());

    expect(rows).toHaveLength(45);
    expect(rows[0]).toMatchObject({ kind: "text", text: "Live update 1" });
    expect(rows.at(-1)).toMatchObject({ kind: "text", text: "Live update 45" });
  });

  it("keeps task entries promoted inside activity rows whenever task metadata is available", () => {
    const activeTask = runningTask("task-active");
    const completedTask: StreamingTask = {
      ...runningTask("task-complete"),
      status: "completed",
    };
    const blocks: StreamingContentBlock[] = [
      { type: "task", toolUseId: activeTask.toolUseId },
      { type: "task", toolUseId: completedTask.toolUseId },
      ...Array.from({ length: 60 }, (_, index) => textBlock(index + 1)),
    ];

    const rows = buildLiveTranscriptRows(
      blocks,
      new Map([
        [activeTask.toolUseId, activeTask],
        [completedTask.toolUseId, completedTask],
      ])
    );

    expect(rows[0]).toMatchObject({
      kind: "tool_group",
      taskEntries: [
        { toolUseId: activeTask.toolUseId },
        { toolUseId: completedTask.toolUseId },
      ],
    });
    expect(rows.at(-1)).toMatchObject({ kind: "text", text: "Live update 60" });
  });

  it("groups consecutive tool calls into one visible live row", () => {
    const rows = buildLiveTranscriptRows(
      [
        textBlock(1, "Before tools"),
        toolBlock(2),
        toolBlock(3),
        toolBlock(4),
        textBlock(5, "After tools"),
      ],
      new Map(),
    );

    expect(rows.map((row) => row.kind)).toEqual(["text", "tool_group", "text"]);
    const toolGroup = rows[1];
    expect(toolGroup).toMatchObject({ kind: "tool_group", count: 3 });
  });

  it("keeps adjacent delegated tasks in one activity run without hiding their promoted rows", () => {
    const task = delegatedTask("delegate-1");
    const rows = buildLiveTranscriptRows(
      [
        toolBlock(1, "Write"),
        { type: "task", toolUseId: task.toolUseId, seq: 2 },
        toolBlock(3, "Edit"),
      ],
      new Map([[task.toolUseId, task]]),
    );

    expect(rows).toHaveLength(1);
    expect(rows[0]).toMatchObject({
      kind: "tool_group",
      count: 3,
      taskEntries: [{ toolUseId: "delegate-1" }],
    });
    expect(rows[0]?.kind === "tool_group" ? rows[0].entries : []).toHaveLength(2);
  });

  it("filters hidden tool calls before grouping visible rows", () => {
    const rows = buildLiveTranscriptRows(
      [toolBlock(1, "hidden"), toolBlock(2, "Read"), toolBlock(3, "hidden")],
      new Map(),
      (toolCall) => toolCall.name === "hidden",
    );

    expect(rows).toHaveLength(1);
    expect(rows[0]).toMatchObject({ kind: "tool_group", count: 1 });
  });

  it("suppresses every live alias for a job once its persisted representative is available", () => {
    const delegated = delegatedTask("lifecycle-alias");
    const rows = buildLiveTranscriptRows(
      [
        toolBlock(1, "delegate_start"),
        { type: "task", toolUseId: delegated.toolUseId, seq: 2 },
      ],
      new Map([[delegated.toolUseId, delegated]]),
      (toolCall) => toolCall.name === "delegate_start",
      (task) => task.delegatedJobId === delegated.delegatedJobId,
    );

    expect(rows).toEqual([]);
  });
});
