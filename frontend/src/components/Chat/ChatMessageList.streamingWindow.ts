import type { StreamingContentBlock, StreamingTask } from "@/types/streaming-task";

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
      contentBlocks,
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
    contentBlocks: selectedBlocks,
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
  if (previousWindow.sourceBlockCount === 0) {
    return liveWindow;
  }
  if (!isFollowingLiveTail) {
    return previousWindow;
  }
  return liveWindow;
}
