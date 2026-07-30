import type { StreamingContentBlock, StreamingTask } from "@/types/streaming-task";
import type { ToolCall } from "./ToolCallIndicator";

export type StreamingToolUseBlock = Extract<StreamingContentBlock, { type: "tool_use" }>;

export interface LiveTranscriptToolEntry {
  block: StreamingToolUseBlock;
  index: number;
}

export interface LiveTranscriptTaskEntry {
  toolUseId: string;
  index: number;
  receivedAt?: number;
}
export type StreamingThinkingBlock = Extract<StreamingContentBlock, { type: "thinking" }>;

export type LiveTranscriptRow =
  | {
      kind: "thinking";
      key: string;
      block: StreamingThinkingBlock;
      index: number;
      sourceIndex: number;
      receivedAt?: number;
    }
  | {
      kind: "text";
      key: string;
      text: string;
      sourceIndex: number;
      receivedAt?: number;
    }
  | {
      kind: "task";
      key: string;
      toolUseId: string;
      sourceIndex: number;
      receivedAt?: number;
    }
  | {
      kind: "tool_call";
      key: string;
      block: StreamingToolUseBlock;
      index: number;
      toolCall: ToolCall;
      receivedAt?: number;
    }
  | {
      kind: "tool_group";
      key: string;
      entries: LiveTranscriptToolEntry[];
      taskEntries: LiveTranscriptTaskEntry[];
      count: number;
      sourceIndex: number;
      receivedAt?: number;
    };

export type ShouldHideLiveToolCall = (toolCall: ToolCall) => boolean;
export type ShouldHideLiveTask = (task: StreamingTask) => boolean;

const LIVE_THINKING_GROUP_KEY_PREFIX = "streaming-thinking:";

function blockKeyPart(block: StreamingContentBlock, index: number): string {
  const seq = "seq" in block ? block.seq : undefined;
  return seq != null ? `seq-${seq}` : `idx-${index}`;
}

function textRowKey(
  block: Extract<StreamingContentBlock, { type: "text" }>,
  index: number,
): string {
  const keyPart = block.blockIndex != null
    ? `block-${block.blockIndex}`
    : blockKeyPart(block, index);
  return `streaming-text:${keyPart}`;
}

export function liveThinkingGroupKey(block: StreamingThinkingBlock, index: number): string {
  return `${LIVE_THINKING_GROUP_KEY_PREFIX}${block.blockIndex ?? blockKeyPart(block, index)}`;
}

export function isLiveThinkingGroupKey(groupKey: string): boolean {
  return groupKey.startsWith(LIVE_THINKING_GROUP_KEY_PREFIX);
}

export type ThinkingGroupIntent = "expanded" | "collapsed";

/**
 * Sole owner of automatic thinking-group expansion: the latest running group is
 * open and every other one is closed. A recorded user intent always wins, so a
 * manual collapse is not undone by the next streaming delta. Returns `current`
 * unchanged when nothing moved, keeping the Set identity stable across deltas.
 */
export function synchronizeThinkingGroupExpansion(
  current: Set<string>,
  rows: LiveTranscriptRow[],
  intentByGroupKey: ReadonlyMap<string, ThinkingGroupIntent>,
): Set<string> {
  const thinkingRows = rows.filter((row) => row.kind === "thinking");
  const latestRunningThinking = [...thinkingRows].reverse().find((row) => !row.block.isSettled);
  let next = current;

  for (const row of thinkingRows) {
    const groupKey = liveThinkingGroupKey(row.block, row.index);
    const intent = intentByGroupKey.get(groupKey);
    const shouldExpand = intent === "expanded" || (
      intent === undefined && latestRunningThinking === row
    );
    if (current.has(groupKey) === shouldExpand) {
      continue;
    }
    if (next === current) {
      next = new Set(current);
    }
    if (shouldExpand) {
      next.add(groupKey);
    } else {
      next.delete(groupKey);
    }
  }

  return next;
}

export function liveToolGroupKey(
  entries: LiveTranscriptToolEntry[],
  taskEntries: LiveTranscriptTaskEntry[] = [],
): string {
  const firstTool = entries[0];
  const firstTask = taskEntries[0];
  if (!firstTool && !firstTask) {
    return "streaming-tool-group:empty";
  }
  const firstKey = firstTool
    ? firstTool.block.toolCall.id || firstTool.block.seq || firstTool.index
    : firstTask!.toolUseId || firstTask!.index;
  return [
    "streaming-tool-group",
    firstKey,
  ].join(":");
}

export function buildLiveTranscriptRows(
  contentBlocks: StreamingContentBlock[],
  streamingTasks: Map<string, StreamingTask> | undefined,
  shouldHideToolCall: ShouldHideLiveToolCall = () => false,
  shouldHideTask: ShouldHideLiveTask = () => false,
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
          ...(block.receivedAt != null ? { receivedAt: block.receivedAt } : {}),
        });
      }
      index += 1;
      continue;
    }
    if (block.type === "thinking") {
      rows.push({
        kind: "thinking",
        key: liveThinkingGroupKey(block, index),
        block,
        index,
        sourceIndex: index,
        ...(block.receivedAt != null ? { receivedAt: block.receivedAt } : {}),
      });
      index += 1;
      continue;
    }

    const entries: LiveTranscriptToolEntry[] = [];
    const taskEntries: LiveTranscriptTaskEntry[] = [];
    let endIndex = index;
    while (endIndex < contentBlocks.length) {
      const nextBlock = contentBlocks[endIndex];
      if (!nextBlock || nextBlock.type === "text") {
        break;
      }
      if (nextBlock.type === "tool_use" && !shouldHideToolCall(nextBlock.toolCall)) {
        entries.push({ block: nextBlock, index: endIndex });
      } else if (nextBlock.type === "task") {
        const task = streamingTasks?.get(nextBlock.toolUseId);
        if (!task || shouldHideTask(task)) {
          endIndex += 1;
          continue;
        }
        taskEntries.push({
          toolUseId: nextBlock.toolUseId,
          index: endIndex,
          ...(nextBlock.receivedAt != null ? { receivedAt: nextBlock.receivedAt } : {}),
        });
      }
      endIndex += 1;
    }

    if (entries.length > 0 || taskEntries.length > 0) {
      const firstEntry = [...entries, ...taskEntries].sort((left, right) => left.index - right.index)[0]!;
      const receivedAt = "block" in firstEntry
        ? firstEntry.block.receivedAt
        : firstEntry.receivedAt;
      rows.push({
        kind: "tool_group",
        key: liveToolGroupKey(entries, taskEntries),
        entries,
        taskEntries,
        count: entries.length + taskEntries.length,
        sourceIndex: firstEntry.index,
        ...(receivedAt != null ? { receivedAt } : {}),
      });
    }

    index = endIndex;
  }

  return rows;
}
