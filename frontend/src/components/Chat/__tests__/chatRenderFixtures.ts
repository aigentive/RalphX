import type { ChatMessage as IdeationChatMessage } from "@/types/ideation";
import type { MessageAttachment } from "../MessageAttachments";
import type { ContentBlockItem, MessageItemProps } from "../MessageItem";
import type { ToolCall } from "../ToolCallIndicator";

const DEFAULT_TIMESTAMP = "2026-04-10T07:00:00Z";

type ToolCallOverrides = Partial<ToolCall> & {
  arguments?: unknown;
};

type ChatMessageOverrides = Partial<IdeationChatMessage> & {
  toolCalls?: unknown;
  contentBlocks?: unknown;
};

export function makeToolCall(
  name = "custom_tool",
  {
    id = `${name}-1`,
    arguments: args = {},
    result,
    error,
    diffContext,
    stats,
    resultPreviewTruncated,
    resultPreviewOriginalBytes,
    resultPreviewLineCount,
    resultPreviewOmittedLines,
    argumentsPreviewTruncated,
    argumentsPreviewOriginalBytes,
    argumentsPreviewLineCount,
    argumentsPreviewOmittedLines,
    diffPreview,
    detailRef,
  }: ToolCallOverrides = {},
): ToolCall {
  return {
    id,
    name,
    arguments: args,
    ...(result !== undefined ? { result } : {}),
    ...(error !== undefined ? { error } : {}),
    ...(diffContext !== undefined ? { diffContext } : {}),
    ...(stats !== undefined ? { stats } : {}),
    ...(resultPreviewTruncated !== undefined ? { resultPreviewTruncated } : {}),
    ...(resultPreviewOriginalBytes !== undefined ? { resultPreviewOriginalBytes } : {}),
    ...(resultPreviewLineCount !== undefined ? { resultPreviewLineCount } : {}),
    ...(resultPreviewOmittedLines !== undefined ? { resultPreviewOmittedLines } : {}),
    ...(argumentsPreviewTruncated !== undefined ? { argumentsPreviewTruncated } : {}),
    ...(argumentsPreviewOriginalBytes !== undefined ? { argumentsPreviewOriginalBytes } : {}),
    ...(argumentsPreviewLineCount !== undefined ? { argumentsPreviewLineCount } : {}),
    ...(argumentsPreviewOmittedLines !== undefined ? { argumentsPreviewOmittedLines } : {}),
    ...(diffPreview !== undefined ? { diffPreview } : {}),
    ...(detailRef !== undefined ? { detailRef } : {}),
  };
}

export function makeContentText(text: string): ContentBlockItem {
  return {
    type: "text",
    text,
  };
}

export function makeContentToolUse(
  name: string,
  {
    id = `${name}-block-1`,
    arguments: args = {},
    result,
    diffContext,
  }: Partial<ContentBlockItem> = {},
): ContentBlockItem {
  return {
    type: "tool_use",
    id,
    name,
    arguments: args,
    ...(result !== undefined ? { result } : {}),
    ...(diffContext !== undefined ? { diffContext } : {}),
  };
}

/** Build the canonical content-block form of an in-flight or finalized Edit. */
export function makeEditContentToolUse(
  id: string,
  {
    filePath,
    oldContent,
    oldString,
    newString,
  }: {
    filePath: string;
    oldContent: string;
    oldString: string;
    newString: string;
  },
): ContentBlockItem {
  return makeContentToolUse("Edit", {
    id,
    arguments: {
      file_path: filePath,
      old_string: oldString,
      new_string: newString,
    },
    diffContext: {
      filePath,
      oldContent,
      oldFileExists: true,
    },
  });
}

export function makeMessageAttachment(
  overrides: Partial<MessageAttachment> = {},
): MessageAttachment {
  return {
    id: overrides.id ?? "attachment-1",
    fileName: overrides.fileName ?? "test.txt",
    fileSize: overrides.fileSize ?? 1024,
    mimeType: overrides.mimeType ?? "text/plain",
  };
}

export function makeMessageItemProps(
  overrides: Partial<MessageItemProps> = {},
): MessageItemProps {
  return {
    role: overrides.role ?? "assistant",
    content: overrides.content ?? "Hello world",
    createdAt: overrides.createdAt ?? DEFAULT_TIMESTAMP,
    ...(overrides.toolCalls !== undefined ? { toolCalls: overrides.toolCalls } : {}),
    ...(overrides.contentBlocks !== undefined ? { contentBlocks: overrides.contentBlocks } : {}),
    ...(overrides.attachments !== undefined ? { attachments: overrides.attachments } : {}),
    ...(overrides.providerHarness !== undefined ? { providerHarness: overrides.providerHarness } : {}),
    ...(overrides.providerSessionId !== undefined
      ? { providerSessionId: overrides.providerSessionId }
      : {}),
    ...(overrides.upstreamProvider !== undefined ? { upstreamProvider: overrides.upstreamProvider } : {}),
    ...(overrides.providerProfile !== undefined ? { providerProfile: overrides.providerProfile } : {}),
    ...(overrides.logicalModel !== undefined ? { logicalModel: overrides.logicalModel } : {}),
    ...(overrides.effectiveModelId !== undefined ? { effectiveModelId: overrides.effectiveModelId } : {}),
    ...(overrides.logicalEffort !== undefined ? { logicalEffort: overrides.logicalEffort } : {}),
    ...(overrides.effectiveEffort !== undefined ? { effectiveEffort: overrides.effectiveEffort } : {}),
    ...(overrides.inputTokens !== undefined ? { inputTokens: overrides.inputTokens } : {}),
    ...(overrides.outputTokens !== undefined ? { outputTokens: overrides.outputTokens } : {}),
    ...(overrides.cacheCreationTokens !== undefined
      ? { cacheCreationTokens: overrides.cacheCreationTokens }
      : {}),
    ...(overrides.cacheReadTokens !== undefined ? { cacheReadTokens: overrides.cacheReadTokens } : {}),
    ...(overrides.estimatedUsd !== undefined ? { estimatedUsd: overrides.estimatedUsd } : {}),
  };
}

export function makeIdeationChatMessage(
  overrides: ChatMessageOverrides = {},
): IdeationChatMessage {
  const rawToolCalls = overrides.toolCalls;
  const rawContentBlocks = overrides.contentBlocks;

  return {
    id: overrides.id ?? "msg-1",
    sessionId: overrides.sessionId ?? "session-1",
    projectId: overrides.projectId ?? "project-1",
    taskId: overrides.taskId ?? null,
    role: overrides.role ?? "orchestrator",
    content: overrides.content ?? "Hello from RalphX",
    metadata: overrides.metadata ?? null,
    parentMessageId: overrides.parentMessageId ?? null,
    conversationId: overrides.conversationId ?? null,
    toolCalls:
      rawToolCalls === undefined
        ? null
        : typeof rawToolCalls === "string"
          ? rawToolCalls
          : JSON.stringify(rawToolCalls),
    contentBlocks:
      rawContentBlocks === undefined
        ? null
        : typeof rawContentBlocks === "string"
          ? rawContentBlocks
          : JSON.stringify(rawContentBlocks),
    createdAt: overrides.createdAt ?? DEFAULT_TIMESTAMP,
  };
}
