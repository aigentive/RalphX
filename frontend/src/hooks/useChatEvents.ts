/**
 * useChatEvents — Unified event subscription for all chat panels
 *
 * Merges:
 * - useIntegratedChatEvents (streaming text, subagent routing, diff views)
 * - Event handling from useChatPanelHandlers (tool calls, run lifecycle, queue)
 *
 * Uses registry feature flags to conditionally enable subscriptions:
 * - supportsStreamingText → agent:chunk
 * - supportsSubagentTasks → agent:task_started/completed, parent_tool_use_id routing
 * - supportsDiffViews → diff_context on tool calls
 *
 * The hook subscribes to events that supplement useAgentEvents (which handles
 * the core lifecycle: run_started, message_created, run_completed, queue_sent,
 * stopped, error, session_recovered). This hook adds streaming UI features.
 */

import { useEffect, useRef, type Dispatch, type SetStateAction } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { useEventBus } from "@/providers/EventProvider";
import {
  chatKeys,
  getCachedConversationMessages,
  invalidateConversationDataQueries,
  upsertFinalizedMessageIntoConversationCache,
  upsertRenderReadyMessageIntoConversationCache,
  type RenderReadyMessageCreatedPayload,
} from "@/hooks/useChat";
import { conversationStatsKey } from "@/hooks/useConversationStats";
import { getContextConfig } from "@/lib/chat-context-registry";
import { isProviderRole } from "@/lib/chat/provider-role";
import type { ContextType } from "@/types/chat-conversation";
import type { AgentRunCompletedPayload } from "@/types/events";
import type { ToolCall } from "@/components/Chat/ToolCallIndicator";
import type { ChatMessageResponse } from "@/api/chat";
import { FileDiffSchema, transformFileDiff, type FileDiff } from "@/api/diff";
import type { ContentBlockItem } from "@/components/Chat/MessageItem";
import type { StreamingTask, StreamingContentBlock } from "@/types/streaming-task";
import type { Unsubscribe } from "@/lib/event-bus";
import { useChatStore } from "@/stores/chatStore";
import { canonicalizeToolName } from "@/components/Chat/tool-widgets/tool-name";
import {
  extractDelegationMetadata,
  type ReconcileDelegationTaskInput,
  buildDelegationLifecycleTask,
  findDelegationTaskKey,
  isDelegationControlToolCall,
  isDelegationStartToolCall,
  mergeDelegationTaskMetadata,
  parseToolResultId,
  reconcileDelegationTaskMap,
  reconcileDelegationTaskMarkers,
} from "@/components/Chat/delegation-tool-calls";

function stableSerialize(value: unknown): string {
  if (value == null || typeof value !== "object") {
    return JSON.stringify(value) ?? String(value);
  }

  if (Array.isArray(value)) {
    return `[${value.map(stableSerialize).join(",")}]`;
  }

  const objectValue = value as Record<string, unknown>;
  return `{${Object.keys(objectValue)
    .sort()
    .map((key) => `${JSON.stringify(key)}:${stableSerialize(objectValue[key])}`)
    .join(",")}}`;
}

function buildStreamingToolCallId(toolName: string, args: unknown): string {
  return `streaming-agent:${canonicalizeToolName(toolName)}:${stableSerialize(args)}`;
}

const STREAMING_RESULT_PREVIEW_MAX_LINES = 10;
const STREAMING_RESULT_PREVIEW_MAX_CHARS = 4_000;

function stringifyToolResultForPreview(result: unknown): string {
  if (typeof result === "string") {
    return result;
  }
  if (Array.isArray(result)) {
    const textItems = result
      .map((item) => {
        if (item != null && typeof item === "object") {
          const text = (item as Record<string, unknown>).text;
          return typeof text === "string" ? text : null;
        }
        return null;
      })
      .filter((text): text is string => text != null);
    if (textItems.length > 0) {
      return textItems.join("\n");
    }
  }
  if (result != null && typeof result === "object") {
    const record = result as Record<string, unknown>;
    for (const key of ["text", "content", "output", "aggregated_output", "aggregatedOutput"]) {
      const value = record[key];
      if (typeof value === "string") {
        return value;
      }
    }
  }

  try {
    return JSON.stringify(result, null, 2);
  } catch {
    return String(result);
  }
}

function buildStreamingResultPreview(result: unknown) {
  const text = stringifyToolResultForPreview(result);
  const lineCount = text.length === 0 ? 0 : text.split(/\r?\n/).length;
  if (
    lineCount <= STREAMING_RESULT_PREVIEW_MAX_LINES
    && Array.from(text).length <= STREAMING_RESULT_PREVIEW_MAX_CHARS
  ) {
    return null;
  }

  let preview = "";
  let previewLines = 0;
  let charCount = 0;
  const lines = text.split(/\r?\n/);
  for (let lineIndex = 0; lineIndex < lines.length && lineIndex < STREAMING_RESULT_PREVIEW_MAX_LINES; lineIndex += 1) {
    if (lineIndex > 0) preview += "\n";
    previewLines += 1;
    for (const ch of lines[lineIndex] ?? "") {
      if (charCount >= STREAMING_RESULT_PREVIEW_MAX_CHARS) {
        break;
      }
      preview += ch;
      charCount += 1;
    }
    if (charCount >= STREAMING_RESULT_PREVIEW_MAX_CHARS) {
      break;
    }
  }

  return {
    result: preview,
    resultPreviewTruncated: true,
    resultPreviewOriginalBytes: text.length,
    resultPreviewLineCount: lineCount,
    resultPreviewOmittedLines: Math.max(0, lineCount - previewLines),
  } satisfies Partial<ToolCall>;
}

type ToolResultPreviewMetadata = {
  result_preview_truncated?: unknown;
  resultPreviewTruncated?: unknown;
  result_preview_original_bytes?: unknown;
  resultPreviewOriginalBytes?: unknown;
  result_preview_line_count?: unknown;
  resultPreviewLineCount?: unknown;
  result_preview_omitted_lines?: unknown;
  resultPreviewOmittedLines?: unknown;
  result_preview_paths?: unknown;
  resultPreviewPaths?: unknown;
  arguments_preview_truncated?: unknown;
  argumentsPreviewTruncated?: unknown;
  arguments_preview_original_bytes?: unknown;
  argumentsPreviewOriginalBytes?: unknown;
  arguments_preview_line_count?: unknown;
  argumentsPreviewLineCount?: unknown;
  arguments_preview_omitted_lines?: unknown;
  argumentsPreviewOmittedLines?: unknown;
  diff_preview?: unknown;
  diffPreview?: unknown;
  detail_ref?: unknown;
  detailRef?: unknown;
};

function getNumberMetadata(
  metadata: ToolResultPreviewMetadata,
  snakeKey: keyof ToolResultPreviewMetadata,
  camelKey: keyof ToolResultPreviewMetadata,
): number | undefined {
  const value = metadata[snakeKey] ?? metadata[camelKey];
  return typeof value === "number" ? value : undefined;
}

function getStringArrayMetadata(
  metadata: ToolResultPreviewMetadata,
  snakeKey: keyof ToolResultPreviewMetadata,
  camelKey: keyof ToolResultPreviewMetadata,
): string[] | undefined {
  const value = metadata[snakeKey] ?? metadata[camelKey];
  return Array.isArray(value) && value.every((item) => typeof item === "string")
    ? value
    : undefined;
}

function normalizeStreamingToolDetailRef(raw: unknown): ToolCall["detailRef"] | undefined {
  if (raw == null || typeof raw !== "object") {
    return undefined;
  }
  const record = raw as Record<string, unknown>;
  const conversationId = record.conversation_id ?? record.conversationId;
  const messageId = record.message_id ?? record.messageId;
  if (typeof conversationId !== "string" || typeof messageId !== "string") {
    return undefined;
  }

  const detailRef: NonNullable<ToolCall["detailRef"]> = { conversationId, messageId };
  const toolCallId = record.tool_call_id ?? record.toolCallId;
  const contentBlockIndex = record.content_block_index ?? record.contentBlockIndex;
  if (typeof toolCallId === "string") {
    detailRef.toolCallId = toolCallId;
  }
  if (typeof contentBlockIndex === "number") {
    detailRef.contentBlockIndex = contentBlockIndex;
  }
  return detailRef;
}

function normalizeStreamingDiffPreview(raw: unknown): FileDiff | undefined {
  const parsed = FileDiffSchema.safeParse(raw);
  return parsed.success ? transformFileDiff(parsed.data) : undefined;
}

function applyBackendToolResultPreviewMetadata(
  toolCall: ToolCall,
  metadata: ToolResultPreviewMetadata,
) {
  toolCall.resultPreviewTruncated = true;

  const originalBytes = getNumberMetadata(
    metadata,
    "result_preview_original_bytes",
    "resultPreviewOriginalBytes",
  );
  const lineCount = getNumberMetadata(
    metadata,
    "result_preview_line_count",
    "resultPreviewLineCount",
  );
  const omittedLines = getNumberMetadata(
    metadata,
    "result_preview_omitted_lines",
    "resultPreviewOmittedLines",
  );
  if (originalBytes != null) toolCall.resultPreviewOriginalBytes = originalBytes;
  if (lineCount != null) toolCall.resultPreviewLineCount = lineCount;
  if (omittedLines != null) toolCall.resultPreviewOmittedLines = omittedLines;
  const previewPaths = getStringArrayMetadata(
    metadata,
    "result_preview_paths",
    "resultPreviewPaths",
  );
  if (previewPaths != null) {
    toolCall.resultPreviewPaths = previewPaths;
  } else {
    delete toolCall.resultPreviewPaths;
  }

  const detailRef = normalizeStreamingToolDetailRef(metadata.detail_ref ?? metadata.detailRef);
  if (detailRef) {
    toolCall.detailRef = detailRef;
  } else if (!toolCall.argumentsPreviewTruncated) {
    delete toolCall.detailRef;
  }
}

function applyBackendToolArgumentPreviewMetadata(
  toolCall: ToolCall,
  metadata: ToolResultPreviewMetadata,
) {
  if (
    metadata.arguments_preview_truncated !== true
    && metadata.argumentsPreviewTruncated !== true
  ) {
    return;
  }

  toolCall.argumentsPreviewTruncated = true;

  const originalBytes = getNumberMetadata(
    metadata,
    "arguments_preview_original_bytes",
    "argumentsPreviewOriginalBytes",
  );
  const lineCount = getNumberMetadata(
    metadata,
    "arguments_preview_line_count",
    "argumentsPreviewLineCount",
  );
  const omittedLines = getNumberMetadata(
    metadata,
    "arguments_preview_omitted_lines",
    "argumentsPreviewOmittedLines",
  );
  if (originalBytes != null) toolCall.argumentsPreviewOriginalBytes = originalBytes;
  if (lineCount != null) toolCall.argumentsPreviewLineCount = lineCount;
  if (omittedLines != null) toolCall.argumentsPreviewOmittedLines = omittedLines;

  const diffPreview = normalizeStreamingDiffPreview(metadata.diff_preview ?? metadata.diffPreview);
  if (diffPreview) {
    toolCall.diffPreview = diffPreview;
  }

  const detailRef = normalizeStreamingToolDetailRef(metadata.detail_ref ?? metadata.detailRef);
  if (detailRef) {
    toolCall.detailRef = detailRef;
  }
}

function applyToolCallResultPreview(
  toolCall: ToolCall,
  result: unknown,
  metadata?: ToolResultPreviewMetadata,
) {
  if (
    metadata
    && (metadata.result_preview_truncated === true || metadata.resultPreviewTruncated === true)
  ) {
    toolCall.result = result;
    applyBackendToolResultPreviewMetadata(toolCall, metadata);
    return;
  }

  const preview = buildStreamingResultPreview(result);
  if (preview) {
    toolCall.result = preview.result;
    toolCall.resultPreviewTruncated = preview.resultPreviewTruncated;
    toolCall.resultPreviewOriginalBytes = preview.resultPreviewOriginalBytes;
    toolCall.resultPreviewLineCount = preview.resultPreviewLineCount;
    toolCall.resultPreviewOmittedLines = preview.resultPreviewOmittedLines;
    delete toolCall.resultPreviewPaths;
    if (!toolCall.argumentsPreviewTruncated) {
      delete toolCall.detailRef;
    }
    return;
  }

  toolCall.result = result;
  delete toolCall.resultPreviewTruncated;
  delete toolCall.resultPreviewOriginalBytes;
  delete toolCall.resultPreviewLineCount;
  delete toolCall.resultPreviewOmittedLines;
  delete toolCall.resultPreviewPaths;
  if (!toolCall.argumentsPreviewTruncated) {
    delete toolCall.detailRef;
  }
}

type AgentMessageCreatedPayload = {
  conversation_id?: string;
  context_id?: string;
  context_type?: string;
  role?: string;
  message_id?: string;
  content?: string;
  created_at?: string;
  metadata?: string | null;
  render_ready?: RenderReadyMessageCreatedPayload | null;
};

function contentBlockFromToolCall(toolCall: ToolCall): ContentBlockItem {
  return {
    type: "tool_use",
    id: toolCall.id,
    name: toolCall.name,
    arguments: toolCall.arguments,
    ...(toolCall.result !== undefined ? { result: toolCall.result } : {}),
    ...(toolCall.resultPreviewTruncated !== undefined
      ? { resultPreviewTruncated: toolCall.resultPreviewTruncated }
      : {}),
    ...(toolCall.resultPreviewOriginalBytes !== undefined
      ? { resultPreviewOriginalBytes: toolCall.resultPreviewOriginalBytes }
      : {}),
    ...(toolCall.resultPreviewLineCount !== undefined
      ? { resultPreviewLineCount: toolCall.resultPreviewLineCount }
      : {}),
    ...(toolCall.resultPreviewOmittedLines !== undefined
      ? { resultPreviewOmittedLines: toolCall.resultPreviewOmittedLines }
      : {}),
    ...(toolCall.argumentsPreviewTruncated !== undefined
      ? { argumentsPreviewTruncated: toolCall.argumentsPreviewTruncated }
      : {}),
    ...(toolCall.argumentsPreviewOriginalBytes !== undefined
      ? { argumentsPreviewOriginalBytes: toolCall.argumentsPreviewOriginalBytes }
      : {}),
    ...(toolCall.argumentsPreviewLineCount !== undefined
      ? { argumentsPreviewLineCount: toolCall.argumentsPreviewLineCount }
      : {}),
    ...(toolCall.argumentsPreviewOmittedLines !== undefined
      ? { argumentsPreviewOmittedLines: toolCall.argumentsPreviewOmittedLines }
      : {}),
    ...(toolCall.diffPreview ? { diffPreview: toolCall.diffPreview } : {}),
    ...(toolCall.detailRef ? { detailRef: toolCall.detailRef } : {}),
    ...(toolCall.parentToolUseId ? { parentToolUseId: toolCall.parentToolUseId } : {}),
    ...(toolCall.diffContext ? { diffContext: toolCall.diffContext } : {}),
  };
}

function buildFinalizedContentBlocks(
  payload: AgentMessageCreatedPayload,
  streamingContentBlocks: StreamingContentBlock[],
  streamingToolCalls: ToolCall[],
): ContentBlockItem[] | null {
  if (streamingContentBlocks.some((block) => block.type === "task")) {
    return null;
  }

  const blocks = streamingContentBlocks
    .filter((block): block is Exclude<StreamingContentBlock, { type: "task" }> => block.type !== "task")
    .map((block): ContentBlockItem | null => {
      if (block.type === "text") {
        return block.text.trim().length > 0 ? { type: "text", text: block.text } : null;
      }
      return contentBlockFromToolCall(block.toolCall);
    })
    .filter((block): block is ContentBlockItem => block != null);

  if (blocks.length > 0) {
    return blocks;
  }

  if (streamingToolCalls.length > 0) {
    return streamingToolCalls.map(contentBlockFromToolCall);
  }

  const content = payload.content ?? "";
  return content.trim().length > 0 ? [{ type: "text", text: content }] : null;
}

function buildFinalizedMessageForCache(
  payload: AgentMessageCreatedPayload,
  contentBlocks: ContentBlockItem[],
): ChatMessageResponse | null {
  if (!payload.message_id || !payload.conversation_id || !payload.role) {
    return null;
  }

  const toolCalls = contentBlocks
    .filter((block): block is ContentBlockItem & { type: "tool_use" } => block.type === "tool_use")
    .map((block): ToolCall => ({
      id: block.id ?? `tool:${block.name ?? "unknown"}`,
      name: block.name ?? "unknown",
      arguments: block.arguments ?? {},
      ...(block.result !== undefined ? { result: block.result } : {}),
      ...(block.resultPreviewTruncated !== undefined
        ? { resultPreviewTruncated: block.resultPreviewTruncated }
        : {}),
      ...(block.resultPreviewOriginalBytes !== undefined
        ? { resultPreviewOriginalBytes: block.resultPreviewOriginalBytes }
        : {}),
      ...(block.resultPreviewLineCount !== undefined
        ? { resultPreviewLineCount: block.resultPreviewLineCount }
        : {}),
      ...(block.resultPreviewOmittedLines !== undefined
        ? { resultPreviewOmittedLines: block.resultPreviewOmittedLines }
        : {}),
      ...(block.argumentsPreviewTruncated !== undefined
        ? { argumentsPreviewTruncated: block.argumentsPreviewTruncated }
        : {}),
      ...(block.argumentsPreviewOriginalBytes !== undefined
        ? { argumentsPreviewOriginalBytes: block.argumentsPreviewOriginalBytes }
        : {}),
      ...(block.argumentsPreviewLineCount !== undefined
        ? { argumentsPreviewLineCount: block.argumentsPreviewLineCount }
        : {}),
      ...(block.argumentsPreviewOmittedLines !== undefined
        ? { argumentsPreviewOmittedLines: block.argumentsPreviewOmittedLines }
        : {}),
      ...(block.diffPreview ? { diffPreview: block.diffPreview } : {}),
      ...(block.detailRef ? { detailRef: block.detailRef } : {}),
      ...(block.parentToolUseId ? { parentToolUseId: block.parentToolUseId } : {}),
      ...(block.diffContext ? { diffContext: block.diffContext } : {}),
    }));
  const textContent =
    payload.content ??
    contentBlocks
      .filter((block) => block.type === "text")
      .map((block) => block.text ?? "")
      .join("");

  return {
    id: payload.message_id,
    sessionId: null,
    projectId: null,
    taskId: null,
    role: payload.role,
    content: textContent,
    metadata: payload.metadata ?? null,
    parentMessageId: null,
    conversationId: payload.conversation_id,
    toolCalls: toolCalls.length > 0 ? toolCalls : null,
    contentBlocks,
    sender: null,
    createdAt: payload.created_at ?? new Date().toISOString(),
  };
}

// ============================================================================
// Types
// ============================================================================

interface UseChatEventsProps {
  activeConversationId: string | null;
  activeAgentRunId?: string | null;
  contextId: string | null;
  contextType: ContextType | null;
  streamingToolCalls?: ToolCall[];
  streamingContentBlocks?: StreamingContentBlock[];
  streamingTasks?: Map<string, StreamingTask>;
  setStreamingToolCalls: Dispatch<SetStateAction<ToolCall[]>>;
  setStreamingContentBlocks: Dispatch<SetStateAction<StreamingContentBlock[]>>;
  setStreamingTasks: Dispatch<SetStateAction<Map<string, StreamingTask>>>;
  /** Setter to mark the conversation as finalizing (between message_created and query refetch) */
  setIsFinalizing: Dispatch<SetStateAction<boolean>>;
  /** Store key for writing tool call start times (storeKey → toolCallId → timestamp) */
  storeKey?: string;
}

// ============================================================================
// Hook
// ============================================================================

export function useChatEvents({
  activeConversationId,
  activeAgentRunId,
  contextId,
  contextType,
  streamingToolCalls = [],
  streamingContentBlocks = [],
  streamingTasks = new Map(),
  setStreamingToolCalls,
  setStreamingContentBlocks,
  setStreamingTasks,
  setIsFinalizing,
  storeKey,
}: UseChatEventsProps) {
  const bus = useEventBus();
  const queryClient = useQueryClient();
  const streamingToolCallsRef = useRef(streamingToolCalls);
  const streamingContentBlocksRef = useRef(streamingContentBlocks);
  const streamingTasksRef = useRef(streamingTasks);
  const lastChunkSeqRef = useRef<number | null>(null);

  useEffect(() => {
    streamingToolCallsRef.current = streamingToolCalls;
  }, [streamingToolCalls]);

  useEffect(() => {
    streamingContentBlocksRef.current = streamingContentBlocks;
  }, [streamingContentBlocks]);

  useEffect(() => {
    streamingTasksRef.current = streamingTasks;
  }, [streamingTasks]);

  // Resolve feature flags from registry
  const config = contextType ? getContextConfig(contextType) : null;
  const supportsStreamingText = config?.supportsStreamingText ?? false;
  const supportsSubagentTasks = config?.supportsSubagentTasks ?? false;

  // ── Finalization two-effect contract ────────────────────────────────────────
  // `activeCancelFnsRef` is a ref (not a local variable) so finalization watchers
  // survive effect re-runs triggered by unrelated deps (e.g., user sends a message).
  // The main subscription effect NEVER cancels finalization on cleanup.
  // Only the dedicated `[activeConversationId, contextId]` effect below cancels on
  // genuine context switch — prevents isFinalizing from being interrupted mid-stream.
  // ❌ Do NOT add activeCancelFnsRef cleanup to the main effect. ❌ Do NOT add unrelated
  // deps to the context-switch effect (it must only fire on real navigation).
  // ────────────────────────────────────────────────────────────────────────────
  const activeCancelFnsRef = useRef<Array<() => void>>([]);

  // Genuine context switch: cancel pending finalizations when conversation/context changes.
  useEffect(() => {
    return () => {
      activeCancelFnsRef.current.slice().forEach(fn => fn());
      activeCancelFnsRef.current = [];
    };
  }, [activeConversationId, contextId]);

  useEffect(() => {
    // Clear streaming state immediately when conversation changes to ensure clean slate
    // This runs BEFORE subscribing to new events, preventing stale state from previous conversation
    lastChunkSeqRef.current = null;
    setStreamingToolCalls(prev => prev.length === 0 ? prev : []);
    setStreamingContentBlocks(prev => prev.length === 0 ? prev : []);
    setStreamingTasks(prev => prev.size === 0 ? prev : new Map());

    const unsubscribes: Unsubscribe[] = [];

    // Helper: check if event matches current context
    const isRelevant = (payload: {
      conversation_id?: string;
      context_id?: string;
      run_id?: string | null;
    }) =>
      payload.conversation_id === activeConversationId &&
      (!contextId || payload.context_id === contextId) &&
      (!activeAgentRunId || !payload.run_id || payload.run_id === activeAgentRunId);

    const isDelegatedTaskEventPayload = (payload: {
      tool_name?: string | undefined;
      subagent_type?: string | undefined;
      delegated_job_id?: string | undefined;
      delegated_session_id?: string | undefined;
      delegated_conversation_id?: string | undefined;
      delegated_agent_run_id?: string | undefined;
    }) =>
      (payload.tool_name != null && canonicalizeToolName(payload.tool_name) === "delegate_start")
      || payload.subagent_type === "delegated"
      || payload.delegated_job_id != null
      || payload.delegated_session_id != null
      || payload.delegated_conversation_id != null
      || payload.delegated_agent_run_id != null;

    const normalizeDelegatedTaskStatus = (
      status: string | undefined,
    ): StreamingTask["status"] | undefined => {
      switch (status) {
        case "running":
        case "completed":
        case "failed":
        case "cancelled":
          return status;
        default:
          return undefined;
      }
    };

    const commitDelegationLifecycle = (
      evidence: ReconcileDelegationTaskInput,
      receivedAt: number,
    ) => {
      const reconciliation = reconcileDelegationTaskMap(streamingTasksRef.current, evidence);
      const blocks = reconcileDelegationTaskMarkers(streamingContentBlocksRef.current, {
        canonicalKey: reconciliation.canonicalKey,
        aliasKeys: reconciliation.aliasKeys,
        ...(evidence.seq != null ? { seq: evidence.seq } : {}),
        receivedAt,
      });
      // Lifecycle state is one transaction: both imperative snapshots advance before
      // either React commit can trigger a later event handler.
      streamingTasksRef.current = reconciliation.tasks;
      streamingContentBlocksRef.current = blocks;
      setStreamingContentBlocks(() => blocks);
      setStreamingTasks(() => reconciliation.tasks);
    };

    // ── agent:tool_call ──────────────────────────────────────────────
    // Handles tool call accumulation for streaming display.
    // Routes child tool calls to parent task when supportsSubagentTasks is enabled.
    unsubscribes.push(
      bus.subscribe<{
        tool_name: string;
        tool_id?: string;
        arguments: unknown;
        result?: unknown;
        result_preview_truncated?: boolean | null;
        resultPreviewTruncated?: boolean | null;
        result_preview_original_bytes?: number | null;
        resultPreviewOriginalBytes?: number | null;
        result_preview_line_count?: number | null;
        resultPreviewLineCount?: number | null;
        result_preview_omitted_lines?: number | null;
        resultPreviewOmittedLines?: number | null;
        arguments_preview_truncated?: boolean | null;
        argumentsPreviewTruncated?: boolean | null;
        arguments_preview_original_bytes?: number | null;
        argumentsPreviewOriginalBytes?: number | null;
        arguments_preview_line_count?: number | null;
        argumentsPreviewLineCount?: number | null;
        arguments_preview_omitted_lines?: number | null;
        argumentsPreviewOmittedLines?: number | null;
        diff_preview?: unknown;
        diffPreview?: unknown;
        detail_ref?: unknown;
        detailRef?: unknown;
        conversation_id: string;
        context_id?: string;
        context_type?: string;
        diff_context?: {
          old_content?: string;
          old_file_exists?: boolean;
          file_path: string;
        } | null;
        parent_tool_use_id?: string | null;
        seq?: number;
      }>("agent:tool_call", (payload) => {
        const receivedAt = Date.now();
        const { tool_name, tool_id, arguments: args, result, diff_context, parent_tool_use_id } = payload;

        if (!isRelevant(payload)) return;

        // Handle result events: update existing tool calls with result payload
        const resultToolUseId = parseToolResultId(tool_name);
        if (resultToolUseId) {
          const toolUseId = resultToolUseId;

          // Remove start time when tool call completes; update heartbeat + grace period timestamp + per-tool completion
          if (storeKey) {
            const store = useChatStore.getState();
            store.removeToolCallStartTime(storeKey, toolUseId);
            store.updateLastAgentEvent(storeKey);
            store.setLastToolCallCompletionTimestamp(storeKey, Date.now());
            store.setToolCallCompletionTimestamp(storeKey, toolUseId, Date.now());
          }

          // 1. Update matching entry in streamingToolCalls
          setStreamingToolCalls((prev) =>
            prev.map((tc) => {
              if (tc.id !== toolUseId) return tc;
              const updated: ToolCall = { ...tc };
              if (result != null) {
                applyToolCallResultPreview(updated, result, payload);
              }
              return updated;
            })
          );

          // 2. Update matching entry in streamingContentBlocks
          setStreamingContentBlocks((prev) =>
            prev.map((block) => {
              if (block.type !== "tool_use" || block.toolCall.id !== toolUseId) return block;
              const updated: ToolCall = { ...block.toolCall };
              if (result != null) {
                applyToolCallResultPreview(updated, result, payload);
              }
              return { ...block, toolCall: updated };
            })
          );

          // 3. Update matching entry in streamingTasks.childToolCalls
          const delegationResult = extractDelegationMetadata(undefined, result);
          setStreamingTasks((prev) => {
            const next = new Map(prev);
            let changed = false;

            const delegation = delegationResult;
            const matchedKey = findDelegationTaskKey(prev, toolUseId, delegation.jobId);
            const parentTask = matchedKey ? prev.get(matchedKey) : undefined;
            if (
              parentTask
              && (canonicalizeToolName(parentTask.toolName) === "delegate_start" || delegation.jobId)
            ) {
              const directTask = prev.get(toolUseId);
              const providerKey = directTask ? toolUseId : matchedKey!;
              const baseTask = directTask ?? parentTask;
              const updatedTask = mergeDelegationTaskMetadata(baseTask, delegation);
              const reconciled = reconcileDelegationTaskMap(prev, {
                source: "provider",
                toolUseId: providerKey,
                providerToolUseId: providerKey,
                ...(delegation.jobId ? { jobId: delegation.jobId } : {}),
                ...(payload.seq != null ? { seq: payload.seq } : {}),
                task: updatedTask,
              });
              next.clear();
              reconciled.tasks.forEach((task, key) => next.set(key, task));
              changed = true;
            }

            for (const [taskId, task] of next) {
              const childIdx = task.childToolCalls.findIndex((tc) => tc.id === toolUseId);
              if (childIdx >= 0) {
                const updatedCalls = [...task.childToolCalls];
                const existing = updatedCalls[childIdx]!;
                const updated: ToolCall = { ...existing };
                if (result != null) {
                  applyToolCallResultPreview(updated, result, payload);
                }
                updatedCalls[childIdx] = updated;
                next.set(taskId, { ...task, childToolCalls: updatedCalls });
                changed = true;
              }
            }
            return changed ? next : prev;
          });

          if (delegationResult.jobId) {
            const temporaryId = `delegate-job:${delegationResult.jobId}`;
            setStreamingContentBlocks((prev) => {
              const matchedKey = findDelegationTaskKey(
                streamingTasksRef.current,
                toolUseId,
                delegationResult.jobId,
              );
              const canonicalKey = streamingTasksRef.current.has(toolUseId)
                ? toolUseId
                : matchedKey ?? toolUseId;
              return reconcileDelegationTaskMarkers(prev, {
                canonicalKey,
                aliasKeys: [toolUseId, temporaryId, matchedKey].filter(
                  (key): key is string => key != null,
                ),
              });
            });
          }

          return;
        }

        // Build diffContext with exactOptionalPropertyTypes compliance
        let diffContext: ToolCall["diffContext"];
        if (diff_context) {
          diffContext = { filePath: diff_context.file_path };
          if (diff_context.old_content != null) {
            diffContext.oldContent = diff_context.old_content;
          }
          if (typeof diff_context.old_file_exists === "boolean") {
            diffContext.oldFileExists = diff_context.old_file_exists;
          }
        }

        // Use backend tool_id for deduplication. Some provider streams can omit
        // item ids, so fall back to a stable name+arguments key to let the
        // completed event update the live card instead of leaving a loading card.
        const id = tool_id ?? buildStreamingToolCallId(tool_name, args);

        const entry: ToolCall = { id, name: tool_name, arguments: args };
        if (result != null) {
          applyToolCallResultPreview(entry, result, payload);
        }
        applyBackendToolArgumentPreviewMetadata(entry, payload);
        if (diffContext) {
          entry.diffContext = diffContext;
        }

        const canonicalToolName = canonicalizeToolName(tool_name);

        if (!parent_tool_use_id && isDelegationStartToolCall(canonicalToolName)) {
          setStreamingContentBlocks((prev) => {
            const next = reconcileDelegationTaskMarkers(prev, {
              canonicalKey: id,
              aliasKeys: [id],
              ...(payload.seq != null ? { seq: payload.seq } : {}),
              receivedAt,
            });
            streamingContentBlocksRef.current = next;
            return next;
          });
          setStreamingTasks((prev) => {
            const delegation = extractDelegationMetadata(args, result);
            const description =
              delegation.title
              ?? delegation.prompt
              ?? (typeof args === "object" && args != null && "prompt" in args && typeof (args as { prompt?: unknown }).prompt === "string"
                ? (args as { prompt: string }).prompt
                : "");
            const task: StreamingTask = {
              toolUseId: id,
              toolName: tool_name,
              description,
              subagentType: "delegated",
              model: delegation.effectiveModelId ?? delegation.logicalModel ?? "unknown",
              status: "running",
              startedAt: Date.now(),
              childToolCalls: [],
              ...(delegation.jobId ? { delegatedJobId: delegation.jobId } : {}),
              ...(delegation.delegatedSessionId ? { delegatedSessionId: delegation.delegatedSessionId } : {}),
              ...(delegation.delegatedConversationId ? { delegatedConversationId: delegation.delegatedConversationId } : {}),
              ...(delegation.delegatedAgentRunId ? { delegatedAgentRunId: delegation.delegatedAgentRunId } : {}),
              ...(delegation.providerHarness ? { providerHarness: delegation.providerHarness } : {}),
              ...(delegation.providerSessionId ? { providerSessionId: delegation.providerSessionId } : {}),
              ...(delegation.upstreamProvider ? { upstreamProvider: delegation.upstreamProvider } : {}),
              ...(delegation.providerProfile ? { providerProfile: delegation.providerProfile } : {}),
              ...(delegation.logicalModel ? { logicalModel: delegation.logicalModel } : {}),
              ...(delegation.effectiveModelId ? { effectiveModelId: delegation.effectiveModelId } : {}),
              ...(delegation.logicalEffort ? { logicalEffort: delegation.logicalEffort } : {}),
              ...(delegation.effectiveEffort ? { effectiveEffort: delegation.effectiveEffort } : {}),
              ...(delegation.approvalPolicy ? { approvalPolicy: delegation.approvalPolicy } : {}),
              ...(delegation.sandboxMode ? { sandboxMode: delegation.sandboxMode } : {}),
              ...(payload.seq != null ? { seq: payload.seq } : {}),
            };
            const next = reconcileDelegationTaskMap(prev, {
              source: "provider",
              toolUseId: id,
              ...(delegation.jobId ? { jobId: delegation.jobId } : {}),
              ...(payload.seq != null ? { seq: payload.seq } : {}),
              task,
            }).tasks;
            streamingTasksRef.current = next;
            return next;
          });
          return;
        }

        if (!parent_tool_use_id && isDelegationControlToolCall(canonicalToolName)) {
          const delegation = extractDelegationMetadata(args, result);
          setStreamingTasks((prev) => {
            const matchedKey = findDelegationTaskKey(prev, undefined, delegation.jobId);
            if (!matchedKey) return prev;
            const task = prev.get(matchedKey);
            if (!task) return prev;
            const updated = mergeDelegationTaskMetadata(task, delegation);
            return reconcileDelegationTaskMap(prev, {
              source: "provider",
              toolUseId: matchedKey,
              providerToolUseId: matchedKey,
              ...(delegation.jobId ? { jobId: delegation.jobId } : {}),
              ...(payload.seq != null ? { seq: payload.seq } : {}),
              task: updated,
            }).tasks;
          });
          return;
        }

        // Record start time for new non-result tool calls (for elapsed timer display)
        // Also update heartbeat timestamp so watchdog doesn't false-trigger during long tool calls
        if (storeKey && result == null) {
          const store = useChatStore.getState();
          const existingTimes = store.toolCallStartTimes[storeKey];
          if (!existingTimes?.[id]) {
            store.setToolCallStartTime(storeKey, id, Date.now());
          }
          store.updateLastAgentEvent(storeKey);
        } else if (storeKey && result != null) {
          const store = useChatStore.getState();
          store.removeToolCallStartTime(storeKey, id);
          store.updateLastAgentEvent(storeKey);
          store.setLastToolCallCompletionTimestamp(storeKey, Date.now());
          store.setToolCallCompletionTimestamp(storeKey, id, Date.now());
        }

        // Route to parent task's childToolCalls if this is a subagent tool call
        if (supportsSubagentTasks && parent_tool_use_id) {
          setStreamingTasks((prev) => {
            const task = prev.get(parent_tool_use_id);
            if (!task) return prev;
            const next = new Map(prev);
            const existingIdx = task.childToolCalls.findIndex((tc) => tc.id === id);
            if (existingIdx >= 0) {
              // Update existing (Started → Completed lifecycle)
              const updatedCalls = [...task.childToolCalls];
              const existing = updatedCalls[existingIdx]!;
              const updated: ToolCall = {
                ...existing,
                name: tool_name,
                arguments: args ?? existing.arguments,
              };
              if (result != null) {
                applyToolCallResultPreview(updated, result, payload);
              } else if (existing.result != null) {
                updated.result = existing.result;
              }
              applyBackendToolArgumentPreviewMetadata(updated, payload);
              if (diffContext) {
                updated.diffContext = diffContext;
              }
              updatedCalls[existingIdx] = updated;
              next.set(parent_tool_use_id, { ...task, childToolCalls: updatedCalls });
            } else {
              // New child tool call — append
              next.set(parent_tool_use_id, {
                ...task,
                childToolCalls: [...task.childToolCalls, entry],
              });
            }
            return next;
          });
        } else {
          // Parent-level tool call — route to streamingToolCalls
          setStreamingToolCalls((prev) => {
            const existing = prev.find((tc) => tc.id === id);
            if (existing) {
              return prev.map((tc) => {
                if (tc.id !== id) return tc;
                const updated: ToolCall = {
                  ...tc,
                  name: tool_name,
                  arguments: args ?? tc.arguments,
                };
                if (result != null) {
                  applyToolCallResultPreview(updated, result, payload);
                } else if (tc.result != null) {
                  updated.result = tc.result;
                }
                applyBackendToolArgumentPreviewMetadata(updated, payload);
                if (diffContext) {
                  updated.diffContext = diffContext;
                }
                return updated;
              });
            }
            return [...prev, entry];
          });

          // Push to streamingContentBlocks to preserve chronological position.
          // Task/Agent tool calls get a position-marker block { type: "task", toolUseId }
          // so they render inline at the correct position (not grouped after all text).
          // Actual task metadata is read from streamingTasks Map via toolUseId lookup.
          if (canonicalToolName === "task" || canonicalToolName === "agent" || canonicalToolName === "delegate_start") {
            setStreamingContentBlocks((prev) => {
              // Only add the marker once — deduplicate by toolUseId
              const alreadyHasMarker = prev.some((block) => block.type === "task" && block.toolUseId === id);
              if (alreadyHasMarker) return prev;
              return [...prev, { type: "task", toolUseId: id, receivedAt }];
            });
          } else {
            setStreamingContentBlocks((prev) => {
              const existing = prev.find((block) => block.type === "tool_use" && block.toolCall.id === id);
              if (existing) {
                // Update existing tool_use block
                return prev.map((block) => {
                  if (block.type !== "tool_use" || block.toolCall.id !== id) return block;
                  const updated: ToolCall = {
                    ...block.toolCall,
                    name: tool_name,
                    arguments: args ?? block.toolCall.arguments,
                  };
                  if (result != null) {
                    applyToolCallResultPreview(updated, result, payload);
                  } else if (block.toolCall.result != null) {
                    updated.result = block.toolCall.result;
                  }
                  applyBackendToolArgumentPreviewMetadata(updated, payload);
                  if (diffContext) {
                    updated.diffContext = diffContext;
                  }
                  // Preserve existing seq/timestamp when updating block
                  const updatedBlock = { type: "tool_use" as const, toolCall: updated };
                  return {
                    ...updatedBlock,
                    ...(block.seq != null ? { seq: block.seq } : {}),
                    ...(block.receivedAt != null ? { receivedAt: block.receivedAt } : {}),
                  };
                });
              }
              // New tool_use block — append
              const newBlock = { type: "tool_use" as const, toolCall: entry, receivedAt };
              return [...prev, payload.seq != null ? { ...newBlock, seq: payload.seq } : newBlock];
            });
          }
        }
        // No per-tool-call invalidation: tool calls are visible via streaming state.
        // DB refetch happens only at turn completion (agent:run_completed).
      })
    );

    // ── agent:task_started (subagent) ────────────────────────────────
    unsubscribes.push(
      bus.subscribe<{
          tool_use_id: string;
          tool_name?: string;
          description?: string;
          subagent_type?: string;
          model?: string;
          delegated_job_id?: string;
          delegated_session_id?: string;
          delegated_conversation_id?: string;
          delegated_agent_run_id?: string;
          provider_harness?: string;
          provider_session_id?: string;
          upstream_provider?: string;
          provider_profile?: string;
          logical_model?: string;
          effective_model_id?: string;
          logical_effort?: string;
          effective_effort?: string;
          approval_policy?: string;
          sandbox_mode?: string;
          started_at?: string;
          completed_at?: string;
          timestamp_provenance?: "delegated_run" | "delegation_job";
          conversation_id: string;
          run_id?: string | null;
          context_id?: string;
          context_type?: string;
          seq?: number;
      }>("agent:task_started", (payload) => {
          const receivedAt = Date.now();
          if (!isRelevant(payload)) return;
          const isDelegated = isDelegatedTaskEventPayload(payload);
          if (isDelegated && activeAgentRunId && payload.run_id !== activeAgentRunId) return;
          if (!supportsSubagentTasks && !isDelegated) return;
          if (isDelegated) {
            const lifecycleTask = buildDelegationLifecycleTask(payload);
            const evidence = {
              source: "lifecycle-start" as const,
              toolUseId: payload.tool_use_id,
              ...(payload.delegated_job_id != null ? { jobId: payload.delegated_job_id } : {}),
              ...(payload.seq != null ? { seq: payload.seq } : {}),
              allowSingleUnresolvedPlaceholder: true,
              task: lifecycleTask,
            };
            commitDelegationLifecycle(evidence, receivedAt);
            return;
          }
          setStreamingContentBlocks((prev) => {
            const alreadyHasMarker = prev.some(
              (block) => block.type === "task" && block.toolUseId === payload.tool_use_id,
            );
            if (alreadyHasMarker) return prev;
            return [...prev, { type: "task", toolUseId: payload.tool_use_id, receivedAt }];
          });
          setStreamingTasks((prev) => {
            const existingKey = findDelegationTaskKey(
              prev,
              payload.tool_use_id,
              payload.delegated_job_id,
            );
            const placementKey = existingKey ?? payload.tool_use_id;
            const existing = existingKey ? prev.get(existingKey) : undefined;
            const next = new Map(prev);
            const delegatedJobId = payload.delegated_job_id ?? existing?.delegatedJobId;
            const delegatedSessionId = payload.delegated_session_id ?? existing?.delegatedSessionId;
            const delegatedConversationId =
              payload.delegated_conversation_id ?? existing?.delegatedConversationId;
            const delegatedAgentRunId =
              payload.delegated_agent_run_id ?? existing?.delegatedAgentRunId;
            const providerHarness = payload.provider_harness ?? existing?.providerHarness;
            const providerSessionId = payload.provider_session_id ?? existing?.providerSessionId;
            const upstreamProvider = payload.upstream_provider ?? existing?.upstreamProvider;
            const providerProfile = payload.provider_profile ?? existing?.providerProfile;
            const logicalModel = payload.logical_model ?? existing?.logicalModel;
            const effectiveModelId = payload.effective_model_id ?? existing?.effectiveModelId;
            const logicalEffort = payload.logical_effort ?? existing?.logicalEffort;
            const effectiveEffort = payload.effective_effort ?? existing?.effectiveEffort;
            const approvalPolicy = payload.approval_policy ?? existing?.approvalPolicy;
            const sandboxMode = payload.sandbox_mode ?? existing?.sandboxMode;
            const newTask: StreamingTask = {
              toolUseId: placementKey,
              toolName: payload.tool_name ?? existing?.toolName ?? "Task",
              description: payload.description ?? existing?.description ?? "",
              subagentType:
                payload.subagent_type
                ?? existing?.subagentType
                ?? (isDelegated ? "delegated" : "unknown"),
              model:
                payload.model
                ?? payload.effective_model_id
                ?? payload.logical_model
                ?? existing?.model
                ?? "unknown",
              status: normalizeDelegatedTaskStatus(existing?.status) ?? "running",
              startedAt: existing?.startedAt ?? Date.now(),
              childToolCalls: existing?.childToolCalls ?? [],
              ...(delegatedJobId != null ? { delegatedJobId } : {}),
              ...(delegatedSessionId != null ? { delegatedSessionId } : {}),
              ...(delegatedConversationId != null ? { delegatedConversationId } : {}),
              ...(delegatedAgentRunId != null ? { delegatedAgentRunId } : {}),
              ...(providerHarness != null ? { providerHarness } : {}),
              ...(providerSessionId != null ? { providerSessionId } : {}),
              ...(upstreamProvider != null ? { upstreamProvider } : {}),
              ...(providerProfile != null ? { providerProfile } : {}),
              ...(logicalModel != null ? { logicalModel } : {}),
              ...(effectiveModelId != null ? { effectiveModelId } : {}),
              ...(logicalEffort != null ? { logicalEffort } : {}),
              ...(effectiveEffort != null ? { effectiveEffort } : {}),
              ...(approvalPolicy != null ? { approvalPolicy } : {}),
              ...(sandboxMode != null ? { sandboxMode } : {}),
              ...(existing?.completedAt != null ? { completedAt: existing.completedAt } : {}),
              ...(existing?.totalDurationMs != null ? { totalDurationMs: existing.totalDurationMs } : {}),
              ...(existing?.totalTokens != null ? { totalTokens: existing.totalTokens } : {}),
              ...(existing?.totalToolUseCount != null ? { totalToolUseCount: existing.totalToolUseCount } : {}),
              ...(existing?.agentId ? { agentId: existing.agentId } : {}),
              ...(existing?.inputTokens != null ? { inputTokens: existing.inputTokens } : {}),
              ...(existing?.outputTokens != null ? { outputTokens: existing.outputTokens } : {}),
              ...(existing?.cacheCreationTokens != null
                ? { cacheCreationTokens: existing.cacheCreationTokens }
                : {}),
              ...(existing?.cacheReadTokens != null ? { cacheReadTokens: existing.cacheReadTokens } : {}),
              ...(existing?.estimatedUsd != null ? { estimatedUsd: existing.estimatedUsd } : {}),
              ...(existing?.textOutput ? { textOutput: existing.textOutput } : {}),
            };
            if (payload.seq != null) {
              newTask.seq = payload.seq;
            } else if (existing?.seq != null) {
              newTask.seq = existing.seq;
            }
            next.set(placementKey, newTask);
            return next;
          });
      })
    );

    // ── agent:task_completed (subagent) ──────────────────────────────
    unsubscribes.push(
      bus.subscribe<{
          tool_use_id: string;
          agent_id?: string;
          status?: string;
          delegated_job_id?: string;
          delegated_session_id?: string;
          delegated_conversation_id?: string;
          delegated_agent_run_id?: string;
          provider_harness?: string;
          provider_session_id?: string;
          upstream_provider?: string;
          provider_profile?: string;
          logical_model?: string;
          effective_model_id?: string;
          logical_effort?: string;
          effective_effort?: string;
          approval_policy?: string;
          sandbox_mode?: string;
          total_duration_ms?: number;
          total_tokens?: number;
          total_tool_use_count?: number;
          input_tokens?: number;
          output_tokens?: number;
          cache_creation_tokens?: number;
          cache_read_tokens?: number;
          estimated_usd?: number;
          text_output?: string;
          error?: string;
          started_at?: string;
          completed_at?: string;
          timestamp_provenance?: "delegated_run" | "delegation_job";
          conversation_id: string;
          run_id?: string | null;
          context_id?: string;
          context_type?: string;
          seq?: number;
      }>("agent:task_completed", (payload) => {
          if (!isRelevant(payload)) return;
          const isDelegatedPayload = isDelegatedTaskEventPayload(payload);
          if (
            isDelegatedPayload
            && activeAgentRunId
            && payload.run_id !== activeAgentRunId
          ) return;
          if (!supportsSubagentTasks && !isDelegatedPayload) return;
          if (isDelegatedPayload) {
            const currentKey = findDelegationTaskKey(
              streamingTasksRef.current,
              payload.tool_use_id,
              payload.delegated_job_id,
            );
            const current = currentKey ? streamingTasksRef.current.get(currentKey) : undefined;
            const terminalTask = buildDelegationLifecycleTask(
              {
                ...payload,
                status: normalizeDelegatedTaskStatus(payload.status) ?? "completed",
              },
              current,
            );
            const evidence = {
              source: "lifecycle-complete" as const,
              toolUseId: payload.tool_use_id,
              ...(payload.delegated_job_id != null ? { jobId: payload.delegated_job_id } : {}),
              ...(payload.seq != null ? { seq: payload.seq } : {}),
              allowSingleUnresolvedPlaceholder: true,
              task: terminalTask,
            };
            commitDelegationLifecycle(evidence, Date.now());
            return;
          }
          setStreamingTasks((prev) => {
            const taskKey = findDelegationTaskKey(
              prev,
              payload.tool_use_id,
              payload.delegated_job_id,
            );
            const task = taskKey ? prev.get(taskKey) : undefined;
            const isDelegated = isDelegatedPayload
              || (task != null && isDelegatedTaskEventPayload({
                tool_name: task.toolName,
                subagent_type: task.subagentType,
                delegated_job_id: task.delegatedJobId,
                delegated_session_id: task.delegatedSessionId,
                delegated_conversation_id: task.delegatedConversationId,
                delegated_agent_run_id: task.delegatedAgentRunId,
              }));
            if (!task && !isDelegated) return prev;
            const next = new Map(prev);
            const updated: StreamingTask = {
              ...(task ?? {
                toolUseId: payload.tool_use_id,
                toolName: payload.delegated_job_id ? "delegate_start" : "Task",
                description: "",
                subagentType: isDelegated ? "delegated" : "unknown",
                model:
                  payload.effective_model_id
                  ?? payload.logical_model
                  ?? "unknown",
                startedAt: Date.now(),
                childToolCalls: [],
                status: "running",
              }),
              status: normalizeDelegatedTaskStatus(payload.status) ?? "completed",
              completedAt: Date.now(),
            };
            if (payload.agent_id != null) {
              updated.agentId = payload.agent_id;
            }
            if (payload.total_duration_ms != null) {
              updated.totalDurationMs = payload.total_duration_ms;
            }
            if (payload.total_tokens != null) {
              updated.totalTokens = payload.total_tokens;
            }
            if (payload.total_tool_use_count != null) {
              updated.totalToolUseCount = payload.total_tool_use_count;
            }
            if (payload.delegated_job_id != null) {
              updated.delegatedJobId = payload.delegated_job_id;
            }
            if (payload.delegated_session_id != null) {
              updated.delegatedSessionId = payload.delegated_session_id;
            }
            if (payload.delegated_conversation_id != null) {
              updated.delegatedConversationId = payload.delegated_conversation_id;
            }
            if (payload.delegated_agent_run_id != null) {
              updated.delegatedAgentRunId = payload.delegated_agent_run_id;
            }
            if (payload.provider_harness != null) {
              updated.providerHarness = payload.provider_harness;
            }
            if (payload.provider_session_id != null) {
              updated.providerSessionId = payload.provider_session_id;
            }
            if (payload.upstream_provider != null) {
              updated.upstreamProvider = payload.upstream_provider;
            }
            if (payload.provider_profile != null) {
              updated.providerProfile = payload.provider_profile;
            }
            if (payload.logical_model != null) {
              updated.logicalModel = payload.logical_model;
            }
            if (payload.effective_model_id != null) {
              updated.effectiveModelId = payload.effective_model_id;
            }
            if (payload.logical_effort != null) {
              updated.logicalEffort = payload.logical_effort;
            }
            if (payload.effective_effort != null) {
              updated.effectiveEffort = payload.effective_effort;
            }
            if (payload.approval_policy != null) {
              updated.approvalPolicy = payload.approval_policy;
            }
            if (payload.sandbox_mode != null) {
              updated.sandboxMode = payload.sandbox_mode;
            }
            if (payload.input_tokens != null) {
              updated.inputTokens = payload.input_tokens;
            }
            if (payload.output_tokens != null) {
              updated.outputTokens = payload.output_tokens;
            }
            if (payload.cache_creation_tokens != null) {
              updated.cacheCreationTokens = payload.cache_creation_tokens;
            }
            if (payload.cache_read_tokens != null) {
              updated.cacheReadTokens = payload.cache_read_tokens;
            }
            if (payload.estimated_usd != null) {
              updated.estimatedUsd = payload.estimated_usd;
            }
            if (payload.text_output != null) {
              updated.textOutput = payload.text_output;
            }
            if (payload.seq != null) {
              updated.seq = payload.seq;
            }
            next.set(taskKey ?? payload.tool_use_id, updated);
            return next;
          });
      })
    );

    // ── agent:chunk (streaming text) ─────────────────────────────────
    // Chunks are filtered by conversation_id via isRelevant, including delegated
    // conversations when one is active.
    if (supportsStreamingText) {
      unsubscribes.push(
        bus.subscribe<{
          text: string;
          conversation_id: string;
          context_id?: string;
          context_type?: string;
          seq?: number;
          append_to_previous?: boolean;
          block_index?: number;
          run_id?: string | null;
        }>(
          "agent:chunk", (payload) => {
            const receivedAt = Date.now();
            if (!isRelevant(payload)) return;
            if (activeAgentRunId && payload.run_id && payload.run_id !== activeAgentRunId) return;
            if (
              payload.seq != null
              && lastChunkSeqRef.current != null
              && payload.seq <= lastChunkSeqRef.current
            ) return;
            if (payload.seq != null) {
              lastChunkSeqRef.current = payload.seq;
            }
            setStreamingContentBlocks((prev) => {
              const shouldAppend = payload.append_to_previous ?? true;
              // Staleness is guarded by lastChunkSeqRef above; block seq values
              // are not comparable to chunk seq — recovered anchors carry
              // timelineSequence, a different counter.
              const hasBlockIndex = payload.block_index != null;
              // If last block is text and the backend says this chunk extends it, append.
              // Codex agent_message events are already logical text blocks, so they set
              // append_to_previous=false to preserve live block boundaries.
              const appendIndex = hasBlockIndex
                ? prev.findIndex((block) => block.type === "text" && block.blockIndex === payload.block_index)
                : prev[prev.length - 1]?.type === "text" ? prev.length - 1 : -1;
              if (shouldAppend && appendIndex >= 0) {
                const updated = [...prev];
                const lastBlock = updated[appendIndex]! as Extract<StreamingContentBlock, { type: "text" }>;
                const appendBlock = {
                  ...lastBlock,
                  text: lastBlock.text + payload.text,
                  ...(payload.seq != null && {
                    seq: Math.max(lastBlock.seq ?? payload.seq, payload.seq),
                  }),
                };
                updated[appendIndex] = appendBlock;
                return updated;
              }
              // New text block: use seq from payload
              const newBlock = {
                type: "text" as const,
                text: payload.text,
                receivedAt,
                ...(payload.block_index != null && { blockIndex: payload.block_index }),
                ...(payload.seq != null && { seq: payload.seq }),
              };
              return [...prev, newBlock];
            });
          }
        )
      );
    }

    // ── agent:message_created ────────────────────────────────────────
    // Clear streaming state for assistant messages to prevent duplicate display.
    //
    // Query-aware finalization strategy:
    // 1. Streaming active: streamingContentBlocks visible, last DB assistant message filtered
    // 2. agent:message_created fires: setIsFinalizing(true) + clear streaming state (same batch)
    // 3. Re-render: hasActiveStreaming=false, isFinalizing=true → filter still applies
    // 4. Try a lightweight active-tail cache handoff from a backend render-ready
    //    payload or the live streaming snapshot; if it succeeds, the watcher clears immediately.
    // 5. Otherwise subscribe to query cache; when the fallback refetch returns
    //    data containing the new message_id, call setIsFinalizing(false) and unsubscribe.
    // 6. Safety timeout (3s) clears isFinalizing if the query never returns the expected message.
    // Result: smooth swap with no fixed-delay race condition.
    unsubscribes.push(
      bus.subscribe<AgentMessageCreatedPayload>("agent:message_created", (payload) => {
        if (!payload.conversation_id) return;
        if (!isRelevant(payload)) return;

        let usedLightweightHandoff = false;
        if (isProviderRole(payload.role)) {
          const convId = payload.conversation_id;
          const assistantMessageId = payload.message_id;
          const contentBlocks = payload.render_ready || contextType === "ideation"
            ? null
            : buildFinalizedContentBlocks(
              payload,
              streamingContentBlocksRef.current,
              streamingToolCallsRef.current,
            );
          const finalizedMessage = contentBlocks
            ? buildFinalizedMessageForCache(payload, contentBlocks)
            : null;

          // Set isFinalizing=true in the same batch as clearing streaming state.
          // When the active timeline cache can be updated from the canonical
          // event payload or live stream snapshot, the cache watcher clears
          // finalizing immediately; otherwise it waits for the DB refetch fallback.
          setIsFinalizing(true);
          if (payload.render_ready) {
            usedLightweightHandoff = upsertRenderReadyMessageIntoConversationCache(
              queryClient,
              convId,
              payload.render_ready,
            );
          } else {
            usedLightweightHandoff = finalizedMessage
              ? upsertFinalizedMessageIntoConversationCache(queryClient, convId, finalizedMessage)
              : false;
          }
          setStreamingContentBlocks(prev => prev.length === 0 ? prev : []);
          setStreamingToolCalls(prev => prev.length === 0 ? prev : []);
          setStreamingTasks(prev => prev.size === 0 ? prev : new Map());

          let cleanupDone = false;
          let safetyTimerId: ReturnType<typeof setTimeout> | undefined;
          let unsubscribeCache: (() => void) | undefined;

          const clearFinalizing = () => {
            if (cleanupDone) return;
            cleanupDone = true;
            setIsFinalizing(false);
            if (safetyTimerId !== undefined) {
              clearTimeout(safetyTimerId);
              safetyTimerId = undefined;
            }
            if (unsubscribeCache) {
              unsubscribeCache();
              unsubscribeCache = undefined;
            }
            const idx = activeCancelFnsRef.current.indexOf(clearFinalizing);
            if (idx >= 0) activeCancelFnsRef.current.splice(idx, 1);
          };

          activeCancelFnsRef.current.push(clearFinalizing);

          // Safety fallback — prevents isFinalizing from being stuck forever
          safetyTimerId = setTimeout(clearFinalizing, 3000);

          if (assistantMessageId) {
            // Race guard: check if the query already has the message before subscribing
            const existing = getCachedConversationMessages(queryClient, convId);
            if (
              existing.some(
                (message) =>
                  message.id === assistantMessageId ||
                  message.parentMessageId === assistantMessageId
              )
            ) {
              clearFinalizing();
            } else {
              // Subscribe to query cache updates — clear isFinalizing when the new
              // assistant message appears in the refetched conversation data.
              unsubscribeCache = queryClient.getQueryCache().subscribe((event) => {
                if (event.type !== "updated") return;
                const evKey = event.query.queryKey;
                if (!Array.isArray(evKey) || evKey.length < 3 || evKey[2] !== convId) return;
                const data = getCachedConversationMessages(queryClient, convId);
                if (
                  data.some(
                    (message) =>
                      message.id === assistantMessageId ||
                      message.parentMessageId === assistantMessageId
                  )
                ) {
                  clearFinalizing();
                }
              });
            }
          }
          // If no message_id in payload, the safety timeout alone handles cleanup
        }

        // Cancel in-flight fetches so a stale response cannot overwrite either
        // the lightweight active-tail handoff or the fallback refetch.
        void queryClient.cancelQueries({ queryKey: chatKeys.conversation(payload.conversation_id), exact: true });
        void queryClient.cancelQueries({ queryKey: chatKeys.conversationSummary(payload.conversation_id) });
        void queryClient.cancelQueries({ queryKey: chatKeys.conversationHistory(payload.conversation_id) });
        void queryClient.cancelQueries({ queryKey: chatKeys.conversationTimeline(payload.conversation_id) });
        if (isProviderRole(payload.role) && usedLightweightHandoff) {
          queryClient.invalidateQueries({
            queryKey: chatKeys.conversationSummary(payload.conversation_id),
          });
        } else {
          invalidateConversationDataQueries(queryClient, payload.conversation_id);
        }
        queryClient.invalidateQueries({
          queryKey: conversationStatsKey(payload.conversation_id),
        });
      })
    );

    // ── agent:run_completed ──────────────────────────────────────────
    // Keep streaming state visible on completion until agent:message_created
    // performs the query-aware handoff to persisted DB data.
    // Query invalidation is owned by useAgentEvents to avoid duplicate refetches.
    unsubscribes.push(
      bus.subscribe<AgentRunCompletedPayload>("agent:run_completed", (payload) => {
        if (!isRelevant(payload)) return;

        // Clear all tool call start times and completion timestamps on run completion
        if (storeKey) {
          const store = useChatStore.getState();
          store.clearToolCallStartTimes(storeKey);
          store.clearToolCallCompletionTimestamps(storeKey);
        }

        queryClient.invalidateQueries({
          queryKey: conversationStatsKey(payload.conversation_id),
        });
      })
    );

    // ── agent:turn_completed ────────────────────────────────────────
    // Keep streaming state visible until agent:message_created swaps in
    // persisted DB data. Clearing here can blank an interactive turn if the
    // completion event beats the final message invalidation/refetch.
    // Query invalidation is owned by useAgentEvents to avoid duplicate refetches.
    unsubscribes.push(
      bus.subscribe<AgentRunCompletedPayload>("agent:turn_completed", (payload) => {
        if (!isRelevant(payload)) return;

        queryClient.invalidateQueries({
          queryKey: conversationStatsKey(payload.conversation_id),
        });
      })
    );

    // ── agent:error ──────────────────────────────────────────────────
    // Clear ALL streaming state on error.
    // Query invalidation is owned by useAgentEvents to avoid duplicate refetches.
    unsubscribes.push(
      bus.subscribe<{
        conversation_id: string;
        context_id?: string;
        context_type?: string;
        error: string;
      }>("agent:error", (payload) => {
        if (!isRelevant(payload)) return;

        setStreamingToolCalls(prev => prev.length === 0 ? prev : []);
        setStreamingContentBlocks(prev => prev.length === 0 ? prev : []);
        setStreamingTasks(prev => prev.size === 0 ? prev : new Map());
      })
    );

    // ── Cleanup ──────────────────────────────────────────────────────
    return () => {
      setStreamingToolCalls(prev => prev.length === 0 ? prev : []);
      setStreamingContentBlocks(prev => prev.length === 0 ? prev : []);
      setStreamingTasks(prev => prev.size === 0 ? prev : new Map());
      // NOTE: Do NOT cancel activeCancelFnsRef.current here — only cancel on genuine
      // context switch (handled by the [activeConversationId, contextId] effect above).
      // Cancelling here would interrupt isFinalizing for same-context re-renders
      // (e.g., when user sends a message while finalization is in progress).
      // NOTE: Do NOT call setIsFinalizing(false) here — the context-switch effect
      // clears isFinalizing via clearFinalizing() when it's genuinely needed.
      unsubscribes.forEach((unsub) => unsub());
    };
  }, [
    bus, queryClient, activeConversationId, activeAgentRunId, contextId, contextType,
    supportsStreamingText, supportsSubagentTasks,
    setStreamingToolCalls, setStreamingContentBlocks, setStreamingTasks,
    setIsFinalizing, storeKey,
  ]);
}
