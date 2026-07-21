import type { ChatMessageResponse } from "@/api/chat";
import type { StreamingTask } from "@/types/streaming-task";
import type { ToolCall } from "./ToolCallIndicator";
import { normalizeToolCallTranscriptPayload } from "./tool-call-transcript";
import { parseToolResultId } from "./delegation-tool-calls";

export type TaskCardTranscriptBlock =
  | { type: "text"; text: string }
  | { type: "tool_call"; toolCall: ToolCall }
  | { type: "activity"; label: string };

export interface TaskCardTranscriptEntry {
  id: string;
  role: "user" | "assistant";
  speakerLabel?: string;
  blocks: TaskCardTranscriptBlock[];
}

function normalizeSpeakerLabel(
  role: string,
  sender: string | null | undefined,
): string {
  if (sender) return sender;
  return role === "user" ? "User" : role;
}

function makeTextBlock(text: string | undefined): TaskCardTranscriptBlock | null {
  if (!text) return null;
  const trimmed = text.trim();
  return trimmed ? { type: "text", text: trimmed } : null;
}

export function buildTaskCardTranscriptEntryFromToolCall({
  entryId,
  bodyText,
  childToolCalls,
  speakerLabel,
}: {
  entryId: string;
  bodyText?: string | undefined;
  childToolCalls: ToolCall[];
  speakerLabel?: string | undefined;
}): TaskCardTranscriptEntry {
  const blocks: TaskCardTranscriptBlock[] = childToolCalls.map((toolCall) => ({
    type: "tool_call",
    toolCall,
  }));
  const textBlock = makeTextBlock(bodyText);
  if (textBlock) {
    blocks.push(textBlock);
  }

  return {
    id: entryId,
    role: "assistant",
    ...(speakerLabel ? { speakerLabel } : {}),
    blocks,
  };
}

export function buildTaskCardTranscriptEntryFromStreamingTask(
  task: StreamingTask,
): TaskCardTranscriptEntry {
  const blocks: TaskCardTranscriptBlock[] = task.childToolCalls
    .filter((toolCall) => parseToolResultId(toolCall.name) == null)
    .map((toolCall) => ({
      type: "tool_call" as const,
      toolCall,
    }));

  if (task.status === "running") {
    blocks.push({ type: "activity", label: "Working…" });
  }

  const textBlock = makeTextBlock(task.textOutput);
  if (textBlock) {
    blocks.push(textBlock);
  }

  return {
    id: task.toolUseId,
    role: "assistant",
    blocks,
  };
}

export function buildTaskCardTranscriptEntriesFromConversation(
  messages: ChatMessageResponse[],
): TaskCardTranscriptEntry[] {
  const entries: Array<TaskCardTranscriptEntry | null> = messages
    .map((message) => {
      const { contentBlocks, toolCalls } = normalizeToolCallTranscriptPayload({
        contentBlocks: message.contentBlocks,
        toolCalls: message.toolCalls,
      });

      const blocks: TaskCardTranscriptBlock[] = [];

      if (contentBlocks.length > 0) {
        for (const block of contentBlocks) {
          if (block.type === "text" && block.text) {
            blocks.push({ type: "text", text: block.text });
            continue;
          }
          if (block.type === "tool_use" && block.name) {
            blocks.push({
              type: "tool_call",
              toolCall: {
                id: block.id ?? `${message.id}-tool`,
                name: block.name,
                arguments: block.arguments ?? {},
                ...(block.result != null ? { result: block.result } : {}),
                ...(block.parentToolUseId
                  ? { parentToolUseId: block.parentToolUseId }
                  : {}),
              },
            });
          }
        }
      } else {
        const textBlock = makeTextBlock(message.content);
        if (textBlock) {
          blocks.push(textBlock);
        }
        for (const toolCall of toolCalls) {
          blocks.push({ type: "tool_call", toolCall });
        }
      }

      if (blocks.length === 0) {
        return null;
      }

      return {
        id: message.id,
        role: message.role === "user" ? "user" : "assistant",
        speakerLabel: normalizeSpeakerLabel(message.role, message.sender),
        blocks,
      };
    });

  return entries.filter((entry): entry is TaskCardTranscriptEntry => entry != null);
}
