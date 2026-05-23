import type { StreamingContentBlock, StreamingTask } from "@/types/streaming-task";

/** Keep the live streaming row bounded while preserving recent Codex block boundaries. */
export const STREAMING_TEXT_BLOCK_TAIL_LIMIT = 40;

/** Maximum live streaming blocks rendered in the scrollable transcript tail. */
export const STREAMING_TRANSCRIPT_BLOCK_TAIL_LIMIT = 40;

export interface StreamingTranscriptWindow {
  contentBlocks: StreamingContentBlock[];
  hiddenBlockCount: number;
  sourceBlockCount: number;
}

export const EMPTY_STREAMING_TRANSCRIPT_WINDOW: StreamingTranscriptWindow = {
  contentBlocks: [],
  hiddenBlockCount: 0,
  sourceBlockCount: 0,
};

function compactStreamingTextRun(
  run: Extract<StreamingContentBlock, { type: "text" }>[]
): StreamingContentBlock[] {
  if (run.length <= STREAMING_TEXT_BLOCK_TAIL_LIMIT) {
    return run;
  }

  const compactedBlocks = run.slice(0, -STREAMING_TEXT_BLOCK_TAIL_LIMIT);
  const recentBlocks = run.slice(-STREAMING_TEXT_BLOCK_TAIL_LIMIT);
  const compactedText = compactedBlocks
    .map((block) => block.text.trimEnd())
    .filter((text) => text.length > 0)
    .join("\n\n");

  if (compactedText.length === 0) {
    return recentBlocks;
  }

  const firstBlock = compactedBlocks[0];
  const compactedBlock: Extract<StreamingContentBlock, { type: "text" }> =
    firstBlock?.seq != null
      ? { type: "text", text: compactedText, seq: firstBlock.seq }
      : { type: "text", text: compactedText };

  return [compactedBlock, ...recentBlocks];
}

function compactStreamingTextBlocks(
  contentBlocks: StreamingContentBlock[]
): StreamingContentBlock[] {
  if (contentBlocks.length <= STREAMING_TEXT_BLOCK_TAIL_LIMIT) {
    return contentBlocks;
  }

  const compacted: StreamingContentBlock[] = [];
  let textRun: Extract<StreamingContentBlock, { type: "text" }>[] = [];

  const flushTextRun = () => {
    if (textRun.length === 0) {
      return;
    }
    compacted.push(...compactStreamingTextRun(textRun));
    textRun = [];
  };

  for (const block of contentBlocks) {
    if (block.type === "text") {
      textRun.push(block);
      continue;
    }
    flushTextRun();
    compacted.push(block);
  }

  flushTextRun();
  return compacted;
}

function isActiveStreamingTaskBlock(
  block: StreamingContentBlock,
  streamingTasks: Map<string, StreamingTask> | undefined
): boolean {
  if (block.type !== "task") {
    return false;
  }
  return streamingTasks?.get(block.toolUseId)?.status === "running";
}

export function buildStreamingTranscriptWindow(
  contentBlocks: StreamingContentBlock[],
  streamingTasks: Map<string, StreamingTask> | undefined
): StreamingTranscriptWindow {
  if (contentBlocks.length === 0) {
    return EMPTY_STREAMING_TRANSCRIPT_WINDOW;
  }

  if (contentBlocks.length <= STREAMING_TRANSCRIPT_BLOCK_TAIL_LIMIT) {
    return {
      contentBlocks: compactStreamingTextBlocks(contentBlocks),
      hiddenBlockCount: 0,
      sourceBlockCount: contentBlocks.length,
    };
  }

  const keepIndexes = new Set<number>();
  const tailStart = Math.max(0, contentBlocks.length - STREAMING_TRANSCRIPT_BLOCK_TAIL_LIMIT);
  for (let index = tailStart; index < contentBlocks.length; index += 1) {
    keepIndexes.add(index);
  }

  contentBlocks.forEach((block, index) => {
    if (isActiveStreamingTaskBlock(block, streamingTasks)) {
      keepIndexes.add(index);
    }
  });

  const selectedBlocks = contentBlocks.filter((_, index) => keepIndexes.has(index));
  return {
    contentBlocks: compactStreamingTextBlocks(selectedBlocks),
    hiddenBlockCount: Math.max(0, contentBlocks.length - keepIndexes.size),
    sourceBlockCount: contentBlocks.length,
  };
}

export function getNextStreamingTranscriptWindow(
  previousWindow: StreamingTranscriptWindow,
  liveWindow: StreamingTranscriptWindow,
  isFollowingLiveTail: boolean
): StreamingTranscriptWindow {
  if (liveWindow.sourceBlockCount === 0) {
    return previousWindow.sourceBlockCount === 0
      ? previousWindow
      : EMPTY_STREAMING_TRANSCRIPT_WINDOW;
  }
  if (!isFollowingLiveTail) {
    return previousWindow;
  }
  return liveWindow;
}
