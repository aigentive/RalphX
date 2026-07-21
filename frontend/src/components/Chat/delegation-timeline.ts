import type { ToolCall } from "./ToolCallIndicator";
import type { ChatMessageData } from "./ChatMessageList";
import {
  extractDelegationMetadata,
  isDelegationControlToolCall,
  isDelegationStartToolCall,
  mergeDelegationContentBlocks,
  mergeDelegationToolCalls,
} from "./delegation-tool-calls";

export function persistedTimelineToolCall(message: ChatMessageData): ToolCall | null {
  const block = message.contentBlocks?.[0];
  if (!block || block.type !== "tool_use" || !block.name) {
    return null;
  }
  const matching = block.id
    ? message.toolCalls?.find((toolCall) => toolCall.id === block.id)
    : undefined;
  const toolCall: ToolCall = {
    id: block.id || matching?.id || message.id,
    name: block.name || matching?.name || "unknown",
    arguments: block.arguments ?? matching?.arguments ?? {},
    result: block.result ?? matching?.result,
  };
  const diffContext = block.diffContext ?? matching?.diffContext;
  if (diffContext) {
    toolCall.diffContext = diffContext;
  }
  return toolCall;
}

export function foldDelegationTimelineMessages(
  messages: ChatMessageData[],
): ChatMessageData[] {
  const startIndexByJobId = new Map<string, number>();
  messages.forEach((message, index) => {
    const toolCall = persistedTimelineToolCall(message);
    if (!toolCall || !isDelegationStartToolCall(toolCall.name)) return;
    const jobId = extractDelegationMetadata(toolCall.arguments, toolCall.result).jobId;
    if (jobId) startIndexByJobId.set(jobId, index);
  });

  const suppressed = new Set<number>();
  const folded = [...messages];
  messages.forEach((message, index) => {
    const control = persistedTimelineToolCall(message);
    if (!control || !isDelegationControlToolCall(control.name)) return;
    const jobId = extractDelegationMetadata(control.arguments, control.result).jobId;
    const startIndex = jobId ? startIndexByJobId.get(jobId) : undefined;
    if (startIndex == null || startIndex === index) {
      folded[index] = {
        ...message,
        contentBlocks: mergeDelegationContentBlocks(message.contentBlocks ?? []),
        toolCalls: mergeDelegationToolCalls(message.toolCalls ?? [control]),
      };
      return;
    }

    const start = folded[startIndex];
    if (!start) return;
    const startToolCall = persistedTimelineToolCall(start);
    folded[startIndex] = {
      ...start,
      contentBlocks: mergeDelegationContentBlocks([
        ...(start.contentBlocks ?? []),
        ...(message.contentBlocks ?? []),
      ]),
      toolCalls: mergeDelegationToolCalls([
        ...(start.toolCalls ?? (startToolCall ? [startToolCall] : [])),
        ...(message.toolCalls ?? [control]),
      ]),
    };
    suppressed.add(index);
  });

  return folded.filter((_message, index) => !suppressed.has(index));
}
