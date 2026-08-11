import type { ToolCall } from "./ToolCallIndicator";
import type { ChatMessageData } from "./ChatMessageList";
import type { StreamingTask } from "@/types/streaming-task";
import {
  extractDelegationMetadata,
  isDelegationControlToolCall,
  isDelegationStartToolCall,
  mergeDelegationContentBlocks,
  mergeDelegationToolCalls,
} from "./delegation-tool-calls";

export function delegationJobIdForToolCall(toolCall: ToolCall): string | undefined {
  return extractDelegationMetadata(toolCall.arguments, toolCall.result).jobId;
}

export function persistedDelegationJobIds(messages: readonly ChatMessageData[]): Set<string> {
  const jobIds = new Set<string>();
  for (const message of messages) {
    const toolCall = persistedTimelineToolCall(message);
    if (!toolCall || (!isDelegationStartToolCall(toolCall.name) && !isDelegationControlToolCall(toolCall.name))) {
      continue;
    }
    const jobId = delegationJobIdForToolCall(toolCall);
    if (jobId) jobIds.add(jobId);
  }
  return jobIds;
}

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
  const suppressed = new Set<number>();
  const folded = [...messages];
  const representativeIndexByJobId = new Map<string, number>();

  messages.forEach((message, index) => {
    const lifecycle = persistedTimelineToolCall(message);
    if (
      !lifecycle
      || (!isDelegationStartToolCall(lifecycle.name) && !isDelegationControlToolCall(lifecycle.name))
    ) {
      return;
    }
    const jobId = delegationJobIdForToolCall(lifecycle);
    if (!jobId) {
      if (!isDelegationControlToolCall(lifecycle.name)) return;
      folded[index] = {
        ...message,
        contentBlocks: mergeDelegationContentBlocks(message.contentBlocks ?? []),
        toolCalls: mergeDelegationToolCalls(message.toolCalls ?? [lifecycle]),
      };
      return;
    }

    const representativeIndex = representativeIndexByJobId.get(jobId);
    if (representativeIndex == null) {
      representativeIndexByJobId.set(jobId, index);
      folded[index] = {
        ...message,
        contentBlocks: mergeDelegationContentBlocks(message.contentBlocks ?? []),
        toolCalls: mergeDelegationToolCalls(message.toolCalls ?? [lifecycle]),
      };
      return;
    }

    const representative = folded[representativeIndex];
    if (!representative) return;
    const representativeToolCall = persistedTimelineToolCall(representative);
    folded[representativeIndex] = {
      ...representative,
      contentBlocks: mergeDelegationContentBlocks([
        ...(representative.contentBlocks ?? []),
        ...(message.contentBlocks ?? []),
      ]),
      toolCalls: mergeDelegationToolCalls([
        ...(representative.toolCalls ?? (representativeToolCall ? [representativeToolCall] : [])),
        ...(message.toolCalls ?? [lifecycle]),
      ]),
    };
    suppressed.add(index);
  });

  return folded.filter((_message, index) => !suppressed.has(index));
}

export interface DelegationTimelineProjection {
  messages: ChatMessageData[];
  /** Live task and marker keys represented by persisted delegation cards. */
  liveAliases: Set<string>;
}

function isTerminalDelegationStatus(status: string | undefined): boolean {
  return status === "completed" || status === "failed" || status === "cancelled";
}

function resultRecord(value: unknown): Record<string, unknown> {
  return value != null && typeof value === "object" && !Array.isArray(value)
    ? value as Record<string, unknown>
    : {};
}

function enrichedDelegationResult(
  persisted: ToolCall,
  live: StreamingTask,
): Record<string, unknown> {
  const result = resultRecord(persisted.result);
  const persistedMetadata = extractDelegationMetadata(persisted.arguments, persisted.result);
  const persistedTerminal = isTerminalDelegationStatus(persistedMetadata.status);
  const status = persistedTerminal ? persistedMetadata.status : live.status;
  const terminal = isTerminalDelegationStatus(status);
  const hasBackendClock = live.clockSource === "delegated-run"
    || live.clockSource === "delegation-job";

  return {
    ...result,
    job_id: persistedMetadata.jobId ?? live.delegatedJobId,
    status,
    ...(live.delegatedSessionId ? { delegated_session_id: live.delegatedSessionId } : {}),
    ...(live.delegatedConversationId ? { delegated_conversation_id: live.delegatedConversationId } : {}),
    ...(live.delegatedAgentRunId ? { delegated_agent_run_id: live.delegatedAgentRunId } : {}),
    ...(live.providerHarness ? { harness: live.providerHarness } : {}),
    ...(live.providerSessionId ? { provider_session_id: live.providerSessionId } : {}),
    ...(live.upstreamProvider ? { upstream_provider: live.upstreamProvider } : {}),
    ...(live.providerProfile ? { provider_profile: live.providerProfile } : {}),
    ...(live.logicalModel ? { logical_model: live.logicalModel } : {}),
    ...(live.effectiveModelId ? { effective_model_id: live.effectiveModelId } : {}),
    ...(live.logicalEffort ? { logical_effort: live.logicalEffort } : {}),
    ...(live.effectiveEffort ? { effective_effort: live.effectiveEffort } : {}),
    ...(live.approvalPolicy ? { approval_policy: live.approvalPolicy } : {}),
    ...(live.sandboxMode ? { sandbox_mode: live.sandboxMode } : {}),
    ...(live.inputTokens != null ? { input_tokens: live.inputTokens } : {}),
    ...(live.outputTokens != null ? { output_tokens: live.outputTokens } : {}),
    ...(live.cacheCreationTokens != null ? { cache_creation_tokens: live.cacheCreationTokens } : {}),
    ...(live.cacheReadTokens != null ? { cache_read_tokens: live.cacheReadTokens } : {}),
    ...(live.totalTokens != null ? { total_tokens: live.totalTokens } : {}),
    ...(live.totalDurationMs != null ? { total_duration_ms: live.totalDurationMs } : {}),
    ...(live.estimatedUsd != null ? { estimated_usd: live.estimatedUsd } : {}),
    ...(!persistedTerminal && live.textOutput ? { content: live.textOutput } : {}),
    ...(hasBackendClock && terminal && live.completedAt != null
      ? { completed_at: new Date(live.completedAt).toISOString() }
      : {}),
    ...(hasBackendClock && live.startedAt > 0
      ? {
        started_at: new Date(live.startedAt).toISOString(),
        timestamp_provenance: live.clockSource === "delegated-run"
          ? "delegated_run"
          : "delegation_job",
      }
      : {}),
  };
}

/**
 * Keeps persisted placement and transcript authority while allowing newer live
 * lifecycle evidence to enrich the one persisted representative for each job.
 */
export function projectDelegationTimelineMessages(
  messages: ChatMessageData[],
  streamingTasks: ReadonlyMap<string, StreamingTask> | undefined,
): DelegationTimelineProjection {
  const folded = foldDelegationTimelineMessages(messages);
  if (!streamingTasks || streamingTasks.size === 0) {
    return { messages: folded, liveAliases: new Set() };
  }

  const liveByJobId = new Map<string, StreamingTask>();
  const liveByToolUseId = new Map<string, StreamingTask>();
  const retainMostCompleteTask = (
    target: Map<string, StreamingTask>,
    key: string,
    task: StreamingTask,
  ) => {
    const current = target.get(key);
    if (!current || (!isTerminalDelegationStatus(current.status) && isTerminalDelegationStatus(task.status))) {
      target.set(key, task);
    }
  };
  for (const [key, task] of streamingTasks) {
    retainMostCompleteTask(liveByToolUseId, key, task);
    retainMostCompleteTask(liveByToolUseId, task.toolUseId, task);
    if (task.delegatedJobId) {
      retainMostCompleteTask(liveByJobId, task.delegatedJobId, task);
    }
  }

  const liveAliases = new Set<string>();
  const projected = folded.map((message) => {
    const persisted = persistedTimelineToolCall(message);
    if (!persisted || !isDelegationStartToolCall(persisted.name)) return message;
    const jobId = delegationJobIdForToolCall(persisted);
    const live = (jobId ? liveByJobId.get(jobId) : undefined)
      ?? liveByToolUseId.get(persisted.id);
    if (!live) return message;

    liveAliases.add(live.toolUseId);
    for (const [key, task] of streamingTasks) {
      if (
        (jobId != null && task.delegatedJobId === jobId)
        || task.toolUseId === live.toolUseId
        || key === persisted.id
      ) {
        liveAliases.add(key);
      }
    }
    const result = enrichedDelegationResult(persisted, live);
    const updateTool = <T extends ToolCall>(toolCall: T): T =>
      toolCall.id === persisted.id ? { ...toolCall, result } : toolCall;
    const contentBlocks = message.contentBlocks?.map((block) =>
      block.type === "tool_use" && (block.id === persisted.id || !block.id)
        ? { ...block, result }
        : block,
    );
    const toolCalls = message.toolCalls?.map(updateTool);
    return {
      ...message,
      ...(contentBlocks ? { contentBlocks } : {}),
      ...(toolCalls ? { toolCalls } : {}),
    };
  });

  return { messages: projected, liveAliases };
}
