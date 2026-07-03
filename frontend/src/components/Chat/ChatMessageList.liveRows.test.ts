import { describe, expect, it } from "vitest";
import type { StreamingContentBlock, StreamingTask } from "@/types/streaming-task";
import { buildLiveTranscriptRows } from "./ChatMessageList.liveRows";

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

describe("ChatMessageList live transcript rows", () => {
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

  it("carries live block receipt timestamps onto visible rows", () => {
    const blocks = [
      { type: "text", text: "Before user send", receivedAt: 1_000 },
      { type: "text", text: "After user send", receivedAt: 3_000 },
    ] satisfies StreamingContentBlock[];

    const rows = buildLiveTranscriptRows(blocks, new Map());

    expect(rows[0]).toMatchObject({ kind: "text", receivedAt: 1_000 });
    expect(rows[1]).toMatchObject({ kind: "text", receivedAt: 3_000 });
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

  it("keeps task rows whenever task metadata is available", () => {
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

    expect(rows).toContainEqual(expect.objectContaining({ kind: "task", toolUseId: activeTask.toolUseId }));
    expect(rows).toContainEqual(expect.objectContaining({ kind: "task", toolUseId: completedTask.toolUseId }));
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

  it("filters hidden tool calls before grouping visible rows", () => {
    const rows = buildLiveTranscriptRows(
      [toolBlock(1, "hidden"), toolBlock(2, "Read"), toolBlock(3, "hidden")],
      new Map(),
      (toolCall) => toolCall.name === "hidden",
    );

    expect(rows).toHaveLength(1);
    expect(rows[0]).toMatchObject({ kind: "tool_group", count: 1 });
  });
});
