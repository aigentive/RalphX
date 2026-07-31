import type { ChatMessageResponse } from "@/api/chat";
import type { ToolCall } from "@/components/Chat/ToolCallIndicator";
import type { StreamingContentBlock } from "@/types/streaming-task";

/**
 * Project durable streaming timeline rows into the same ordered block shape as
 * live events. Recovery uses these rows as anchors before merging the
 * cumulative active-state cache, so persisted text/tool interleave stays
 * canonical across a remount.
 */
export function projectPersistedStreamingContentBlocks(
  messages: readonly ChatMessageResponse[],
  activeRunId?: string,
): StreamingContentBlock[] {
  let textBlockIndex = 0;
  let thinkingBlockIndex = 0;
  return messages.flatMap((message) => {
    if (message.timelineStatus !== "streaming") return [];
    if (activeRunId != null && message.runId != null && message.runId !== activeRunId) return [];
    const seq = message.timelineSequence ?? undefined;
    return (message.contentBlocks ?? []).flatMap((block): StreamingContentBlock[] => {
      if (block.type === "text") {
        const legacyBlockIndex = textBlockIndex++;
        return [{
          type: "text",
          text: block.text ?? "",
          blockIndex: message.timelineBlockIndex ?? legacyBlockIndex,
          ...(seq != null ? { seq } : {}),
        }];
      }
      if (block.type === "thinking") {
        const text = block.text ?? "";
        if (text.length === 0) return [];
        const legacyBlockIndex = thinkingBlockIndex++;
        return [{
          type: "thinking",
          text,
          blockIndex: message.timelineBlockIndex ?? legacyBlockIndex,
          ...(block.durationMs != null ? { durationMs: block.durationMs } : {}),
          ...(block.isSettled != null ? { isSettled: block.isSettled } : {}),
          ...(seq != null ? { seq } : {}),
        }];
      }
      if (block.type !== "tool_use") return [];

      const persistedToolCall = message.toolCalls?.find((toolCall) => toolCall.id === block.id);
      const toolCall: ToolCall = persistedToolCall ?? {
        id: block.id ?? `tool:${block.name ?? "unknown"}`,
        name: block.name ?? "unknown",
        arguments: block.arguments ?? {},
        ...(block.result !== undefined ? { result: block.result } : {}),
        ...(block.diffContext !== undefined ? { diffContext: block.diffContext } : {}),
      };
      return [{ type: "tool_use", toolCall, ...(seq != null ? { seq } : {}) }];
    });
  });
}
