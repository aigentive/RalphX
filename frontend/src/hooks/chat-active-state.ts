import { parseToolCalls, type ActiveStreamingTaskResponse } from "@/api/chat";
import type { ToolCall } from "@/components/Chat/ToolCallIndicator";
import {
  extractDelegationMetadata,
  isDelegationStartToolCall,
  reconcileDelegationTaskMap,
  reconcileDelegationTaskMarkers,
} from "@/components/Chat/delegation-tool-calls";
import type { StreamingContentBlock, StreamingTask } from "@/types/streaming-task";

interface ActiveDelegationAliases {
  providerIdByTaskId: Map<string, string>;
  promotedProviderIds: Set<string>;
}

function activeDelegationAliases(
  rawToolCalls: unknown[],
  tasks: ActiveStreamingTaskResponse[],
): ActiveDelegationAliases {
  const providerStarts = parseToolCalls(rawToolCalls).filter((toolCall) =>
    isDelegationStartToolCall(toolCall.name)
  );
  const providerIdByJob = new Map<string, string>();
  const unresolvedProviderIds: string[] = [];
  for (const toolCall of providerStarts) {
    const jobId = extractDelegationMetadata(toolCall.arguments, toolCall.result).jobId;
    if (jobId) providerIdByJob.set(jobId, toolCall.id);
    else unresolvedProviderIds.push(toolCall.id);
  }

  const delegatedTasks = tasks.filter((task) => task.delegated_job_id != null);
  const unmatchedDelegatedTasks = delegatedTasks.filter((task) =>
    task.delegated_job_id != null && !providerIdByJob.has(task.delegated_job_id)
  );
  const provisionalProviderId = unresolvedProviderIds.length === 1
    && unmatchedDelegatedTasks.length === 1
      ? unresolvedProviderIds[0]
      : undefined;
  const providerIdByTaskId = new Map<string, string>();
  const promotedProviderIds = new Set<string>();
  for (const task of delegatedTasks) {
    const providerId = task.delegated_job_id
      ? providerIdByJob.get(task.delegated_job_id) ?? provisionalProviderId
      : undefined;
    if (providerId) {
      providerIdByTaskId.set(task.tool_use_id, providerId);
      promotedProviderIds.add(providerId);
    }
  }
  return { providerIdByTaskId, promotedProviderIds };
}

function providerTaskMetadata(
  providerId: string | undefined,
  rawToolCalls: unknown[],
): { toolName?: string; description?: string } {
  if (!providerId) return {};
  const toolCall = parseToolCalls(rawToolCalls).find((call) => call.id === providerId);
  if (!toolCall) return {};
  const metadata = extractDelegationMetadata(toolCall.arguments, toolCall.result);
  return {
    toolName: toolCall.name,
    ...((metadata.title ?? metadata.prompt) != null
      ? { description: metadata.title ?? metadata.prompt }
      : {}),
  };
}

function mapActiveTaskToStreamingTask(
  task: ActiveStreamingTaskResponse,
  existing?: StreamingTask,
): StreamingTask {
  const isDelegated = task.delegated_job_id != null;
  const status = task.status as StreamingTask["status"];

  return {
    toolUseId: task.tool_use_id,
    toolName: existing?.toolName ?? (isDelegated ? "delegate_start" : "Task"),
    description: task.description ?? existing?.description ?? "",
    subagentType:
      task.subagent_type
      ?? existing?.subagentType
      ?? (isDelegated ? "delegated" : "unknown"),
    model:
      task.model
      ?? task.effective_model_id
      ?? task.logical_model
      ?? existing?.model
      ?? "unknown",
    status,
    startedAt: existing?.startedAt ?? Date.now(),
    childToolCalls: existing?.childToolCalls ?? [],
    ...(existing?.completedAt != null ? { completedAt: existing.completedAt } : {}),
    ...(task.total_tokens != null
      ? { totalTokens: task.total_tokens }
      : existing?.totalTokens != null
        ? { totalTokens: existing.totalTokens }
        : {}),
    ...(task.total_tool_uses != null
      ? { totalToolUseCount: task.total_tool_uses }
      : existing?.totalToolUseCount != null
        ? { totalToolUseCount: existing.totalToolUseCount }
        : {}),
    ...(task.duration_ms != null
      ? { totalDurationMs: task.duration_ms }
      : existing?.totalDurationMs != null
        ? { totalDurationMs: existing.totalDurationMs }
        : {}),
    ...(task.agent_id != null ? { agentId: task.agent_id } : existing?.agentId ? { agentId: existing.agentId } : {}),
    ...(task.delegated_job_id != null
      ? { delegatedJobId: task.delegated_job_id }
      : existing?.delegatedJobId
        ? { delegatedJobId: existing.delegatedJobId }
        : {}),
    ...(task.delegated_session_id != null
      ? { delegatedSessionId: task.delegated_session_id }
      : existing?.delegatedSessionId
        ? { delegatedSessionId: existing.delegatedSessionId }
        : {}),
    ...(task.delegated_conversation_id != null
      ? { delegatedConversationId: task.delegated_conversation_id }
      : existing?.delegatedConversationId
        ? { delegatedConversationId: existing.delegatedConversationId }
        : {}),
    ...(task.delegated_agent_run_id != null
      ? { delegatedAgentRunId: task.delegated_agent_run_id }
      : existing?.delegatedAgentRunId
        ? { delegatedAgentRunId: existing.delegatedAgentRunId }
        : {}),
    ...(task.provider_harness != null
      ? { providerHarness: task.provider_harness }
      : existing?.providerHarness
        ? { providerHarness: existing.providerHarness }
        : {}),
    ...(task.provider_session_id != null
      ? { providerSessionId: task.provider_session_id }
      : existing?.providerSessionId
        ? { providerSessionId: existing.providerSessionId }
        : {}),
    ...(task.upstream_provider != null
      ? { upstreamProvider: task.upstream_provider }
      : existing?.upstreamProvider
        ? { upstreamProvider: existing.upstreamProvider }
        : {}),
    ...(task.provider_profile != null
      ? { providerProfile: task.provider_profile }
      : existing?.providerProfile
        ? { providerProfile: existing.providerProfile }
        : {}),
    ...(task.logical_model != null
      ? { logicalModel: task.logical_model }
      : existing?.logicalModel
        ? { logicalModel: existing.logicalModel }
        : {}),
    ...(task.effective_model_id != null
      ? { effectiveModelId: task.effective_model_id }
      : existing?.effectiveModelId
        ? { effectiveModelId: existing.effectiveModelId }
        : {}),
    ...(task.logical_effort != null
      ? { logicalEffort: task.logical_effort }
      : existing?.logicalEffort
        ? { logicalEffort: existing.logicalEffort }
        : {}),
    ...(task.effective_effort != null
      ? { effectiveEffort: task.effective_effort }
      : existing?.effectiveEffort
        ? { effectiveEffort: existing.effectiveEffort }
        : {}),
    ...(task.approval_policy != null
      ? { approvalPolicy: task.approval_policy }
      : existing?.approvalPolicy
        ? { approvalPolicy: existing.approvalPolicy }
        : {}),
    ...(task.sandbox_mode != null
      ? { sandboxMode: task.sandbox_mode }
      : existing?.sandboxMode
        ? { sandboxMode: existing.sandboxMode }
        : {}),
    ...(task.input_tokens != null
      ? { inputTokens: task.input_tokens }
      : existing?.inputTokens != null
        ? { inputTokens: existing.inputTokens }
        : {}),
    ...(task.output_tokens != null
      ? { outputTokens: task.output_tokens }
      : existing?.outputTokens != null
        ? { outputTokens: existing.outputTokens }
        : {}),
    ...(task.cache_creation_tokens != null
      ? { cacheCreationTokens: task.cache_creation_tokens }
      : existing?.cacheCreationTokens != null
        ? { cacheCreationTokens: existing.cacheCreationTokens }
        : {}),
    ...(task.cache_read_tokens != null
      ? { cacheReadTokens: task.cache_read_tokens }
      : existing?.cacheReadTokens != null
        ? { cacheReadTokens: existing.cacheReadTokens }
        : {}),
    ...(task.estimated_usd != null
      ? { estimatedUsd: task.estimated_usd }
      : existing?.estimatedUsd != null
        ? { estimatedUsd: existing.estimatedUsd }
        : {}),
    ...(task.text_output != null
      ? { textOutput: task.text_output }
      : existing?.textOutput
        ? { textOutput: existing.textOutput }
        : {}),
    ...(existing?.seq != null ? { seq: existing.seq } : {}),
  };
}

export function mergeActiveStreamingTasks(
  previous: Map<string, StreamingTask>,
  tasks: ActiveStreamingTaskResponse[],
  rawToolCalls: unknown[] = [],
): Map<string, StreamingTask> {
  if (tasks.length === 0) {
    return previous;
  }

  let next = new Map(previous);
  const aliases = activeDelegationAliases(rawToolCalls, tasks);
  for (const task of tasks) {
    const providerToolUseId = aliases.providerIdByTaskId.get(task.tool_use_id);
    const provider = providerTaskMetadata(providerToolUseId, rawToolCalls);
    const existing = previous.get(providerToolUseId ?? task.tool_use_id)
      ?? [...previous.values()].find((candidate) =>
        task.delegated_job_id != null && candidate.delegatedJobId === task.delegated_job_id
      );
    const mapped = mapActiveTaskToStreamingTask(task, existing);
    const delegatedJobId = task.delegated_job_id ?? existing?.delegatedJobId;
    if (delegatedJobId) mapped.delegatedJobId = delegatedJobId;
    if (provider.toolName) mapped.toolName = provider.toolName;
    if (provider.description) mapped.description = provider.description;
    if (delegatedJobId != null) {
      next = reconcileDelegationTaskMap(next, {
        source: "active-state",
        toolUseId: task.tool_use_id,
        ...(providerToolUseId ? { providerToolUseId } : {}),
        jobId: delegatedJobId,
        allowSingleUnresolvedPlaceholder: true,
        task: mapped,
      }).tasks;
    } else {
      next.set(task.tool_use_id, mapped);
    }
  }
  return next;
}

export function mergeActiveStreamingToolCalls(
  previous: ToolCall[],
  rawToolCalls: unknown[],
  tasks: ActiveStreamingTaskResponse[] = [],
): ToolCall[] {
  const toolCalls = parseToolCalls(rawToolCalls);
  if (toolCalls.length === 0) {
    return previous;
  }

  const next = [...previous];
  for (const toolCall of toolCalls) {
    const existingIndex = next.findIndex((existing) => existing.id === toolCall.id);
    const existing = existingIndex >= 0 ? next[existingIndex] : undefined;
    if (existing) {
      next[existingIndex] = { ...existing, ...toolCall };
    } else {
      next.push(toolCall);
    }
  }
  const aliases = activeDelegationAliases(rawToolCalls, tasks);
  return next.filter((toolCall) => !aliases.promotedProviderIds.has(toolCall.id));
}

function mergePartialTextBlock(
  previous: StreamingContentBlock[],
  partialText: string,
): StreamingContentBlock[] {
  if (partialText.trim().length === 0) {
    return previous;
  }

  const next = [...previous];
  const textIndex = next.findIndex((block) => block.type === "text");
  if (textIndex < 0) {
    return [...next, { type: "text", text: partialText }];
  }

  const existing = next[textIndex] as Extract<StreamingContentBlock, { type: "text" }>;

  const mergedText = mergeStreamingTextSnapshot(partialText, existing.text);
  if (mergedText !== existing.text) {
    next[textIndex] = { ...existing, text: mergedText };
  }
  return next;
}

export function mergeStreamingTextSnapshot(snapshotText: string, liveText: string): string {
  if (liveText.length === 0) {
    return snapshotText;
  }
  if (snapshotText === liveText) {
    return liveText;
  }
  if (snapshotText.startsWith(liveText) || snapshotText.endsWith(liveText)) {
    return snapshotText;
  }
  if (liveText.startsWith(snapshotText) || liveText.endsWith(snapshotText)) {
    return liveText;
  }

  const snapshotThenLiveOverlap = longestSuffixPrefixOverlap(snapshotText, liveText);
  const liveThenSnapshotOverlap = longestSuffixPrefixOverlap(liveText, snapshotText);
  if (liveThenSnapshotOverlap > snapshotThenLiveOverlap) {
    return liveText + snapshotText.slice(liveThenSnapshotOverlap);
  }
  return snapshotText + liveText.slice(snapshotThenLiveOverlap);
}

function longestSuffixPrefixOverlap(left: string, right: string): number {
  const maxLength = Math.min(left.length, right.length);
  for (let length = maxLength; length > 0; length -= 1) {
    if (left.endsWith(right.slice(0, length))) {
      return length;
    }
  }
  return 0;
}

function mergeTaskMarker(
  previous: StreamingContentBlock[],
  toolUseId: string,
): StreamingContentBlock[] {
  if (previous.some((block) => block.type === "task" && block.toolUseId === toolUseId)) {
    return previous;
  }
  return [...previous, { type: "task", toolUseId }];
}

function mergeToolCallBlock(
  previous: StreamingContentBlock[],
  toolCall: ToolCall,
): StreamingContentBlock[] {
  const next = [...previous];
  const existingIndex = next.findIndex(
    (block) => block.type === "tool_use" && block.toolCall.id === toolCall.id,
  );
  if (existingIndex >= 0) {
    next[existingIndex] = { type: "tool_use", toolCall };
    return next;
  }
  return [...next, { type: "tool_use", toolCall }];
}

export function mergeActiveStreamingContentBlocks(
  previous: StreamingContentBlock[],
  activeState: {
    partial_text: string;
    tool_calls: unknown[];
    streaming_tasks: ActiveStreamingTaskResponse[];
  },
): StreamingContentBlock[] {
  let next = mergePartialTextBlock(previous, activeState.partial_text);
  const aliases = activeDelegationAliases(
    activeState.tool_calls,
    activeState.streaming_tasks,
  );
  const taskToolUseIds = new Set(activeState.streaming_tasks.flatMap((task) => [
    task.tool_use_id,
    aliases.providerIdByTaskId.get(task.tool_use_id),
  ].filter((id): id is string => id != null)));

  for (const task of activeState.streaming_tasks) {
    const providerToolUseId = aliases.providerIdByTaskId.get(task.tool_use_id);
    if (task.delegated_job_id != null) {
      next = reconcileDelegationTaskMarkers(next, {
        canonicalKey: providerToolUseId ?? task.tool_use_id,
        aliasKeys: [
          task.tool_use_id,
          providerToolUseId,
          `delegate-job:${task.delegated_job_id}`,
        ].filter((id): id is string => id != null),
      });
    } else {
      next = mergeTaskMarker(next, task.tool_use_id);
    }
  }

  for (const toolCall of parseToolCalls(activeState.tool_calls)) {
    if (taskToolUseIds.has(toolCall.id)) {
      continue;
    }
    next = mergeToolCallBlock(next, toolCall);
  }

  return next;
}
