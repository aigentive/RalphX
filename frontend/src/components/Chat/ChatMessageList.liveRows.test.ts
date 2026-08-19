import { describe, expect, it } from "vitest";
import type { StreamingContentBlock, StreamingTask } from "@/types/streaming-task";
import {
  buildLiveTranscriptRows,
  isLiveThinkingGroupKey,
  liveThinkingGroupKey,
  liveTranscriptRowSortTimes,
} from "./ChatMessageList.liveRows";
import type { LiveTranscriptRow } from "./ChatMessageList.liveRows";

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

  it("keeps one thinking group between text and tool activity", () => {
    const rows = buildLiveTranscriptRows([
      textBlock(1, "Before"),
      { type: "thinking", text: "Reasoning", blockIndex: 2, seq: 2 },
      toolBlock(3),
    ], new Map());

    expect(rows.map((row) => row.kind)).toEqual(["text", "thinking_group", "tool_group"]);
    expect(rows[1]).toMatchObject({ kind: "thinking_group", blocks: [{ block: { text: "Reasoning", blockIndex: 2 } }] });
  });

  it("hides settled empty thinking rows while keeping running and token-progress rows", () => {
    const rows = buildLiveTranscriptRows([
      { type: "thinking", text: "", blockIndex: 0, isSettled: true },
      { type: "thinking", text: "", blockIndex: 1, isSettled: false },
      { type: "thinking", text: "", blockIndex: 2, estimatedTokens: 2_000 },
    ], new Map());

    expect(rows).toHaveLength(1);
    expect(rows[0]).toMatchObject({ kind: "thinking_group", blocks: [{ block: { blockIndex: 1 } }, { block: { blockIndex: 2, estimatedTokens: 2_000 } }] });
  });

  it("coalesces visibly adjacent thinking with a key anchored on the first block", () => {
    const first = { type: "thinking" as const, text: "First", blockIndex: 4, isSettled: false };
    const rows = buildLiveTranscriptRows([first, { type: "thinking", text: "Second", blockIndex: 5, isSettled: false }], new Map());

    expect(rows).toMatchObject([{
      kind: "thinking_group",
      key: liveThinkingGroupKey(first, 0),
      blocks: [{ index: 0 }, { index: 1 }],
    }]);
  });

  it("keeps live thinking grouped across a hidden tool call", () => {
    const first = { type: "thinking" as const, text: "First", blockIndex: 1, isSettled: false };
    const second = { type: "thinking" as const, text: "Second", blockIndex: 3, isSettled: false };
    const rows = buildLiveTranscriptRows(
      [first, toolBlock(2, "hidden"), second],
      new Map(),
      (toolCall) => toolCall.name === "hidden",
    );

    expect(rows.map((row) => row.kind)).toEqual(["thinking_group"]);
    expect(rows[0]).toMatchObject({
      kind: "thinking_group",
      blocks: [{ block: first }, { block: second }],
    });
  });

  it("keeps live thinking grouped across an interleaved settled-empty segment", () => {
    const first = { type: "thinking" as const, text: "First", blockIndex: 1, isSettled: false };
    const empty = { type: "thinking" as const, text: "", blockIndex: 2, isSettled: true };
    const second = { type: "thinking" as const, text: "Second", blockIndex: 3, isSettled: false };
    const rows = buildLiveTranscriptRows([first, empty, second], new Map());

    expect(rows.map((row) => row.kind)).toEqual(["thinking_group"]);
    expect(rows[0]).toMatchObject({
      kind: "thinking_group",
      blocks: [{ block: first }, { block: second }],
    });
  });

  it("stops a tool scan at thinking so the thinking block is not swallowed", () => {
    const rows = buildLiveTranscriptRows([
      toolBlock(1),
      { type: "thinking", text: "Visible after tool", blockIndex: 2, isSettled: false },
      toolBlock(3),
    ], new Map());

    expect(rows.map((row) => row.kind)).toEqual(["tool_group", "thinking_group", "tool_group"]);
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

describe("liveTranscriptRowSortTimes", () => {
  function row(key: string, receivedAt?: number): LiveTranscriptRow {
    return {
      kind: "text",
      key,
      text: key,
      sourceIndex: 0,
      ...(receivedAt != null ? { receivedAt } : {}),
    };
  }

  it("uses receipt times so persisted rows can interleave with the live tail", () => {
    const sortTimes = liveTranscriptRowSortTimes([
      row("first", 1_000),
      row("second", 3_000),
    ]);

    expect(sortTimes).toEqual([1_000, 3_000]);
  });

  it("carries the last known receipt time forward across rows that have none", () => {
    const sortTimes = liveTranscriptRowSortTimes([
      row("first", 1_000),
      row("gap"),
      row("last", 3_000),
    ]);

    expect(sortTimes).toEqual([1_000, 1_000, 3_000]);
  });

  it("carries the first known receipt time back over recovered prefix rows", () => {
    const sortTimes = liveTranscriptRowSortTimes([
      row("recovered-text"),
      row("recovered-tools"),
      row("late-live", 60_000),
    ]);

    // Recovered rows carry no wall clock; they must stay ahead of the late
    // live row instead of floating to epoch 0.
    expect(sortTimes).toEqual([60_000, 60_000, 60_000]);
  });

  it("keeps the legacy bottom-pinned scheme when no row carries a receipt time", () => {
    const rows = [row("a"), row("b"), row("c")];

    const sortTimes = liveTranscriptRowSortTimes(rows);

    expect(sortTimes).toEqual([
      Number.MAX_SAFE_INTEGER - 4,
      Number.MAX_SAFE_INTEGER - 3,
      Number.MAX_SAFE_INTEGER - 2,
    ]);
    expect(sortTimes.every((sortTime) => sortTime < Number.MAX_SAFE_INTEGER)).toBe(true);
  });

  it("returns an empty projection for an empty tail", () => {
    expect(liveTranscriptRowSortTimes([])).toEqual([]);
  });
});
