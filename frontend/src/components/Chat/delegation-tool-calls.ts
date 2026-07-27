import type { ToolCall } from "./tool-widgets/shared.constants";
import { parseMcpToolResultRaw } from "./tool-widgets/shared.constants";
import { canonicalizeToolName } from "./tool-widgets/tool-name";
import type { StreamingContentBlock, StreamingTask } from "@/types/streaming-task";

export const DELEGATION_START_TOOL_NAME = "delegate_start";
export const DELEGATION_WAIT_TOOL_NAME = "delegate_wait";
export const DELEGATION_CANCEL_TOOL_NAME = "delegate_cancel";
export const DELEGATION_TERMINAL_TOOL_NAME = "delegate_terminal";

type UnknownRecord = Record<string, unknown>;

export interface DelegationMetadata {
  jobId?: string;
  status?: string;
  agentName?: string;
  prompt?: string;
  title?: string;
  providerHarness?: string;
  providerSessionId?: string;
  upstreamProvider?: string;
  providerProfile?: string;
  delegatedSessionId?: string;
  delegatedConversationId?: string;
  delegatedAgentRunId?: string;
  logicalModel?: string;
  effectiveModelId?: string;
  logicalEffort?: string;
  effectiveEffort?: string;
  approvalPolicy?: string;
  sandboxMode?: string;
  inputTokens?: number;
  outputTokens?: number;
  cacheCreationTokens?: number;
  cacheReadTokens?: number;
  totalTokens?: number;
  estimatedUsd?: number;
  durationMs?: number;
  startedAt?: number;
  completedAt?: number;
  clockSource?: "delegated-run" | "delegation-job";
  textOutput?: string;
  assignment?: {
    taskNumber?: number;
    title?: string;
    taskState?: string;
    assignmentState?: string;
    delegateAgentName?: string;
  };
}

export type DelegationEvidenceSource =
  | "provider"
  | "lifecycle-start"
  | "lifecycle-complete"
  | "active-state";

export interface ReconcileDelegationTaskInput {
  source: DelegationEvidenceSource;
  toolUseId: string;
  providerToolUseId?: string;
  jobId?: string;
  seq?: number;
  allowSingleUnresolvedPlaceholder?: boolean;
  task: StreamingTask;
}

export interface DelegationTaskReconciliation {
  tasks: Map<string, StreamingTask>;
  canonicalKey: string;
  aliasKeys: string[];
}

export interface DelegationLifecycleTaskPayload {
  tool_use_id: string;
  tool_name?: string;
  description?: string;
  subagent_type?: string;
  model?: string;
  status?: string;
  agent_id?: string;
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
  seq?: number;
}

type DelegationMergeable = {
  name?: string;
  arguments?: unknown;
  result?: unknown;
  error?: string;
};

interface NormalizeDelegationTranscriptPayloadArgs<
  TContentBlock extends DelegationMergeable,
  TToolCall extends ToolCall,
> {
  contentBlocks?: TContentBlock[] | null | undefined;
  toolCalls?: TToolCall[] | null | undefined;
}

interface NormalizedDelegationTranscriptPayload<
  TContentBlock extends DelegationMergeable,
  TToolCall extends ToolCall,
> {
  contentBlocks: TContentBlock[];
  toolCalls: TToolCall[];
}

function asRecord(value: unknown): UnknownRecord | null {
  return value != null && typeof value === "object" && !Array.isArray(value)
    ? (value as UnknownRecord)
    : null;
}

function getFirstRecord(record: UnknownRecord | null, ...keys: string[]): UnknownRecord | null {
  if (!record) return null;
  for (const key of keys) {
    const nested = asRecord(record[key]);
    if (nested) return nested;
  }
  return null;
}

function getFirstString(record: UnknownRecord | null, ...keys: string[]): string | undefined {
  if (!record) return undefined;
  for (const key of keys) {
    const value = record[key];
    if (typeof value === "string" && value.length > 0) {
      return value;
    }
  }
  return undefined;
}

function getFirstNumber(record: UnknownRecord | null, ...keys: string[]): number | undefined {
  if (!record) return undefined;
  for (const key of keys) {
    const value = record[key];
    if (typeof value === "number" && Number.isFinite(value)) {
      return value;
    }
  }
  return undefined;
}

function getLastMessageText(messages: unknown): string | undefined {
  if (!Array.isArray(messages)) return undefined;
  for (let index = messages.length - 1; index >= 0; index -= 1) {
    const message = asRecord(messages[index]);
    const content = getFirstString(message, "content");
    if (content) return content;
  }
  return undefined;
}

function getContentText(content: unknown): string | undefined {
  if (typeof content === "string" && content.length > 0) {
    return content;
  }
  if (!Array.isArray(content)) return undefined;
  for (let index = content.length - 1; index >= 0; index -= 1) {
    const entry = asRecord(content[index]);
    const text = getFirstString(entry, "text");
    if (text) return text;
  }
  return undefined;
}

const NO_OUTPUT_ERROR_PREFIX = "Codex exited without a response";

function formatNoOutputDelegationFailure(error: string | undefined): string | undefined {
  if (!error?.startsWith(NO_OUTPUT_ERROR_PREFIX)) return undefined;

  const details: string[] = [];
  const exitCode = error.match(/\bcode=Some\((-?\d+)\)/)?.[1];
  const exitSignal = error.match(/\bsignal=Some\((-?\d+)\)/)?.[1];
  if (exitCode) details.push(`Exit code: ${exitCode}`);
  if (exitSignal) details.push(`Exit signal: ${exitSignal}`);

  return details.length > 0
    ? `Delegate completed without a response\n\n${details.join("\n")}`
    : "Delegate completed without a response";
}

function deriveDurationMs(startedAt?: string, completedAt?: string): number | undefined {
  if (!startedAt || !completedAt) return undefined;
  const started = Date.parse(startedAt);
  const completed = Date.parse(completedAt);
  if (!Number.isFinite(started) || !Number.isFinite(completed) || completed < started) {
    return undefined;
  }
  return completed - started;
}

export function parseDelegationTimestamp(value: string | undefined): number | undefined {
  if (!value) return undefined;
  const parsed = Date.parse(value);
  return Number.isFinite(parsed) ? parsed : undefined;
}

function normalizeStatus(status: string | undefined): string | undefined {
  switch (status) {
    case "running":
    case "completed":
    case "failed":
    case "cancelled":
      return status;
    default:
      return status;
  }
}

function toDelegationStartName(name: string): string {
  const canonical = canonicalizeToolName(name);
  if (canonical === DELEGATION_START_TOOL_NAME) {
    return name;
  }
  if (
    canonical === DELEGATION_WAIT_TOOL_NAME
    || canonical === DELEGATION_CANCEL_TOOL_NAME
    || canonical === DELEGATION_TERMINAL_TOOL_NAME
  ) {
    const namespaceSeparator = name.lastIndexOf("::");
    if (namespaceSeparator >= 0) {
      return `${name.slice(0, namespaceSeparator + 2)}${DELEGATION_START_TOOL_NAME}`;
    }
    return DELEGATION_START_TOOL_NAME;
  }
  return name;
}

export function isDelegationStartToolCall(name: string): boolean {
  return canonicalizeToolName(name) === DELEGATION_START_TOOL_NAME;
}

export function isDelegationControlToolCall(name: string): boolean {
  const canonical = canonicalizeToolName(name);
  return canonical === DELEGATION_WAIT_TOOL_NAME
    || canonical === DELEGATION_CANCEL_TOOL_NAME
    || canonical === DELEGATION_TERMINAL_TOOL_NAME;
}

export function isDelegationToolCall(name: string): boolean {
  return isDelegationStartToolCall(name) || isDelegationControlToolCall(name);
}

export function parseToolResultId(name: string): string | undefined {
  if (!name.startsWith("result:")) return undefined;
  const toolUseId = name.slice("result:".length).trim();
  return toolUseId.length > 0 ? toolUseId : undefined;
}

export function findDelegationTaskKey(
  tasks: ReadonlyMap<string, StreamingTask>,
  toolUseId: string | undefined,
  jobId: string | undefined,
): string | undefined {
  if (toolUseId && tasks.has(toolUseId)) return toolUseId;
  if (!jobId) return undefined;
  for (const [key, task] of tasks) {
    if (task.delegatedJobId === jobId) return key;
  }
  return undefined;
}

function isTerminalStatus(status: StreamingTask["status"]): boolean {
  return status === "completed" || status === "failed" || status === "cancelled";
}

function lifecycleStatus(status: string | undefined): StreamingTask["status"] | undefined {
  switch (status) {
    case "running":
    case "completed":
    case "failed":
    case "cancelled":
      return status;
    default:
      return undefined;
  }
}

export function buildDelegationLifecycleTask(
  payload: DelegationLifecycleTaskPayload,
  existing?: StreamingTask,
  now = Date.now(),
): StreamingTask {
  const delegated = payload.delegated_job_id != null || payload.subagent_type === "delegated";
  const status = lifecycleStatus(payload.status) ?? existing?.status ?? "running";
  const startedAt = parseDelegationTimestamp(payload.started_at) ?? existing?.startedAt ?? now;
  const parsedCompletedAt = parseDelegationTimestamp(payload.completed_at) ?? existing?.completedAt;
  const completedAt = parsedCompletedAt != null && parsedCompletedAt >= startedAt
    ? parsedCompletedAt
    : undefined;
  return {
    ...(existing ?? {
      toolUseId: payload.tool_use_id,
      toolName: payload.tool_name ?? (delegated ? "delegate_start" : "Task"),
      description: payload.description ?? "",
      subagentType: payload.subagent_type ?? (delegated ? "delegated" : "unknown"),
      model: payload.model ?? payload.effective_model_id ?? payload.logical_model ?? "unknown",
      status: "running",
      startedAt,
      childToolCalls: [],
    }),
    status,
    startedAt,
    ...(payload.tool_name != null ? { toolName: payload.tool_name } : {}),
    ...(payload.description != null ? { description: payload.description } : {}),
    ...(payload.subagent_type != null ? { subagentType: payload.subagent_type } : {}),
    ...(payload.model != null ? { model: payload.model } : {}),
    ...(payload.agent_id != null ? { agentId: payload.agent_id } : {}),
    ...(payload.delegated_job_id != null ? { delegatedJobId: payload.delegated_job_id } : {}),
    ...(payload.delegated_session_id != null
      ? { delegatedSessionId: payload.delegated_session_id }
      : {}),
    ...(payload.delegated_conversation_id != null
      ? { delegatedConversationId: payload.delegated_conversation_id }
      : {}),
    ...(payload.delegated_agent_run_id != null
      ? { delegatedAgentRunId: payload.delegated_agent_run_id }
      : {}),
    ...(payload.provider_harness != null ? { providerHarness: payload.provider_harness } : {}),
    ...(payload.provider_session_id != null
      ? { providerSessionId: payload.provider_session_id }
      : {}),
    ...(payload.upstream_provider != null ? { upstreamProvider: payload.upstream_provider } : {}),
    ...(payload.provider_profile != null ? { providerProfile: payload.provider_profile } : {}),
    ...(payload.logical_model != null ? { logicalModel: payload.logical_model } : {}),
    ...(payload.effective_model_id != null ? { effectiveModelId: payload.effective_model_id } : {}),
    ...(payload.logical_effort != null ? { logicalEffort: payload.logical_effort } : {}),
    ...(payload.effective_effort != null ? { effectiveEffort: payload.effective_effort } : {}),
    ...(payload.approval_policy != null ? { approvalPolicy: payload.approval_policy } : {}),
    ...(payload.sandbox_mode != null ? { sandboxMode: payload.sandbox_mode } : {}),
    ...(payload.total_duration_ms != null ? { totalDurationMs: payload.total_duration_ms } : {}),
    ...(payload.total_tokens != null ? { totalTokens: payload.total_tokens } : {}),
    ...(payload.total_tool_use_count != null
      ? { totalToolUseCount: payload.total_tool_use_count }
      : {}),
    ...(payload.input_tokens != null ? { inputTokens: payload.input_tokens } : {}),
    ...(payload.output_tokens != null ? { outputTokens: payload.output_tokens } : {}),
    ...(payload.cache_creation_tokens != null
      ? { cacheCreationTokens: payload.cache_creation_tokens }
      : {}),
    ...(payload.cache_read_tokens != null ? { cacheReadTokens: payload.cache_read_tokens } : {}),
    ...(payload.estimated_usd != null ? { estimatedUsd: payload.estimated_usd } : {}),
    ...(payload.text_output != null
      ? { textOutput: payload.text_output }
      : payload.error != null
        ? { textOutput: formatNoOutputDelegationFailure(payload.error) ?? payload.error }
        : {}),
    ...(payload.seq != null ? { seq: payload.seq } : {}),
    ...(payload.timestamp_provenance === "delegated_run"
      ? { clockSource: "delegated-run" as const }
      : payload.timestamp_provenance === "delegation_job"
        ? { clockSource: "delegation-job" as const }
        : existing?.clockSource
          ? { clockSource: existing.clockSource }
          : { clockSource: "local-fallback" as const }),
    ...(isTerminalStatus(status) ? { completedAt: completedAt ?? now } : {}),
  };
}

function isDelegationTask(task: StreamingTask): boolean {
  return task.delegatedJobId != null
    || task.subagentType === "delegated"
    || isDelegationStartToolCall(task.toolName);
}

function uniqueStrings(values: Iterable<string | undefined>): string[] {
  const unique = new Set<string>();
  for (const value of values) {
    if (value) unique.add(value);
  }
  return [...unique];
}

function resolveDelegationAliases(
  tasks: ReadonlyMap<string, StreamingTask>,
  input: ReconcileDelegationTaskInput,
): { canonicalKey: string; aliasKeys: string[] } {
  const exactAliases: string[] = [];
  const jobAliases: string[] = [];
  for (const [key, task] of tasks) {
    if (
      key === input.toolUseId
      || task.toolUseId === input.toolUseId
      || (input.providerToolUseId != null
        && (key === input.providerToolUseId || task.toolUseId === input.providerToolUseId))
    ) {
      exactAliases.push(key);
    }
    if (input.jobId && task.delegatedJobId === input.jobId) {
      jobAliases.push(key);
    }
  }

  let provisionalAlias: string | undefined;
  if (
    input.allowSingleUnresolvedPlaceholder
    && input.jobId
    && jobAliases.length === 0
  ) {
    const unresolved = [...tasks.entries()].filter(([, task]) =>
      isDelegationTask(task) && !task.delegatedJobId
    );
    if (unresolved.length === 1) {
      provisionalAlias = unresolved[0]?.[0];
    }
  }

  const linkedProviderKey = input.providerToolUseId
    ?? (input.source === "provider" ? input.toolUseId : provisionalAlias);
  const exactProviderAlias = linkedProviderKey
    ? [...exactAliases, ...jobAliases].find((key) => key === linkedProviderKey)
      ?? [...tasks.entries()].find(([, task]) => task.toolUseId === linkedProviderKey)?.[0]
    : undefined;
  const nonSyntheticJobAlias = jobAliases.find((key) => !key.startsWith("delegate-job:"));
  const canonicalKey = linkedProviderKey
    ?? exactProviderAlias
    ?? nonSyntheticJobAlias
    ?? exactAliases[0]
    ?? jobAliases[0]
    ?? input.toolUseId;
  const syntheticKey = input.jobId ? `delegate-job:${input.jobId}` : undefined;

  return {
    canonicalKey,
    aliasKeys: uniqueStrings([
      ...exactAliases,
      ...jobAliases,
      provisionalAlias,
      input.toolUseId,
      input.providerToolUseId,
      syntheticKey,
    ]),
  };
}

function terminalSourceRank(source: StreamingTask["delegationTerminalSource"]): number {
  switch (source) {
    case "lifecycle-complete":
      return 3;
    case "provider":
      return 2;
    case "active-state":
      return 1;
    default:
      return 0;
  }
}

function pickTerminalTask(
  candidates: StreamingTask[],
): StreamingTask | undefined {
  return candidates.filter((task) => isTerminalStatus(task.status)).sort((left, right) => {
    if (left.seq != null || right.seq != null) {
      if (left.seq == null) return 1;
      if (right.seq == null) return -1;
      if (left.seq !== right.seq) return right.seq - left.seq;
    }
    return terminalSourceRank(right.delegationTerminalSource)
      - terminalSourceRank(left.delegationTerminalSource);
  })[0];
}

function firstDefined<T>(values: Array<T | undefined>): T | undefined {
  return values.find((value): value is T => value !== undefined);
}

function mergeDelegationTasks(
  existingTasks: StreamingTask[],
  input: ReconcileDelegationTaskInput,
  canonicalKey: string,
): StreamingTask {
  const incoming: StreamingTask = {
    ...input.task,
    ...(input.seq != null ? { seq: input.seq } : {}),
    ...(isTerminalStatus(input.task.status) && input.source !== "lifecycle-start"
      ? { delegationTerminalSource: input.source }
      : {}),
  };
  const candidates = [...existingTasks, incoming];
  const providerTask = candidates.find((task) =>
    task.toolUseId === (input.providerToolUseId ?? (input.source === "provider" ? input.toolUseId : canonicalKey))
    && task.description.trim().length > 0
  );
  const merged = candidates.reduce<StreamingTask>(
    (current, task) => ({ ...current, ...task }),
    incoming,
  );
  const childCalls = new Map<string, ToolCall>();
  for (const task of candidates) {
    for (const child of task.childToolCalls) {
      childCalls.set(child.id, { ...childCalls.get(child.id), ...child });
    }
  }
  const terminal = pickTerminalTask(candidates);
  const terminalCandidates = terminal
    ? [terminal, ...candidates.filter((task) => task !== terminal)]
    : candidates;
  const description = providerTask?.description
    ?? candidates.map((task) => task.description).find((value) => value.trim().length > 0)
    ?? "";
  const providerToolName = providerTask?.toolName
    ?? candidates.map((task) => task.toolName).find((name) => isDelegationStartToolCall(name))
    ?? merged.toolName;
  const startedAt = Math.min(...candidates.map((task) => task.startedAt));
  const clockSource = candidates.find((task) => task.clockSource === "delegated-run")?.clockSource
    ?? candidates.find((task) => task.clockSource === "delegation-job")?.clockSource
    ?? candidates.find((task) => task.clockSource)?.clockSource;
  const seq = terminal?.seq
    ?? candidates.reduce<number | undefined>(
      (latest, task) => task.seq == null ? latest : Math.max(latest ?? task.seq, task.seq),
      undefined,
    );
  const completedAt = firstDefined(terminalCandidates.map((task) => task.completedAt));
  const totalDurationMs = firstDefined(
    terminalCandidates.map((task) => task.totalDurationMs),
  );
  const totalTokens = firstDefined(terminalCandidates.map((task) => task.totalTokens));
  const totalToolUseCount = firstDefined(
    terminalCandidates.map((task) => task.totalToolUseCount),
  );
  const agentId = firstDefined(terminalCandidates.map((task) => task.agentId));
  const inputTokens = firstDefined(terminalCandidates.map((task) => task.inputTokens));
  const outputTokens = firstDefined(terminalCandidates.map((task) => task.outputTokens));
  const cacheCreationTokens = firstDefined(
    terminalCandidates.map((task) => task.cacheCreationTokens),
  );
  const cacheReadTokens = firstDefined(
    terminalCandidates.map((task) => task.cacheReadTokens),
  );
  const estimatedUsd = firstDefined(terminalCandidates.map((task) => task.estimatedUsd));
  const textOutput = firstDefined(terminalCandidates.map((task) => task.textOutput));

  return {
    ...merged,
    toolUseId: canonicalKey,
    toolName: providerToolName,
    description,
    startedAt,
    ...(clockSource ? { clockSource } : {}),
    childToolCalls: [...childCalls.values()],
    status: terminal?.status ?? merged.status,
    ...(seq != null ? { seq } : {}),
    ...(terminal?.delegationTerminalSource
      ? { delegationTerminalSource: terminal.delegationTerminalSource }
      : {}),
    ...(completedAt != null ? { completedAt } : {}),
    ...(totalDurationMs != null ? { totalDurationMs } : {}),
    ...(totalTokens != null ? { totalTokens } : {}),
    ...(totalToolUseCount != null ? { totalToolUseCount } : {}),
    ...(agentId ? { agentId } : {}),
    ...(inputTokens != null ? { inputTokens } : {}),
    ...(outputTokens != null ? { outputTokens } : {}),
    ...(cacheCreationTokens != null ? { cacheCreationTokens } : {}),
    ...(cacheReadTokens != null ? { cacheReadTokens } : {}),
    ...(estimatedUsd != null ? { estimatedUsd } : {}),
    ...(textOutput ? { textOutput } : {}),
  };
}

export function reconcileDelegationTaskMap(
  previous: ReadonlyMap<string, StreamingTask>,
  input: ReconcileDelegationTaskInput,
): DelegationTaskReconciliation {
  const identity = resolveDelegationAliases(previous, input);
  const existingTasks = identity.aliasKeys
    .map((key) => previous.get(key))
    .filter((task): task is StreamingTask => task != null);
  const mergedTask = mergeDelegationTasks(existingTasks, input, identity.canonicalKey);
  const next = new Map(previous);
  for (const alias of identity.aliasKeys) {
    next.delete(alias);
  }
  next.set(identity.canonicalKey, mergedTask);
  return { tasks: next, ...identity };
}

export function reconcileDelegationTaskMarkers(
  previous: StreamingContentBlock[],
  identity: {
    canonicalKey: string;
    aliasKeys: readonly string[];
    seq?: number;
    receivedAt?: number;
  },
): StreamingContentBlock[] {
  const aliasSet = new Set(identity.aliasKeys);
  aliasSet.add(identity.canonicalKey);
  const matchingIndexes = previous.flatMap((block, index) =>
    block.type === "task" && aliasSet.has(block.toolUseId) ? [index] : []
  );
  if (matchingIndexes.length === 0) {
    return [...previous, {
      type: "task",
      toolUseId: identity.canonicalKey,
      ...(identity.seq != null ? { seq: identity.seq } : {}),
      ...(identity.receivedAt != null ? { receivedAt: identity.receivedAt } : {}),
    }];
  }

  const firstIndex = matchingIndexes[0]!;
  const firstMarker = previous[firstIndex];
  const next = previous.filter((block, index) =>
    index === firstIndex || block.type !== "task" || !aliasSet.has(block.toolUseId)
  );
  const replacementIndex = next.findIndex((block) => block === firstMarker);
  if (replacementIndex >= 0 && firstMarker?.type === "task") {
    next[replacementIndex] = { ...firstMarker, toolUseId: identity.canonicalKey };
  }
  return next;
}

export function mergeDelegationTaskMetadata(
  task: StreamingTask,
  metadata: DelegationMetadata,
  completedAt = Date.now(),
): StreamingTask {
  const inferredFailure = metadata.status == null
    && metadata.textOutput?.trim().startsWith("ERROR:");
  const parsedStatus = metadata.status === "running"
    || metadata.status === "completed"
    || metadata.status === "failed"
    || metadata.status === "cancelled"
      ? metadata.status
      : inferredFailure
        ? "failed"
        : task.status;
  const taskIsTerminal = task.status === "completed"
    || task.status === "failed"
    || task.status === "cancelled";
  const status = taskIsTerminal && parsedStatus === "running" ? task.status : parsedStatus;
  const terminal = status === "completed" || status === "failed" || status === "cancelled";
  return {
    ...task,
    status,
    model: metadata.effectiveModelId ?? metadata.logicalModel ?? task.model,
    ...(metadata.agentName ? { subagentType: "delegated" } : {}),
    ...(metadata.providerHarness ? { providerHarness: metadata.providerHarness } : {}),
    ...(metadata.providerSessionId ? { providerSessionId: metadata.providerSessionId } : {}),
    ...(metadata.upstreamProvider ? { upstreamProvider: metadata.upstreamProvider } : {}),
    ...(metadata.providerProfile ? { providerProfile: metadata.providerProfile } : {}),
    ...(metadata.jobId ? { delegatedJobId: metadata.jobId } : {}),
    ...(metadata.delegatedSessionId ? { delegatedSessionId: metadata.delegatedSessionId } : {}),
    ...(metadata.delegatedConversationId
      ? { delegatedConversationId: metadata.delegatedConversationId }
      : {}),
    ...(metadata.delegatedAgentRunId
      ? { delegatedAgentRunId: metadata.delegatedAgentRunId }
      : {}),
    ...(metadata.logicalModel ? { logicalModel: metadata.logicalModel } : {}),
    ...(metadata.effectiveModelId ? { effectiveModelId: metadata.effectiveModelId } : {}),
    ...(metadata.logicalEffort ? { logicalEffort: metadata.logicalEffort } : {}),
    ...(metadata.effectiveEffort ? { effectiveEffort: metadata.effectiveEffort } : {}),
    ...(metadata.approvalPolicy ? { approvalPolicy: metadata.approvalPolicy } : {}),
    ...(metadata.sandboxMode ? { sandboxMode: metadata.sandboxMode } : {}),
    ...(metadata.inputTokens != null ? { inputTokens: metadata.inputTokens } : {}),
    ...(metadata.outputTokens != null ? { outputTokens: metadata.outputTokens } : {}),
    ...(metadata.cacheCreationTokens != null
      ? { cacheCreationTokens: metadata.cacheCreationTokens }
      : {}),
    ...(metadata.cacheReadTokens != null ? { cacheReadTokens: metadata.cacheReadTokens } : {}),
    ...(metadata.totalTokens != null ? { totalTokens: metadata.totalTokens } : {}),
    ...(metadata.estimatedUsd != null ? { estimatedUsd: metadata.estimatedUsd } : {}),
    ...(metadata.durationMs != null ? { totalDurationMs: metadata.durationMs } : {}),
    ...(metadata.startedAt != null ? { startedAt: metadata.startedAt } : {}),
    ...(metadata.completedAt != null ? { completedAt: metadata.completedAt } : {}),
    ...(metadata.clockSource != null ? { clockSource: metadata.clockSource } : {}),
    ...(metadata.textOutput ? { textOutput: metadata.textOutput } : {}),
    ...(terminal ? { completedAt: task.completedAt ?? completedAt } : {}),
  };
}

export function extractDelegationMetadata(
  args: unknown,
  result: unknown,
): DelegationMetadata {
  const argRecord = asRecord(args);
  const resultRecord = asRecord(parseMcpToolResultRaw(result));
  const delegatedStatus =
    getFirstRecord(resultRecord, "delegated_status", "delegatedStatus");
  const latestRun = getFirstRecord(delegatedStatus, "latest_run", "latestRun");
  const session = getFirstRecord(delegatedStatus, "session");
  const assignment = getFirstRecord(resultRecord, "assignment");
  const providerHarness =
    getFirstString(latestRun, "harness")
    ?? getFirstString(resultRecord, "harness")
    ?? getFirstString(session, "harness")
    ?? getFirstString(argRecord, "harness", "harness_override", "harnessOverride");

  const inputTokens = getFirstNumber(latestRun, "input_tokens", "inputTokens")
    ?? getFirstNumber(resultRecord, "input_tokens", "inputTokens");
  const outputTokens = getFirstNumber(latestRun, "output_tokens", "outputTokens")
    ?? getFirstNumber(resultRecord, "output_tokens", "outputTokens");
  const cacheCreationTokens = getFirstNumber(
    latestRun,
    "cache_creation_tokens",
    "cacheCreationTokens",
  ) ?? getFirstNumber(resultRecord, "cache_creation_tokens", "cacheCreationTokens");
  const cacheReadTokens = getFirstNumber(
    latestRun,
    "cache_read_tokens",
    "cacheReadTokens",
  ) ?? getFirstNumber(resultRecord, "cache_read_tokens", "cacheReadTokens");

  const authoritativeTotalTokens =
    getFirstNumber(latestRun, "processed_tokens", "processedTokens")
    ?? getFirstNumber(
      resultRecord,
      "total_tokens",
      "totalTokens",
      "processed_tokens",
      "processedTokens",
    );
  const hasDetailedTokens = inputTokens != null || outputTokens != null;
  const totalTokens = authoritativeTotalTokens
    ?? (hasDetailedTokens && providerHarness === "codex"
      ? (inputTokens ?? 0) + (outputTokens ?? 0)
      : hasDetailedTokens && providerHarness === "claude"
        ? (inputTokens ?? 0)
          + (outputTokens ?? 0)
          + (cacheCreationTokens ?? 0)
          + (cacheReadTokens ?? 0)
        : undefined);

  const rawError = getFirstString(resultRecord, "error");
  const textOutput =
    getFirstString(resultRecord, "content")
    ?? getContentText(resultRecord?.content)
    ?? formatNoOutputDelegationFailure(rawError)
    ?? rawError
    ?? getLastMessageText(delegatedStatus?.recent_messages ?? delegatedStatus?.recentMessages);

  const jobId =
    getFirstString(resultRecord, "job_id", "jobId")
    ?? getFirstString(argRecord, "job_id", "jobId");
  const status = normalizeStatus(
    getFirstString(resultRecord, "status")
    ?? getFirstString(latestRun, "status")
    ?? getFirstString(session, "status"),
  );
  const agentName =
    getFirstString(resultRecord, "agent_name", "agentName")
    ?? getFirstString(session, "agent_name", "agentName")
    ?? getFirstString(argRecord, "agent_name", "agentName");
  const prompt = getFirstString(argRecord, "prompt");
  const title = getFirstString(argRecord, "title");
  const providerSessionId =
    getFirstString(latestRun, "provider_session_id", "providerSessionId")
    ?? getFirstString(resultRecord, "provider_session_id", "providerSessionId")
    ?? getFirstString(session, "provider_session_id", "providerSessionId");
  const upstreamProvider =
    getFirstString(latestRun, "upstream_provider", "upstreamProvider")
    ?? getFirstString(resultRecord, "upstream_provider", "upstreamProvider");
  const providerProfile =
    getFirstString(latestRun, "provider_profile", "providerProfile")
    ?? getFirstString(resultRecord, "provider_profile", "providerProfile");
  const delegatedSessionId =
    getFirstString(resultRecord, "delegated_session_id", "delegatedSessionId")
    ?? getFirstString(argRecord, "delegated_session_id", "delegatedSessionId");
  const delegatedConversationId =
    getFirstString(resultRecord, "delegated_conversation_id", "delegatedConversationId")
    ?? getFirstString(delegatedStatus, "conversation_id", "conversationId");
  const delegatedAgentRunId =
    getFirstString(resultRecord, "delegated_agent_run_id", "delegatedAgentRunId")
    ?? getFirstString(latestRun, "agent_run_id", "agentRunId");
  const logicalModel =
    getFirstString(latestRun, "logical_model", "logicalModel")
    ?? getFirstString(resultRecord, "logical_model", "logicalModel")
    ?? getFirstString(argRecord, "model", "logical_model", "logicalModel");
  const effectiveModelId =
    getFirstString(latestRun, "effective_model_id", "effectiveModelId")
    ?? getFirstString(resultRecord, "effective_model_id", "effectiveModelId");
  const logicalEffort =
    getFirstString(latestRun, "logical_effort", "logicalEffort")
    ?? getFirstString(resultRecord, "logical_effort", "logicalEffort")
    ?? getFirstString(argRecord, "logical_effort", "logicalEffort");
  const effectiveEffort =
    getFirstString(latestRun, "effective_effort", "effectiveEffort")
    ?? getFirstString(resultRecord, "effective_effort", "effectiveEffort");
  const approvalPolicy =
    getFirstString(latestRun, "approval_policy", "approvalPolicy")
    ?? getFirstString(resultRecord, "approval_policy", "approvalPolicy")
    ?? getFirstString(argRecord, "approval_policy", "approvalPolicy");
  const sandboxMode =
    getFirstString(latestRun, "sandbox_mode", "sandboxMode")
    ?? getFirstString(resultRecord, "sandbox_mode", "sandboxMode")
    ?? getFirstString(argRecord, "sandbox_mode", "sandboxMode");
  const estimatedUsd = getFirstNumber(latestRun, "estimated_usd", "estimatedUsd")
    ?? getFirstNumber(resultRecord, "estimated_usd", "estimatedUsd");
  const durationMs = deriveDurationMs(
    getFirstString(latestRun, "started_at", "startedAt")
      ?? getFirstString(resultRecord, "started_at", "startedAt"),
    getFirstString(latestRun, "completed_at", "completedAt")
      ?? getFirstString(resultRecord, "completed_at", "completedAt"),
  );
  const startedAt = parseDelegationTimestamp(
    getFirstString(latestRun, "started_at", "startedAt")
      ?? getFirstString(resultRecord, "started_at", "startedAt"),
  );
  const completedAt = parseDelegationTimestamp(
    getFirstString(latestRun, "completed_at", "completedAt")
      ?? getFirstString(resultRecord, "completed_at", "completedAt"),
  );
  const assignmentTaskNumber = getFirstNumber(
    assignment,
    "task_number",
    "taskNumber",
  );
  const assignmentTitle = getFirstString(assignment, "title");
  const assignmentTaskState = getFirstString(
    assignment,
    "task_state",
    "taskState",
  );
  const assignmentState = getFirstString(
    assignment,
    "assignment_state",
    "assignmentState",
  );
  const assignmentDelegateAgentName = getFirstString(
    assignment,
    "delegate_agent_name",
    "delegateAgentName",
  );
  const assignmentMetadata = assignment
    ? {
        ...(assignmentTaskNumber != null
          ? { taskNumber: assignmentTaskNumber }
          : {}),
        ...(assignmentTitle ? { title: assignmentTitle } : {}),
        ...(assignmentTaskState ? { taskState: assignmentTaskState } : {}),
        ...(assignmentState ? { assignmentState } : {}),
        ...(assignmentDelegateAgentName
          ? { delegateAgentName: assignmentDelegateAgentName }
          : {}),
      }
    : undefined;

  return {
    ...(jobId ? { jobId } : {}),
    ...(status ? { status } : {}),
    ...(agentName ? { agentName } : {}),
    ...(prompt ? { prompt } : {}),
    ...(title ? { title } : {}),
    ...(assignmentMetadata ? { assignment: assignmentMetadata } : {}),
    ...(providerHarness ? { providerHarness } : {}),
    ...(providerSessionId ? { providerSessionId } : {}),
    ...(upstreamProvider ? { upstreamProvider } : {}),
    ...(providerProfile ? { providerProfile } : {}),
    ...(delegatedSessionId ? { delegatedSessionId } : {}),
    ...(delegatedConversationId ? { delegatedConversationId } : {}),
    ...(delegatedAgentRunId ? { delegatedAgentRunId } : {}),
    ...(logicalModel ? { logicalModel } : {}),
    ...(effectiveModelId ? { effectiveModelId } : {}),
    ...(logicalEffort ? { logicalEffort } : {}),
    ...(effectiveEffort ? { effectiveEffort } : {}),
    ...(approvalPolicy ? { approvalPolicy } : {}),
    ...(sandboxMode ? { sandboxMode } : {}),
    ...(inputTokens != null ? { inputTokens } : {}),
    ...(outputTokens != null ? { outputTokens } : {}),
    ...(cacheCreationTokens != null ? { cacheCreationTokens } : {}),
    ...(cacheReadTokens != null ? { cacheReadTokens } : {}),
    ...(totalTokens != null ? { totalTokens } : {}),
    ...(estimatedUsd != null ? { estimatedUsd } : {}),
    ...(durationMs != null ? { durationMs } : {}),
    ...(startedAt != null ? { startedAt } : {}),
    ...(completedAt != null ? { completedAt } : {}),
    ...(startedAt != null ? { clockSource: latestRun ? "delegated-run" as const : "delegation-job" as const } : {}),
    ...(textOutput ? { textOutput } : {}),
  };
}

function mergeDelegationEntries<T extends DelegationMergeable>(entries: T[]): T[] {
  const merged: T[] = [];
  const startIndexByJobId = new Map<string, number>();
  const unresolvedStartIndexes = new Set<number>();

  for (const entry of entries) {
    if (!entry.name || !isDelegationToolCall(entry.name)) {
      merged.push(entry);
      continue;
    }

    const metadata = extractDelegationMetadata(entry.arguments, entry.result);

    if (isDelegationStartToolCall(entry.name)) {
      if (metadata.jobId) {
        const exactStartIndex = startIndexByJobId.get(metadata.jobId);
        const provisionalStartIndex = exactStartIndex == null && unresolvedStartIndexes.size === 1
          ? [...unresolvedStartIndexes][0]
          : undefined;
        const startIndex = exactStartIndex ?? provisionalStartIndex;
        if (startIndex != null) {
          const startEntry = merged[startIndex];
          if (startEntry) {
            merged[startIndex] = {
              ...entry,
              ...startEntry,
              result: entry.result ?? startEntry.result,
              ...(entry.error || startEntry.error
                ? { error: entry.error ?? startEntry.error }
                : {}),
            };
            unresolvedStartIndexes.delete(startIndex);
            startIndexByJobId.set(metadata.jobId, startIndex);
            continue;
          }
        }
      }
      merged.push(entry);
      const newIndex = merged.length - 1;
      if (metadata.jobId) startIndexByJobId.set(metadata.jobId, newIndex);
      else unresolvedStartIndexes.add(newIndex);
      continue;
    }

    if (metadata.jobId) {
      const startIndex = startIndexByJobId.get(metadata.jobId);
      if (startIndex != null) {
        const startEntry = merged[startIndex];
        if (startEntry) {
          merged[startIndex] = {
            ...startEntry,
            result: entry.result ?? startEntry.result,
            ...(entry.error || startEntry.error
              ? { error: entry.error ?? startEntry.error }
              : {}),
          };
          continue;
        }
      }
    }

    const syntheticStartEntry = {
      ...entry,
      name: toDelegationStartName(entry.name),
    } as T;
    merged.push(syntheticStartEntry);
    if (metadata.jobId) {
      startIndexByJobId.set(metadata.jobId, merged.length - 1);
    }
  }

  return merged;
}

export function mergeDelegationToolCalls<T extends ToolCall>(toolCalls: T[]): T[] {
  return mergeDelegationEntries(toolCalls);
}

export function mergeDelegationContentBlocks<
  T extends { type: string; name?: string; arguments?: unknown; result?: unknown; error?: string },
>(blocks: T[]): T[] {
  return mergeDelegationEntries(blocks);
}

export function normalizeDelegationTranscriptPayload<
  TContentBlock extends { type: string; name?: string; arguments?: unknown; result?: unknown; error?: string },
  TToolCall extends ToolCall,
>({
  contentBlocks,
  toolCalls,
}: NormalizeDelegationTranscriptPayloadArgs<TContentBlock, TToolCall>): NormalizedDelegationTranscriptPayload<TContentBlock, TToolCall> {
  return {
    contentBlocks: mergeDelegationContentBlocks(contentBlocks ?? []),
    toolCalls: mergeDelegationToolCalls(toolCalls ?? []),
  };
}
