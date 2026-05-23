import { describe, expect, it } from "vitest";
import type { StreamingContentBlock, StreamingTask } from "@/types/streaming-task";
import {
  buildStreamingTranscriptWindow,
  EMPTY_STREAMING_TRANSCRIPT_WINDOW,
  getNextStreamingTranscriptWindow,
  STREAMING_TRANSCRIPT_BLOCK_TAIL_LIMIT,
} from "./ChatMessageList.streamingWindow";

function textBlock(index: number, text = `Live update ${index}`): StreamingContentBlock {
  return { type: "text", text, seq: index };
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

describe("ChatMessageList streaming transcript window", () => {
  it("returns the shared empty window for empty live blocks", () => {
    expect(buildStreamingTranscriptWindow([], new Map())).toBe(EMPTY_STREAMING_TRANSCRIPT_WINDOW);
  });

  it("keeps short live streams unhidden", () => {
    const blocks = [textBlock(1), textBlock(2)];

    expect(buildStreamingTranscriptWindow(blocks, new Map())).toEqual({
      contentBlocks: blocks,
      hiddenBlockCount: 0,
      sourceBlockCount: 2,
    });
  });

  it("keeps the most recent live blocks in a long stream", () => {
    const blocks = Array.from({ length: STREAMING_TRANSCRIPT_BLOCK_TAIL_LIMIT + 5 }, (_, index) =>
      textBlock(index + 1)
    );

    const window = buildStreamingTranscriptWindow(blocks, new Map());

    expect(window.sourceBlockCount).toBe(45);
    expect(window.hiddenBlockCount).toBe(5);
    expect(window.contentBlocks).toHaveLength(STREAMING_TRANSCRIPT_BLOCK_TAIL_LIMIT);
    expect(window.contentBlocks[0]).toEqual(textBlock(6));
    expect(window.contentBlocks.at(-1)).toEqual(textBlock(45));
  });

  it("pins running task blocks older than the live tail but drops completed task blocks", () => {
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

    const window = buildStreamingTranscriptWindow(
      blocks,
      new Map([
        [activeTask.toolUseId, activeTask],
        [completedTask.toolUseId, completedTask],
      ])
    );

    expect(window.contentBlocks).toContainEqual({ type: "task", toolUseId: activeTask.toolUseId });
    expect(window.contentBlocks).not.toContainEqual({ type: "task", toolUseId: completedTask.toolUseId });
    expect(window.contentBlocks.at(-1)).toEqual(textBlock(60));
  });

  it("freezes, resumes, and clears the rendered window based on live-tail state", () => {
    const previous = buildStreamingTranscriptWindow([textBlock(1)], new Map());
    const live = buildStreamingTranscriptWindow([textBlock(2)], new Map());

    expect(getNextStreamingTranscriptWindow(previous, live, false)).toBe(previous);
    expect(getNextStreamingTranscriptWindow(previous, live, true)).toBe(live);
    expect(getNextStreamingTranscriptWindow(previous, EMPTY_STREAMING_TRANSCRIPT_WINDOW, true)).toBe(
      EMPTY_STREAMING_TRANSCRIPT_WINDOW
    );
    expect(
      getNextStreamingTranscriptWindow(
        EMPTY_STREAMING_TRANSCRIPT_WINDOW,
        EMPTY_STREAMING_TRANSCRIPT_WINDOW,
        true
      )
    ).toBe(EMPTY_STREAMING_TRANSCRIPT_WINDOW);
  });
});
