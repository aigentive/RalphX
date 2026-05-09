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
  getCachedConversationMessages,
  invalidateConversationDataQueries,
} from "@/hooks/useChat";
import { conversationStatsKey } from "@/hooks/useConversationStats";
import { getContextConfig } from "@/lib/chat-context-registry";
import { isProviderRole } from "@/lib/chat/provider-role";
import type { ContextType } from "@/types/chat-conversation";
import type { AgentRunCompletedPayload } from "@/types/events";
import type { ToolCall } from "@/components/Chat/ToolCallIndicator";
import type { StreamingTask, StreamingContentBlock } from "@/types/streaming-task";
import type { Unsubscribe } from "@/lib/event-bus";
import { useChatStore } from "@/stores/chatStore";
import { canonicalizeToolName } from "@/components/Chat/tool-widgets/tool-name";
import {
  extractDelegationMetadata,
  isDelegationControlToolCall,
  isDelegationStartToolCall,
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

// ============================================================================
// Types
// ============================================================================

interface UseChatEventsProps {
  activeConversationId: string | null;
  contextId: string | null;
  contextType: ContextType | null;
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
  contextId,
  contextType,
  setStreamingToolCalls,
  setStreamingContentBlocks,
  setStreamingTasks,
  setIsFinalizing,
  storeKey,
}: UseChatEventsProps) {
  const bus = useEventBus();
  const queryClient = useQueryClient();

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
    setStreamingToolCalls(prev => prev.length === 0 ? prev : []);
    setStreamingContentBlocks(prev => prev.length === 0 ? prev : []);
    setStreamingTasks(prev => prev.size === 0 ? prev : new Map());

    const unsubscribes: Unsubscribe[] = [];

    // Helper: check if event matches current context
    const isRelevant = (payload: { conversation_id?: string; context_id?: string }) =>
      payload.conversation_id === activeConversationId &&
      (!contextId || payload.context_id === contextId);

    const isDelegatedTaskEventPayload = (payload: {
      tool_name?: string;
      subagent_type?: string;
      delegated_job_id?: string;
      delegated_session_id?: string;
      delegated_conversation_id?: string;
      delegated_agent_run_id?: string;
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
        detail_ref?: unknown;
        detailRef?: unknown;
        conversation_id: string;
        context_id?: string;
        context_type?: string;
        diff_context?: { old_content?: string; file_path: string } | null;
        parent_tool_use_id?: string | null;
        seq?: number;
      }>("agent:tool_call", (payload) => {
        const { tool_name, tool_id, arguments: args, result, diff_context, parent_tool_use_id } = payload;

        if (!isRelevant(payload)) return;

        // Handle result events: update existing tool calls with result payload
        if (tool_name.startsWith("result:toolu")) {
          // Extract tool_use_id from tool_name by stripping "result:" prefix
          const toolUseId = tool_name.slice(7); // "result:".length === 7

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
              return { type: "tool_use", toolCall: updated };
            })
          );

          // 3. Update matching entry in streamingTasks.childToolCalls
          setStreamingTasks((prev) => {
            const next = new Map(prev);
            let changed = false;

            const parentTask = prev.get(toolUseId);
            if (parentTask && canonicalizeToolName(parentTask.toolName) === "delegate_start") {
              const delegation = extractDelegationMetadata(undefined, result);
              const inferredFailure =
                delegation.status == null
                && delegation.textOutput?.trim().startsWith("ERROR:");
              const nextStatus =
                normalizeDelegatedTaskStatus(delegation.status)
                ?? (inferredFailure ? "failed" : parentTask.status);
              const updatedTask: StreamingTask = {
                ...parentTask,
                status: nextStatus,
                ...(delegation.agentName
                  ? { subagentType: "delegated", model: delegation.effectiveModelId ?? delegation.logicalModel ?? parentTask.model }
                  : {}),
                ...(delegation.providerHarness ? { providerHarness: delegation.providerHarness } : {}),
                ...(delegation.providerSessionId ? { providerSessionId: delegation.providerSessionId } : {}),
                ...(delegation.upstreamProvider ? { upstreamProvider: delegation.upstreamProvider } : {}),
                ...(delegation.providerProfile ? { providerProfile: delegation.providerProfile } : {}),
                ...(delegation.jobId ? { delegatedJobId: delegation.jobId } : {}),
                ...(delegation.delegatedSessionId ? { delegatedSessionId: delegation.delegatedSessionId } : {}),
                ...(delegation.delegatedConversationId ? { delegatedConversationId: delegation.delegatedConversationId } : {}),
                ...(delegation.delegatedAgentRunId ? { delegatedAgentRunId: delegation.delegatedAgentRunId } : {}),
                ...(delegation.logicalModel ? { logicalModel: delegation.logicalModel } : {}),
                ...(delegation.effectiveModelId ? { effectiveModelId: delegation.effectiveModelId } : {}),
                ...(delegation.logicalEffort ? { logicalEffort: delegation.logicalEffort } : {}),
                ...(delegation.effectiveEffort ? { effectiveEffort: delegation.effectiveEffort } : {}),
                ...(delegation.approvalPolicy ? { approvalPolicy: delegation.approvalPolicy } : {}),
                ...(delegation.sandboxMode ? { sandboxMode: delegation.sandboxMode } : {}),
                ...(delegation.inputTokens != null ? { inputTokens: delegation.inputTokens } : {}),
                ...(delegation.outputTokens != null ? { outputTokens: delegation.outputTokens } : {}),
                ...(delegation.cacheCreationTokens != null
                  ? { cacheCreationTokens: delegation.cacheCreationTokens }
                  : {}),
                ...(delegation.cacheReadTokens != null ? { cacheReadTokens: delegation.cacheReadTokens } : {}),
                ...(delegation.totalTokens != null ? { totalTokens: delegation.totalTokens } : {}),
                ...(delegation.estimatedUsd != null ? { estimatedUsd: delegation.estimatedUsd } : {}),
                ...(delegation.durationMs != null ? { totalDurationMs: delegation.durationMs } : {}),
                ...(delegation.textOutput ? { textOutput: delegation.textOutput } : {}),
                ...((nextStatus === "completed" || nextStatus === "failed" || nextStatus === "cancelled")
                  ? { completedAt: Date.now() }
                  : {}),
              };
              next.set(toolUseId, updatedTask);
              changed = true;
            }

            for (const [taskId, task] of prev) {
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

          return;
        }

        // Build diffContext with exactOptionalPropertyTypes compliance
        let diffContext: ToolCall["diffContext"];
        if (diff_context) {
          diffContext = { filePath: diff_context.file_path };
          if (diff_context.old_content != null) {
            diffContext.oldContent = diff_context.old_content;
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
        if (diffContext) {
          entry.diffContext = diffContext;
        }

        const canonicalToolName = canonicalizeToolName(tool_name);

        if (supportsSubagentTasks && !parent_tool_use_id && isDelegationStartToolCall(canonicalToolName)) {
          setStreamingContentBlocks((prev) => {
            const alreadyHasMarker = prev.some(
              (block) => block.type === "task" && block.toolUseId === id,
            );
            if (alreadyHasMarker) return prev;
            return [...prev, { type: "task", toolUseId: id }];
          });
          setStreamingTasks((prev) => {
            if (prev.has(id)) return prev;
            const delegation = extractDelegationMetadata(args, result);
            const description =
              delegation.title
              ?? delegation.prompt
              ?? (typeof args === "object" && args != null && "prompt" in args && typeof (args as { prompt?: unknown }).prompt === "string"
                ? (args as { prompt: string }).prompt
                : "");
            const next = new Map(prev);
            next.set(id, {
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
            });
            return next;
          });
          return;
        }

        if (supportsSubagentTasks && !parent_tool_use_id && isDelegationControlToolCall(canonicalToolName)) {
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
              return [...prev, { type: "task", toolUseId: id }];
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
                  if (diffContext) {
                    updated.diffContext = diffContext;
                  }
                  // Preserve existing seq when updating block
                  const updatedBlock = { type: "tool_use" as const, toolCall: updated };
                  return block.seq != null ? { ...updatedBlock, seq: block.seq } : updatedBlock;
                });
              }
              // New tool_use block — append
              const newBlock = { type: "tool_use" as const, toolCall: entry };
              return [...prev, payload.seq != null ? { ...newBlock, seq: payload.seq } : newBlock];
            });
          }
        }
        // No per-tool-call invalidation: tool calls are visible via streaming state.
        // DB refetch happens only at turn completion (agent:run_completed).
      })
    );

    // ── agent:task_started (subagent) ────────────────────────────────
    if (supportsSubagentTasks) {
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
          conversation_id: string;
          context_id?: string;
          context_type?: string;
          seq?: number;
        }>("agent:task_started", (payload) => {
          if (!isRelevant(payload)) return;
          setStreamingContentBlocks((prev) => {
            const alreadyHasMarker = prev.some(
              (block) => block.type === "task" && block.toolUseId === payload.tool_use_id,
            );
            if (alreadyHasMarker) return prev;
            return [...prev, { type: "task", toolUseId: payload.tool_use_id }];
          });
          setStreamingTasks((prev) => {
            const existing = prev.get(payload.tool_use_id);
            const next = new Map(prev);
            const isDelegated = isDelegatedTaskEventPayload(payload);
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
              toolUseId: payload.tool_use_id,
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
            next.set(payload.tool_use_id, newTask);
            return next;
          });
        })
      );
    }

    // ── agent:task_completed (subagent) ──────────────────────────────
    if (supportsSubagentTasks) {
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
          conversation_id: string;
          context_id?: string;
          context_type?: string;
          seq?: number;
        }>("agent:task_completed", (payload) => {
          if (!isRelevant(payload)) return;
          setStreamingTasks((prev) => {
            const task = prev.get(payload.tool_use_id);
            const isDelegated = isDelegatedTaskEventPayload(payload);
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
            next.set(payload.tool_use_id, updated);
            return next;
          });
        })
      );
    }

    // ── agent:chunk (streaming text) ─────────────────────────────────
    // Chunks are filtered by conversation_id via isRelevant — teammate chunks
    // match when activeConversationId is the teammate's conversation.
    if (supportsStreamingText) {
      unsubscribes.push(
        bus.subscribe<{
          text: string;
          conversation_id: string;
          context_id?: string;
          context_type?: string;
          seq?: number;
          append_to_previous?: boolean;
        }>(
          "agent:chunk", (payload) => {
            if (!isRelevant(payload)) return;
            setStreamingContentBlocks((prev) => {
              const lastBlock = prev[prev.length - 1];
              const shouldAppend = payload.append_to_previous ?? true;
              // If last block is text and the backend says this chunk extends it, append.
              // Codex agent_message events are already logical text blocks, so they set
              // append_to_previous=false to preserve live block boundaries.
              if (shouldAppend && lastBlock?.type === "text") {
                const updated = [...prev];
                // Preserve existing seq when appending to block (don't use latest chunk's seq)
                const appendBlock = { type: "text" as const, text: lastBlock.text + payload.text };
                updated[updated.length - 1] = lastBlock.seq != null ? { ...appendBlock, seq: lastBlock.seq } : appendBlock;
                return updated;
              }
              // New text block: use seq from payload
              const newBlock = { type: "text" as const, text: payload.text };
              return [...prev, payload.seq != null ? { ...newBlock, seq: payload.seq } : newBlock];
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
    // 4. Subscribe to query cache; when the refetch returns data containing the new message_id,
    //    call setIsFinalizing(false) and unsubscribe.
    // 5. Safety timeout (3s) clears isFinalizing if the query never returns the expected message.
    // Result: smooth swap with no fixed-delay race condition.
    unsubscribes.push(
      bus.subscribe<{
        conversation_id?: string;
        context_id?: string;
        context_type?: string;
        role?: string;
        message_id?: string;
      }>("agent:message_created", (payload) => {
        if (!payload.conversation_id) return;
        if (!isRelevant(payload)) return;

        if (isProviderRole(payload.role)) {
          const convId = payload.conversation_id;
          const assistantMessageId = payload.message_id;

          // Set isFinalizing=true in same batch as clearing streaming state
          setIsFinalizing(true);
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
            if (existing.some((message) => message.id === assistantMessageId)) {
              clearFinalizing();
            } else {
              // Subscribe to query cache updates — clear isFinalizing when the new
              // assistant message appears in the refetched conversation data.
              unsubscribeCache = queryClient.getQueryCache().subscribe((event) => {
                if (event.type !== "updated") return;
                const evKey = event.query.queryKey;
                if (!Array.isArray(evKey) || evKey.length < 3 || evKey[2] !== convId) return;
                const data = getCachedConversationMessages(queryClient, convId);
                if (data.some((message) => message.id === assistantMessageId)) {
                  clearFinalizing();
                }
              });
            }
          }
          // If no message_id in payload, the safety timeout alone handles cleanup
        }

        invalidateConversationDataQueries(queryClient, payload.conversation_id);
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

    // ── agent:usage_updated ─────────────────────────────────────────
    // Usage snapshots are persisted during the live turn; refetch stats immediately.
    unsubscribes.push(
      bus.subscribe<{
        conversation_id: string;
        context_id?: string;
        context_type?: string;
      }>("agent:usage_updated", (payload) => {
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
    bus, queryClient, activeConversationId, contextId, contextType,
    supportsStreamingText, supportsSubagentTasks,
    setStreamingToolCalls, setStreamingContentBlocks, setStreamingTasks,
    setIsFinalizing, storeKey,
  ]);
}
