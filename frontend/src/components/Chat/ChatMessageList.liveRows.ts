import type { StreamingContentBlock, StreamingTask } from "@/types/streaming-task";
import type { ToolCall } from "./ToolCallIndicator";

export type StreamingToolUseBlock = Extract<StreamingContentBlock, { type: "tool_use" }>;

export interface LiveTranscriptToolEntry {
  block: StreamingToolUseBlock;
  index: number;
}

export type LiveTranscriptRow =
  | {
      kind: "text";
      key: string;
      text: string;
      sourceIndex: number;
    }
  | {
      kind: "task";
      key: string;
      toolUseId: string;
      sourceIndex: number;
    }
  | {
      kind: "tool_call";
      key: string;
      block: StreamingToolUseBlock;
      index: number;
      toolCall: ToolCall;
    }
  | {
      kind: "tool_group";
      key: string;
      entries: LiveTranscriptToolEntry[];
      count: number;
      sourceIndex: number;
    };

export type ShouldHideLiveToolCall = (toolCall: ToolCall) => boolean;

function blockKeyPart(block: StreamingContentBlock, index: number): string {
  const seq = "seq" in block ? block.seq : undefined;
  return seq != null ? `seq-${seq}` : `idx-${index}`;
}

function textRowKey(block: StreamingContentBlock, index: number): string {
  return `streaming-text:${blockKeyPart(block, index)}`;
}

function taskRowKey(toolUseId: string): string {
  return `streaming-task:${toolUseId}`;
}

export function liveToolGroupKey(entries: LiveTranscriptToolEntry[]): string {
  const first = entries[0];
  if (!first) {
    return "streaming-tool-group:empty";
  }
  return [
    "streaming-tool-group",
    first.block.toolCall.id || first.block.seq || first.index,
  ].join(":");
}

export function buildLiveTranscriptRows(
  contentBlocks: StreamingContentBlock[],
  streamingTasks: Map<string, StreamingTask> | undefined,
  shouldHideToolCall: ShouldHideLiveToolCall = () => false,
): LiveTranscriptRow[] {
  if (contentBlocks.length === 0) {
    return [];
  }

  const rows: LiveTranscriptRow[] = [];
  let index = 0;
  while (index < contentBlocks.length) {
    const block = contentBlocks[index];
    if (!block) {
      index += 1;
      continue;
    }

    if (block.type === "text") {
      if (block.text.trim().length > 0) {
        rows.push({
          kind: "text",
          key: textRowKey(block, index),
          text: block.text,
          sourceIndex: index,
        });
      }
      index += 1;
      continue;
    }

    if (block.type === "task") {
      if (streamingTasks?.has(block.toolUseId)) {
        rows.push({
          kind: "task",
          key: taskRowKey(block.toolUseId),
          toolUseId: block.toolUseId,
          sourceIndex: index,
        });
      }
      index += 1;
      continue;
    }

    const entries: LiveTranscriptToolEntry[] = [];
    let endIndex = index;
    while (endIndex < contentBlocks.length) {
      const nextBlock = contentBlocks[endIndex];
      if (!nextBlock || nextBlock.type !== "tool_use") {
        break;
      }
      if (!shouldHideToolCall(nextBlock.toolCall)) {
        entries.push({ block: nextBlock, index: endIndex });
      }
      endIndex += 1;
    }

    if (entries.length > 0) {
      rows.push({
        kind: "tool_group",
        key: liveToolGroupKey(entries),
        entries,
        count: entries.length,
        sourceIndex: entries[0]!.index,
      });
    }

    index = endIndex;
  }

  return rows;
}
