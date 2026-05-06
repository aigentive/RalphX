// Tauri invoke wrappers for unified chat API with type safety using Zod schemas

import { invoke } from "@tauri-apps/api/core";
import { z } from "zod";
import type {
  AgentConversationMode,
  ChatConversation,
  AgentRun,
  ContextType,
} from "../types/chat-conversation";
import { normalizeConversationProviderMetadata } from "../types/chat-conversation";
import type { ToolCall } from "../components/Chat/ToolCallIndicator";
import type { ContentBlockItem } from "../components/Chat/MessageItem";
import { isWebMode } from "@/lib/tauri-detection";
import { backendApiUrl } from "@/api/backend";

// ============================================================================
// Typed Invoke Helper
// ============================================================================

async function typedInvoke<T>(
  cmd: string,
  args: Record<string, unknown>,
  schema: z.ZodType<T>
): Promise<T> {
  const result = await invoke(cmd, args);
  return schema.parse(result);
}

// ============================================================================
// Response Types
// ============================================================================

/**
 * Chat message response from backend - with pre-parsed toolCalls and contentBlocks
 */
export interface ChatMessageResponse {
  id: string;
  sessionId: string | null;
  projectId: string | null;
  taskId: string | null;
  role: string;
  content: string;
  metadata: string | null;
  parentMessageId: string | null;
  conversationId: string | null;
  /** Pre-parsed tool calls array (parsed from JSON at API layer) */
  toolCalls: ToolCall[] | null;
  /** Pre-parsed content blocks array (parsed from JSON at API layer) */
  contentBlocks: ContentBlockItem[] | null;
  /** Sender name for team mode messages (teammate name or "lead") */
  sender: string | null;
  attributionSource?: string | null;
  providerHarness?: string | null;
  providerSessionId?: string | null;
  upstreamProvider?: string | null;
  providerProfile?: string | null;
  logicalModel?: string | null;
  effectiveModelId?: string | null;
  logicalEffort?: string | null;
  effectiveEffort?: string | null;
  inputTokens?: number | null;
  outputTokens?: number | null;
  cacheCreationTokens?: number | null;
  cacheReadTokens?: number | null;
  estimatedUsd?: number | null;
  createdAt: string;
}

// ============================================================================
// Parsing Utilities
// ============================================================================

/**
 * Parse content blocks from raw JSON data
 * @param raw The raw data from backend (could be string, array, or null)
 * @returns Parsed content blocks array
 */
export function parseContentBlocks(raw: unknown): ContentBlockItem[] {
  if (!raw) return [];

  // If it's already an array, use it directly
  const data = typeof raw === "string" ? safeJsonParse(raw) : raw;
  if (!Array.isArray(data)) return [];

  return data.map((block) => {
    const item: ContentBlockItem = {
      type: block.type,
      text: block.text,
      id: block.id,
      name: block.name,
      arguments: block.arguments,
      result: block.result,
      parentToolUseId: block.parent_tool_use_id ?? block.parentToolUseId,
    };
    // Transform diff_context (snake_case) to diffContext (camelCase) for tool_use blocks
    if (block.type === "tool_use" && block.diff_context) {
      item.diffContext = {
        oldContent: block.diff_context.old_content ?? undefined,
        filePath: block.diff_context.file_path,
      };
    }
    return item;
  });
}

/**
 * Parse tool calls from raw JSON data
 * @param raw The raw data from backend (could be string, array, or null)
 * @returns Parsed tool calls array
 */
export function parseToolCalls(raw: unknown): ToolCall[] {
  if (!raw) return [];

  // If it's already an array, use it directly
  const data = typeof raw === "string" ? safeJsonParse(raw) : raw;
  if (!Array.isArray(data)) return [];

  return data.map((tc, idx) => {
    const toolCall: ToolCall = {
      id: tc.id ?? `tool-${idx}`,
      name: tc.name ?? "unknown",
      arguments: tc.arguments ?? {},
      result: tc.result,
      parentToolUseId: tc.parent_tool_use_id ?? tc.parentToolUseId,
      error: tc.error,
    };
    if (tc.diff_context) {
      toolCall.diffContext = {
        oldContent: tc.diff_context.old_content ?? undefined,
        filePath: tc.diff_context.file_path,
      };
    }
    return toolCall;
  });
}

/**
 * Safely parse JSON, returning null on failure
 */
function safeJsonParse(str: string): unknown {
  try {
    return JSON.parse(str);
  } catch {
    return null;
  }
}

/**
 * Queued message response from backend
 */
export interface QueuedMessageResponse {
  id: string;
  content: string;
  createdAt: string;
  isEditing: boolean;
}

export interface ConversationListPageResponse {
  conversations: ChatConversation[];
  limit: number;
  offset: number;
  total: number;
  hasMore: boolean;
}

/**
 * A streaming task in the active state HTTP response from GET /api/conversations/:id/active-state.
 * Mirrors the Rust ActiveStreamingTask struct (snake_case — no rename_all on the Rust struct).
 */
export interface ActiveStreamingTaskResponse {
  tool_use_id: string;
  description?: string;
  subagent_type?: string;
  model?: string;
  status: string;
  agent_id?: string;
  teammate_name?: string;
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
  /** Total tokens used (from TaskCompleted stats) */
  total_tokens?: number;
  /** Total tool uses count (from TaskCompleted stats) */
  total_tool_uses?: number;
  /** Duration in milliseconds (from TaskCompleted stats) */
  duration_ms?: number;
  input_tokens?: number;
  output_tokens?: number;
  cache_creation_tokens?: number;
  cache_read_tokens?: number;
  estimated_usd?: number;
  text_output?: string;
}

/**
 * Response from GET /api/conversations/:id/active-state HTTP endpoint.
 * Used to hydrate streaming UI when navigating to an active agent execution.
 */
export interface ConversationActiveStateResponse {
  is_active: boolean;
  tool_calls: unknown[];
  streaming_tasks: ActiveStreamingTaskResponse[];
  partial_text: string;
}

/**
 * Fetch the active streaming state for a conversation.
 * Called when navigating to a conversation with an active agent execution
 * to hydrate the streaming UI with missed events.
 *
 * @param conversationId - The conversation ID
 * @returns The active state response
 */
export async function getConversationActiveState(
  conversationId: string
): Promise<ConversationActiveStateResponse> {
  const res = await fetch(
    backendApiUrl(`conversations/${conversationId}/active-state`)
  );
  if (!res.ok) {
    throw new Error(`Failed to get conversation active state: ${res.status}`);
  }
  return res.json() as Promise<ConversationActiveStateResponse>;
}

// ============================================================================
// Child Session Status
// ============================================================================

export interface ChildSessionMessage {
  role: string;
  content: string;
  created_at: string | null;
}

export interface ChildSessionAgentState {
  estimated_status: "idle" | "likely_generating" | "likely_waiting";
}

export interface ChildSessionVerificationInfo {
  status: string;
  generation: number;
  current_round: number | null;
  gap_score: number | null;
}

export interface ChildSessionStatusResponse {
  session_id: string;
  title: string | null;
  session_status?: string | null;
  session_purpose?: string | null;
  parent_session_id?: string | null;
  agent_state: ChildSessionAgentState;
  recent_messages: ChildSessionMessage[];
  verification?: ChildSessionVerificationInfo | null;
  pending_initial_prompt?: string | null;
  lastEffectiveModel: string | null;
}

/**
 * Fetch the status and recent messages for a child ideation session.
 *
 * @param sessionId - The child session ID
 * @returns Child session status response
 */
export async function getChildSessionStatus(
  sessionId: string
): Promise<ChildSessionStatusResponse> {
  if (isWebMode()) {
    const mockedResponse = await window.__mockChatApi?.getChildSessionStatus(sessionId);
    if (mockedResponse) {
      return mockedResponse;
    }
  }

  const res = await fetch(
    backendApiUrl(
      `ideation/sessions/${sessionId}/child-status?include_messages=true&message_limit=5`
    )
  );
  if (!res.ok) {
    throw new Error(`Failed to get child session status: ${res.status}`);
  }
  const raw = (await res.json()) as {
    session_id?: string;
    title?: string | null;
    session?: {
      id?: string;
      title?: string | null;
      status?: string | null;
      session_purpose?: string | null;
      parent_session_id?: string | null;
      last_effective_model?: string | null;
    };
    agent_state: ChildSessionAgentState;
    recent_messages?: ChildSessionMessage[] | null;
    verification?: ChildSessionVerificationInfo | null;
    pending_initial_prompt?: string | null;
    last_effective_model?: string | null;
  };
  return {
    session_id: raw.session_id ?? raw.session?.id ?? sessionId,
    title: raw.title ?? raw.session?.title ?? null,
    session_status: raw.session?.status ?? null,
    session_purpose: raw.session?.session_purpose ?? null,
    parent_session_id: raw.session?.parent_session_id ?? null,
    agent_state: raw.agent_state,
    recent_messages: raw.recent_messages ?? [],
    verification: raw.verification ?? null,
    ...(raw.pending_initial_prompt !== undefined && {
      pending_initial_prompt: raw.pending_initial_prompt,
    }),
    lastEffectiveModel: raw.last_effective_model ?? raw.session?.last_effective_model ?? null,
  };
}

// ============================================================================
// Response Schemas (snake_case from Rust backend)
// ============================================================================

// Response schemas for backend (snake_case - Rust default serialization)
const ChatConversationResponseSchema = z.object({
  id: z.string(),
  context_type: z.string(),
  context_id: z.string(),
  claude_session_id: z.string().nullable(),
  provider_session_id: z.string().nullable().optional(),
  provider_harness: z.string().min(1).nullable().optional(),
  upstream_provider: z.string().nullable().optional(),
  provider_profile: z.string().nullable().optional(),
  agent_mode: z.enum(["chat", "edit", "ideation"]).nullable().optional(),
  title: z.string().nullable(),
  message_count: z.number(),
  last_message_at: z.string().nullable(),
  created_at: z.string(),
  updated_at: z.string(),
  archived_at: z.string().nullable().optional(),
});

const ConversationListPageResponseSchema = z.object({
  conversations: z.array(ChatConversationResponseSchema),
  limit: z.number(),
  offset: z.number(),
  total: z.number(),
  has_more: z.boolean(),
});

const AgentRunResponseSchema = z.object({
  id: z.string(),
  conversation_id: z.string(),
  status: z.string(),
  started_at: z.string(),
  completed_at: z.string().nullable(),
  error_message: z.string().nullable(),
  model_id: z.string().nullable().optional(),
  model_label: z.string().nullable().optional(),
});

type RawConversation = z.infer<typeof ChatConversationResponseSchema>;
type RawConversationListPage = z.infer<typeof ConversationListPageResponseSchema>;
type RawAgentRun = z.infer<typeof AgentRunResponseSchema>;

function transformConversation(raw: RawConversation): ChatConversation {
  const providerMetadata = normalizeConversationProviderMetadata({
    claudeSessionId: raw.claude_session_id,
    providerSessionId: raw.provider_session_id ?? null,
    providerHarness: raw.provider_harness ?? null,
  });

  return {
    id: raw.id,
    contextType: raw.context_type as ContextType,
    contextId: raw.context_id,
    ...providerMetadata,
    upstreamProvider: raw.upstream_provider ?? null,
    providerProfile: raw.provider_profile ?? null,
    agentMode: raw.agent_mode ?? null,
    title: raw.title,
    messageCount: raw.message_count,
    lastMessageAt: raw.last_message_at,
    createdAt: raw.created_at,
    updatedAt: raw.updated_at,
    archivedAt: raw.archived_at ?? null,
  };
}

function transformConversationListPage(
  raw: RawConversationListPage
): ConversationListPageResponse {
  return {
    conversations: raw.conversations.map(transformConversation),
    limit: raw.limit,
    offset: raw.offset,
    total: raw.total,
    hasMore: raw.has_more,
  };
}

export interface UsageTotalsResponse {
  inputTokens: number;
  outputTokens: number;
  cacheCreationTokens: number;
  cacheReadTokens: number;
  estimatedUsd: number | null;
}

export interface UsageBucketResponse {
  key: string;
  count: number;
  usage: UsageTotalsResponse;
}

export interface ConversationUsageCoverageResponse {
  providerMessageCount: number;
  providerMessagesWithUsage: number;
  runCount: number;
  runsWithUsage: number;
  effectiveTotalsSource: string;
}

export interface ConversationAttributionCoverageResponse {
  providerMessageCount: number;
  providerMessagesWithAttribution: number;
  runCount: number;
  runsWithAttribution: number;
}

export interface ConversationStatsResponse {
  conversationId: string;
  contextType: ContextType;
  contextId: string;
  providerHarness: string | null;
  upstreamProvider: string | null;
  providerProfile: string | null;
  messageUsageTotals: UsageTotalsResponse;
  runUsageTotals: UsageTotalsResponse;
  effectiveUsageTotals: UsageTotalsResponse;
  usageCoverage: ConversationUsageCoverageResponse;
  attributionCoverage: ConversationAttributionCoverageResponse;
  byHarness: UsageBucketResponse[];
  byUpstreamProvider: UsageBucketResponse[];
  byModel: UsageBucketResponse[];
  byEffort: UsageBucketResponse[];
}

export interface ConversationMessagesPageResponse {
  conversation: ChatConversation;
  messages: ChatMessageResponse[];
  limit: number;
  offset: number;
  totalMessageCount: number;
  hasOlder: boolean;
}

const SnakeUsageTotalsResponseSchema = z.object({
  input_tokens: z.number(),
  output_tokens: z.number(),
  cache_creation_tokens: z.number(),
  cache_read_tokens: z.number(),
  estimated_usd: z.number().nullable(),
});

const CamelUsageTotalsResponseSchema = z.object({
  inputTokens: z.number(),
  outputTokens: z.number(),
  cacheCreationTokens: z.number(),
  cacheReadTokens: z.number(),
  estimatedUsd: z.number().nullable(),
});

const UsageTotalsResponseSchema = z.union([
  SnakeUsageTotalsResponseSchema,
  CamelUsageTotalsResponseSchema,
]);

const UsageBucketResponseSchema = z.object({
  key: z.string(),
  count: z.number(),
  usage: UsageTotalsResponseSchema,
});

const SnakeConversationUsageCoverageResponseSchema = z.object({
  provider_message_count: z.number(),
  provider_messages_with_usage: z.number(),
  run_count: z.number(),
  runs_with_usage: z.number(),
  effective_totals_source: z.string(),
});

const CamelConversationUsageCoverageResponseSchema = z.object({
  providerMessageCount: z.number(),
  providerMessagesWithUsage: z.number(),
  runCount: z.number(),
  runsWithUsage: z.number(),
  effectiveTotalsSource: z.string(),
});

const ConversationUsageCoverageResponseSchema = z.union([
  SnakeConversationUsageCoverageResponseSchema,
  CamelConversationUsageCoverageResponseSchema,
]);

const SnakeConversationAttributionCoverageResponseSchema = z.object({
  provider_message_count: z.number(),
  provider_messages_with_attribution: z.number(),
  run_count: z.number(),
  runs_with_attribution: z.number(),
});

const CamelConversationAttributionCoverageResponseSchema = z.object({
  providerMessageCount: z.number(),
  providerMessagesWithAttribution: z.number(),
  runCount: z.number(),
  runsWithAttribution: z.number(),
});

const ConversationAttributionCoverageResponseSchema = z.union([
  SnakeConversationAttributionCoverageResponseSchema,
  CamelConversationAttributionCoverageResponseSchema,
]);

const SnakeConversationStatsResponseSchema = z.object({
  conversation_id: z.string(),
  context_type: z.string(),
  context_id: z.string(),
  provider_harness: z.string().nullable(),
  upstream_provider: z.string().nullable(),
  provider_profile: z.string().nullable(),
  message_usage_totals: UsageTotalsResponseSchema,
  run_usage_totals: UsageTotalsResponseSchema,
  effective_usage_totals: UsageTotalsResponseSchema,
  usage_coverage: ConversationUsageCoverageResponseSchema,
  attribution_coverage: ConversationAttributionCoverageResponseSchema,
  by_harness: z.array(UsageBucketResponseSchema),
  by_upstream_provider: z.array(UsageBucketResponseSchema),
  by_model: z.array(UsageBucketResponseSchema),
  by_effort: z.array(UsageBucketResponseSchema),
});

const CamelConversationStatsResponseSchema = z.object({
  conversationId: z.string(),
  contextType: z.string(),
  contextId: z.string(),
  providerHarness: z.string().nullable(),
  upstreamProvider: z.string().nullable(),
  providerProfile: z.string().nullable(),
  messageUsageTotals: UsageTotalsResponseSchema,
  runUsageTotals: UsageTotalsResponseSchema,
  effectiveUsageTotals: UsageTotalsResponseSchema,
  usageCoverage: ConversationUsageCoverageResponseSchema,
  attributionCoverage: ConversationAttributionCoverageResponseSchema,
  byHarness: z.array(UsageBucketResponseSchema),
  byUpstreamProvider: z.array(UsageBucketResponseSchema),
  byModel: z.array(UsageBucketResponseSchema),
  byEffort: z.array(UsageBucketResponseSchema),
});

const ConversationStatsResponseSchema = z.union([
  SnakeConversationStatsResponseSchema,
  CamelConversationStatsResponseSchema,
]);

type RawConversationStats = z.infer<typeof ConversationStatsResponseSchema>;

function transformUsageTotals(raw: z.infer<typeof UsageTotalsResponseSchema>): UsageTotalsResponse {
  if ("inputTokens" in raw) {
    return {
      inputTokens: raw.inputTokens,
      outputTokens: raw.outputTokens,
      cacheCreationTokens: raw.cacheCreationTokens,
      cacheReadTokens: raw.cacheReadTokens,
      estimatedUsd: raw.estimatedUsd,
    };
  }

  return {
    inputTokens: raw.input_tokens,
    outputTokens: raw.output_tokens,
    cacheCreationTokens: raw.cache_creation_tokens,
    cacheReadTokens: raw.cache_read_tokens,
    estimatedUsd: raw.estimated_usd,
  };
}

function transformUsageBucket(raw: z.infer<typeof UsageBucketResponseSchema>): UsageBucketResponse {
  return {
    key: raw.key,
    count: raw.count,
    usage: transformUsageTotals(raw.usage),
  };
}

function transformUsageCoverage(
  raw: z.infer<typeof ConversationUsageCoverageResponseSchema>,
): ConversationUsageCoverageResponse {
  if ("providerMessageCount" in raw) {
    return {
      providerMessageCount: raw.providerMessageCount,
      providerMessagesWithUsage: raw.providerMessagesWithUsage,
      runCount: raw.runCount,
      runsWithUsage: raw.runsWithUsage,
      effectiveTotalsSource: raw.effectiveTotalsSource,
    };
  }

  return {
    providerMessageCount: raw.provider_message_count,
    providerMessagesWithUsage: raw.provider_messages_with_usage,
    runCount: raw.run_count,
    runsWithUsage: raw.runs_with_usage,
    effectiveTotalsSource: raw.effective_totals_source,
  };
}

function transformAttributionCoverage(
  raw: z.infer<typeof ConversationAttributionCoverageResponseSchema>,
): ConversationAttributionCoverageResponse {
  if ("providerMessageCount" in raw) {
    return {
      providerMessageCount: raw.providerMessageCount,
      providerMessagesWithAttribution: raw.providerMessagesWithAttribution,
      runCount: raw.runCount,
      runsWithAttribution: raw.runsWithAttribution,
    };
  }

  return {
    providerMessageCount: raw.provider_message_count,
    providerMessagesWithAttribution:
      raw.provider_messages_with_attribution,
    runCount: raw.run_count,
    runsWithAttribution: raw.runs_with_attribution,
  };
}

function transformConversationStats(raw: RawConversationStats): ConversationStatsResponse {
  if ("conversationId" in raw) {
    return {
      conversationId: raw.conversationId,
      contextType: raw.contextType as ContextType,
      contextId: raw.contextId,
      providerHarness: raw.providerHarness,
      upstreamProvider: raw.upstreamProvider,
      providerProfile: raw.providerProfile,
      messageUsageTotals: transformUsageTotals(raw.messageUsageTotals),
      runUsageTotals: transformUsageTotals(raw.runUsageTotals),
      effectiveUsageTotals: transformUsageTotals(raw.effectiveUsageTotals),
      usageCoverage: transformUsageCoverage(raw.usageCoverage),
      attributionCoverage: transformAttributionCoverage(raw.attributionCoverage),
      byHarness: raw.byHarness.map(transformUsageBucket),
      byUpstreamProvider: raw.byUpstreamProvider.map(transformUsageBucket),
      byModel: raw.byModel.map(transformUsageBucket),
      byEffort: raw.byEffort.map(transformUsageBucket),
    };
  }

  return {
    conversationId: raw.conversation_id,
    contextType: raw.context_type as ContextType,
    contextId: raw.context_id,
    providerHarness: raw.provider_harness,
    upstreamProvider: raw.upstream_provider,
    providerProfile: raw.provider_profile,
    messageUsageTotals: transformUsageTotals(raw.message_usage_totals),
    runUsageTotals: transformUsageTotals(raw.run_usage_totals),
    effectiveUsageTotals: transformUsageTotals(raw.effective_usage_totals),
    usageCoverage: transformUsageCoverage(raw.usage_coverage),
    attributionCoverage: transformAttributionCoverage(raw.attribution_coverage),
    byHarness: raw.by_harness.map(transformUsageBucket),
    byUpstreamProvider: raw.by_upstream_provider.map(transformUsageBucket),
    byModel: raw.by_model.map(transformUsageBucket),
    byEffort: raw.by_effort.map(transformUsageBucket),
  };
}

function transformAgentRun(raw: RawAgentRun): AgentRun {
  return {
    id: raw.id,
    conversationId: raw.conversation_id,
    status: raw.status as AgentRun["status"],
    startedAt: raw.started_at,
    completedAt: raw.completed_at,
    errorMessage: raw.error_message,
    modelId: raw.model_id ?? null,
    modelLabel: raw.model_label ?? null,
  };
}

// Schema for AgentMessageResponse from unified_chat_commands (snake_case)
const AgentMessageSchema = z.object({
  id: z.string(),
  conversation_id: z.string().nullable().optional(),
  role: z.string(),
  content: z.string(),
  metadata: z.string().nullable().optional(),
  tool_calls: z.any().nullable(),
  content_blocks: z.any().nullable(),
  sender: z.string().nullable().optional(),
  attribution_source: z.string().nullable().optional(),
  provider_harness: z.string().nullable().optional(),
  provider_session_id: z.string().nullable().optional(),
  upstream_provider: z.string().nullable().optional(),
  provider_profile: z.string().nullable().optional(),
  logical_model: z.string().nullable().optional(),
  effective_model_id: z.string().nullable().optional(),
  logical_effort: z.string().nullable().optional(),
  effective_effort: z.string().nullable().optional(),
  input_tokens: z.number().nullable().optional(),
  output_tokens: z.number().nullable().optional(),
  cache_creation_tokens: z.number().nullable().optional(),
  cache_read_tokens: z.number().nullable().optional(),
  estimated_usd: z.number().nullable().optional(),
  created_at: z.string(),
});

type RawAgentMessage = z.infer<typeof AgentMessageSchema>;

const ConversationMessagesPageResponseSchema = z.object({
  conversation: ChatConversationResponseSchema,
  messages: z.array(AgentMessageSchema),
  limit: z.number().int().nonnegative(),
  offset: z.number().int().nonnegative(),
  total_message_count: z.number().int().nonnegative(),
  has_older: z.boolean(),
});

type RawConversationMessagesPage = z.infer<
  typeof ConversationMessagesPageResponseSchema
>;

function transformAgentMessage(
  raw: RawAgentMessage,
  fallbackConversationId?: string
): ChatMessageResponse {
  return {
    id: raw.id,
    sessionId: null,
    projectId: null,
    taskId: null,
    role: raw.role,
    sender: raw.sender ?? null,
    attributionSource: raw.attribution_source ?? null,
    providerHarness: raw.provider_harness ?? null,
    providerSessionId: raw.provider_session_id ?? null,
    upstreamProvider: raw.upstream_provider ?? null,
    providerProfile: raw.provider_profile ?? null,
    logicalModel: raw.logical_model ?? null,
    effectiveModelId: raw.effective_model_id ?? null,
    logicalEffort: raw.logical_effort ?? null,
    effectiveEffort: raw.effective_effort ?? null,
    inputTokens: raw.input_tokens ?? null,
    outputTokens: raw.output_tokens ?? null,
    cacheCreationTokens: raw.cache_creation_tokens ?? null,
    cacheReadTokens: raw.cache_read_tokens ?? null,
    estimatedUsd: raw.estimated_usd ?? null,
    content: raw.content,
    metadata: raw.metadata ?? null,
    parentMessageId: null,
    conversationId: raw.conversation_id ?? fallbackConversationId ?? null,
    // Parse at API layer to avoid redundant parsing in components
    toolCalls: parseToolCalls(raw.tool_calls),
    contentBlocks: parseContentBlocks(raw.content_blocks),
    createdAt: raw.created_at,
  };
}

function transformConversationMessagesPage(
  raw: RawConversationMessagesPage
): ConversationMessagesPageResponse {
  const conversationId = raw.conversation.id;
  return {
    conversation: transformConversation(raw.conversation),
    messages: raw.messages.map((message) => transformAgentMessage(message, conversationId)),
    limit: raw.limit,
    offset: raw.offset,
    totalMessageCount: raw.total_message_count,
    hasOlder: raw.has_older,
  };
}

/**
 * List all conversations for a given context
 * @param contextType The context type
 * @param contextId The context ID
 * @returns Array of conversations
 */
export async function listConversations(
  contextType: ContextType,
  contextId: string,
  includeArchived = false
): Promise<ChatConversation[]> {
  const raw = await typedInvoke(
    "list_agent_conversations",
    { contextType, contextId, includeArchived },
    z.array(ChatConversationResponseSchema)
  );
  return raw.map(transformConversation);
}

/**
 * List a page of conversations for a given context with optional title search.
 */
export async function listConversationsPage(
  contextType: ContextType,
  contextId: string,
  limit: number,
  offset = 0,
  includeArchived = false,
  search?: string,
  archivedOnly = false
): Promise<ConversationListPageResponse> {
  const normalizedSearch = search?.trim();
  const raw = await typedInvoke(
    "list_agent_conversations_page",
    {
      contextType,
      contextId,
      includeArchived,
      ...(archivedOnly ? { archivedOnly } : {}),
      limit,
      offset,
      ...(normalizedSearch ? { search: normalizedSearch } : {}),
    },
    ConversationListPageResponseSchema
  );
  return transformConversationListPage(raw);
}

/**
 * Get a conversation with its messages
 * @param conversationId The conversation ID
 * @returns The conversation with messages
 */
export async function getConversation(
  conversationId: string
): Promise<{ conversation: ChatConversation; messages: ChatMessageResponse[] }> {
  const raw = await typedInvoke(
    "get_agent_conversation",
    { conversationId },
    z.object({
      conversation: ChatConversationResponseSchema,
      messages: z.array(AgentMessageSchema),
    })
  );

  return {
    conversation: transformConversation(raw.conversation),
    messages: raw.messages.map((message) => transformAgentMessage(message, raw.conversation.id)),
  };
}

/**
 * Get a tail-first page of conversation messages.
 * `offset` counts how many newest messages to skip before loading older history.
 */
export async function getConversationMessagesPage(
  conversationId: string,
  limit: number,
  offset = 0
): Promise<ConversationMessagesPageResponse> {
  const raw = await typedInvoke(
    "get_agent_conversation_messages_page",
    { conversationId, limit, offset },
    ConversationMessagesPageResponseSchema
  );

  return transformConversationMessagesPage(raw);
}

export async function getConversationStats(
  conversationId: string
): Promise<ConversationStatsResponse | null> {
  const raw = await typedInvoke(
    "get_agent_conversation_stats",
    { conversationId },
    ConversationStatsResponseSchema.nullable()
  );
  return raw ? transformConversationStats(raw) : null;
}

/**
 * Create a new conversation
 * @param contextType The context type
 * @param contextId The context ID
 * @returns The created conversation
 */
export async function createConversation(
  contextType: ContextType,
  contextId: string,
  title?: string
): Promise<ChatConversation> {
  const raw = await typedInvoke(
    "create_agent_conversation",
    {
      input: {
        contextType,
        contextId,
        ...(title !== undefined && title.trim().length > 0 && { title: title.trim() }),
      },
    },
    ChatConversationResponseSchema
  );
  return transformConversation(raw);
}

export async function updateConversationTitle(
  conversationId: string,
  title: string
): Promise<ChatConversation> {
  const raw = await typedInvoke(
    "update_agent_conversation_title",
    {
      conversationId,
      title: title.trim(),
    },
    ChatConversationResponseSchema
  );
  return transformConversation(raw);
}

export async function spawnConversationSessionNamer(
  conversationId: string,
  firstMessage: string
): Promise<void> {
  await invoke("spawn_session_namer", {
    conversationId,
    firstMessage,
  });
}

export async function archiveConversation(
  conversationId: string
): Promise<ChatConversation> {
  const raw = await typedInvoke(
    "archive_agent_conversation",
    { conversationId },
    ChatConversationResponseSchema
  );
  return transformConversation(raw);
}

export async function restoreConversation(
  conversationId: string
): Promise<ChatConversation> {
  const raw = await typedInvoke(
    "restore_agent_conversation",
    { conversationId },
    ChatConversationResponseSchema
  );
  return transformConversation(raw);
}

/**
 * Get the current agent run status for a conversation
 * @param conversationId The conversation ID
 * @returns The agent run if one is active, null otherwise
 */
export async function getAgentRunStatus(
  conversationId: string
): Promise<AgentRun | null> {
  const raw = await typedInvoke(
    "get_agent_run_status_unified",
    { conversationId },
    AgentRunResponseSchema.nullable()
  );
  return raw ? transformAgentRun(raw) : null;
}

// ============================================================================
// Namespace Export for Alternative Usage Pattern
// ============================================================================

const QueuedMessageResponseSchema = z.object({
  id: z.string(),
  content: z.string(),
  created_at: z.string(),
  is_editing: z.boolean(),
});

type RawQueuedMessage = z.infer<typeof QueuedMessageResponseSchema>;

function transformQueuedMessage(raw: RawQueuedMessage): QueuedMessageResponse {
  return {
    id: raw.id,
    content: raw.content,
    createdAt: raw.created_at,
    isEditing: raw.is_editing,
  };
}

// ============================================================================
// Namespace Export for Alternative Usage Pattern
// ============================================================================

/**
 * Chat API as a namespace object (alternative to individual imports)
 */
export const chatApi = {
  // Conversation management
  listConversations,
  listConversationsPage,
  getConversation,
  getConversationMessagesPage,
  getConversationStats,
  createConversation,
  updateConversationTitle,
  spawnConversationSessionNamer,
  archiveConversation,
  restoreConversation,
  getAgentConversationWorkspace,
  listAgentConversationWorkspacesByProject,
  listAgentConversationWorkspacePublicationEvents,
  getAgentConversationWorkspaceFreshness,
  updateAgentConversationWorkspaceFromBase,
  publishAgentConversationWorkspace,
  closeAgentWorkspacePr,
  getAgentRunStatus,
  // Message sending & queue
  startAgentConversation,
  switchAgentConversationMode,
  sendAgentMessage,
  getQueuedAgentMessages,
  deleteQueuedAgentMessage,
  // Agent lifecycle
  isChatServiceAvailable,
  stopAgent,
  isAgentRunning,
  // Attachments
  listMessageAttachments,
  // Active state
  getConversationActiveState,
  // Child session
  getChildSessionStatus,
} as const;

// ============================================================================
// Unified Agent API Functions (Phase 5-6 Consolidation)
// ============================================================================

/**
 * Response from unified send_agent_message command
 */
export interface SendAgentMessageResult {
  conversationId: string;
  agentRunId: string;
  isNewConversation: boolean;
  wasQueued: boolean;
  queuedAsPending: boolean;
  queuedMessageId?: string | null | undefined;
}

export type AgentConversationWorkspaceMode = AgentConversationMode;
export type AgentConversationBaseRefKind =
  | "project_default"
  | "current_branch"
  | "local_branch";

export interface AgentConversationBaseSelection {
  kind: AgentConversationBaseRefKind;
  ref: string;
  displayName: string;
}

export interface AgentConversationWorkspace {
  conversationId: string;
  projectId: string;
  mode: AgentConversationWorkspaceMode;
  baseRefKind: string;
  baseRef: string;
  baseDisplayName: string | null;
  baseCommit: string | null;
  branchName: string;
  worktreePath: string;
  linkedIdeationSessionId: string | null;
  linkedPlanBranchId: string | null;
  publicationPrNumber: number | null;
  publicationPrUrl: string | null;
  publicationPrStatus: string | null;
  publicationPushStatus: string | null;
  status: string;
  createdAt: string;
  updatedAt: string;
}

export interface StartAgentConversationInput {
  projectId: string;
  content: string;
  conversationId?: string | null;
  providerHarness?: string | null;
  modelId?: string | null;
  logicalEffort?: string | null;
  mode?: AgentConversationWorkspaceMode;
  base?: AgentConversationBaseSelection | null;
}

export interface StartAgentConversationResult {
  conversation: ChatConversation;
  workspace: AgentConversationWorkspace | null;
  sendResult: SendAgentMessageResult;
}

export interface SwitchAgentConversationModeInput {
  conversationId: string;
  mode: AgentConversationWorkspaceMode;
  base?: AgentConversationBaseSelection | null;
}

export interface SwitchAgentConversationModeResult {
  conversation: ChatConversation;
  workspace: AgentConversationWorkspace | null;
}

export interface PublishAgentConversationWorkspaceResult {
  workspace: AgentConversationWorkspace;
  commitSha: string | null;
  pushed: boolean;
  createdPr: boolean;
  prNumber: number | null;
  prUrl: string | null;
}

export interface AgentConversationWorkspacePublicationEvent {
  id: string;
  conversationId: string;
  step: string;
  status: string;
  summary: string;
  classification: string | null;
  createdAt: string;
}

export interface AgentConversationWorkspaceFreshness {
  conversationId: string;
  baseRef: string;
  baseDisplayName: string | null;
  targetRef: string;
  capturedBaseCommit: string | null;
  targetBaseCommit: string;
  isBaseAhead: boolean;
  hasUncommittedChanges: boolean;
  unpublishedCommitCount: number | null;
}

export interface UpdateAgentConversationWorkspaceFromBaseResult {
  workspace: AgentConversationWorkspace;
  updated: boolean;
  targetRef: string;
  baseCommit: string;
}

const SendAgentMessageResponseSchema = z.object({
  conversation_id: z.string(),
  agent_run_id: z.string(),
  is_new_conversation: z.boolean(),
  was_queued: z.boolean().optional().default(false),
  queued_as_pending: z.boolean().optional().default(false),
  queued_message_id: z.string().optional().nullable(),
});

type RawSendAgentMessageResponse = z.infer<typeof SendAgentMessageResponseSchema>;

const AgentConversationWorkspaceResponseSchema = z.object({
  conversation_id: z.string(),
  project_id: z.string(),
  mode: z.string(),
  base_ref_kind: z.string(),
  base_ref: z.string(),
  base_display_name: z.string().nullable(),
  base_commit: z.string().nullable(),
  branch_name: z.string(),
  worktree_path: z.string(),
  linked_ideation_session_id: z.string().nullable(),
  linked_plan_branch_id: z.string().nullable(),
  publication_pr_number: z.number().nullable(),
  publication_pr_url: z.string().nullable(),
  publication_pr_status: z.string().nullable(),
  publication_push_status: z.string().nullable(),
  status: z.string(),
  created_at: z.string(),
  updated_at: z.string(),
});
const AgentConversationWorkspaceListResponseSchema = z.array(
  AgentConversationWorkspaceResponseSchema
);
const AgentConversationWorkspacePublicationEventResponseSchema = z.object({
  id: z.string(),
  conversation_id: z.string(),
  step: z.string(),
  status: z.string(),
  summary: z.string(),
  classification: z.string().nullable(),
  created_at: z.string(),
});
const AgentConversationWorkspacePublicationEventListResponseSchema = z.array(
  AgentConversationWorkspacePublicationEventResponseSchema
);
const AgentConversationWorkspaceFreshnessResponseSchema = z.object({
  conversation_id: z.string(),
  base_ref: z.string(),
  base_display_name: z.string().nullable(),
  target_ref: z.string(),
  captured_base_commit: z.string().nullable(),
  target_base_commit: z.string(),
  is_base_ahead: z.boolean(),
  has_uncommitted_changes: z.boolean(),
  unpublished_commit_count: z.number().nullable(),
});

const StartAgentConversationResponseSchema = z.object({
  conversation: ChatConversationResponseSchema,
  workspace: AgentConversationWorkspaceResponseSchema.nullable(),
  send_result: SendAgentMessageResponseSchema,
});

const SwitchAgentConversationModeResponseSchema = z.object({
  conversation: ChatConversationResponseSchema,
  workspace: AgentConversationWorkspaceResponseSchema.nullable(),
});

const PublishAgentConversationWorkspaceResponseSchema = z.object({
  workspace: AgentConversationWorkspaceResponseSchema,
  commit_sha: z.string().nullable(),
  pushed: z.boolean(),
  created_pr: z.boolean(),
  pr_number: z.number().nullable(),
  pr_url: z.string().nullable(),
});
const UpdateAgentConversationWorkspaceFromBaseResponseSchema = z.object({
  workspace: AgentConversationWorkspaceResponseSchema,
  updated: z.boolean(),
  target_ref: z.string(),
  base_commit: z.string(),
});

type RawAgentConversationWorkspace = z.infer<
  typeof AgentConversationWorkspaceResponseSchema
>;
type RawStartAgentConversationResponse = z.infer<
  typeof StartAgentConversationResponseSchema
>;
type RawSwitchAgentConversationModeResponse = z.infer<
  typeof SwitchAgentConversationModeResponseSchema
>;
type RawPublishAgentConversationWorkspaceResponse = z.infer<
  typeof PublishAgentConversationWorkspaceResponseSchema
>;
type RawAgentConversationWorkspacePublicationEvent = z.infer<
  typeof AgentConversationWorkspacePublicationEventResponseSchema
>;
type RawAgentConversationWorkspaceFreshness = z.infer<
  typeof AgentConversationWorkspaceFreshnessResponseSchema
>;
type RawUpdateAgentConversationWorkspaceFromBaseResponse = z.infer<
  typeof UpdateAgentConversationWorkspaceFromBaseResponseSchema
>;

function transformSendAgentMessageResponse(raw: RawSendAgentMessageResponse): SendAgentMessageResult {
  return {
    conversationId: raw.conversation_id,
    agentRunId: raw.agent_run_id,
    isNewConversation: raw.is_new_conversation,
    wasQueued: raw.was_queued,
    queuedAsPending: raw.queued_as_pending,
    queuedMessageId: raw.queued_message_id,
  };
}

function transformAgentConversationWorkspace(
  raw: RawAgentConversationWorkspace
): AgentConversationWorkspace {
  return {
    conversationId: raw.conversation_id,
    projectId: raw.project_id,
    mode: raw.mode as AgentConversationWorkspaceMode,
    baseRefKind: raw.base_ref_kind,
    baseRef: raw.base_ref,
    baseDisplayName: raw.base_display_name,
    baseCommit: raw.base_commit,
    branchName: raw.branch_name,
    worktreePath: raw.worktree_path,
    linkedIdeationSessionId: raw.linked_ideation_session_id,
    linkedPlanBranchId: raw.linked_plan_branch_id,
    publicationPrNumber: raw.publication_pr_number,
    publicationPrUrl: raw.publication_pr_url,
    publicationPrStatus: raw.publication_pr_status,
    publicationPushStatus: raw.publication_push_status,
    status: raw.status,
    createdAt: raw.created_at,
    updatedAt: raw.updated_at,
  };
}

function transformStartAgentConversationResponse(
  raw: RawStartAgentConversationResponse
): StartAgentConversationResult {
  return {
    conversation: transformConversation(raw.conversation),
    workspace: raw.workspace ? transformAgentConversationWorkspace(raw.workspace) : null,
    sendResult: transformSendAgentMessageResponse(raw.send_result),
  };
}

function transformSwitchAgentConversationModeResponse(
  raw: RawSwitchAgentConversationModeResponse
): SwitchAgentConversationModeResult {
  return {
    conversation: transformConversation(raw.conversation),
    workspace: raw.workspace ? transformAgentConversationWorkspace(raw.workspace) : null,
  };
}

function transformPublishAgentConversationWorkspaceResponse(
  raw: RawPublishAgentConversationWorkspaceResponse
): PublishAgentConversationWorkspaceResult {
  return {
    workspace: transformAgentConversationWorkspace(raw.workspace),
    commitSha: raw.commit_sha,
    pushed: raw.pushed,
    createdPr: raw.created_pr,
    prNumber: raw.pr_number,
    prUrl: raw.pr_url,
  };
}

function transformAgentConversationWorkspacePublicationEvent(
  raw: RawAgentConversationWorkspacePublicationEvent
): AgentConversationWorkspacePublicationEvent {
  return {
    id: raw.id,
    conversationId: raw.conversation_id,
    step: raw.step,
    status: raw.status,
    summary: raw.summary,
    classification: raw.classification,
    createdAt: raw.created_at,
  };
}

function transformAgentConversationWorkspaceFreshness(
  raw: RawAgentConversationWorkspaceFreshness
): AgentConversationWorkspaceFreshness {
  return {
    conversationId: raw.conversation_id,
    baseRef: raw.base_ref,
    baseDisplayName: raw.base_display_name,
    targetRef: raw.target_ref,
    capturedBaseCommit: raw.captured_base_commit,
    targetBaseCommit: raw.target_base_commit,
    isBaseAhead: raw.is_base_ahead,
    hasUncommittedChanges: raw.has_uncommitted_changes,
    unpublishedCommitCount: raw.unpublished_commit_count,
  };
}

function transformUpdateAgentConversationWorkspaceFromBaseResponse(
  raw: RawUpdateAgentConversationWorkspaceFromBaseResponse
): UpdateAgentConversationWorkspaceFromBaseResult {
  return {
    workspace: transformAgentConversationWorkspace(raw.workspace),
    updated: raw.updated,
    targetRef: raw.target_ref,
    baseCommit: raw.base_commit,
  };
}

export async function getAgentConversationWorkspace(
  conversationId: string
): Promise<AgentConversationWorkspace | null> {
  const raw = await typedInvoke(
    "get_agent_conversation_workspace",
    { conversationId },
    AgentConversationWorkspaceResponseSchema.nullable()
  );
  return raw ? transformAgentConversationWorkspace(raw) : null;
}

export async function listAgentConversationWorkspacesByProject(
  projectId: string
): Promise<AgentConversationWorkspace[]> {
  const raw = await typedInvoke(
    "list_agent_conversation_workspaces_by_project",
    { projectId },
    AgentConversationWorkspaceListResponseSchema
  );
  return raw.map(transformAgentConversationWorkspace);
}

export async function listAgentConversationWorkspacePublicationEvents(
  conversationId: string
): Promise<AgentConversationWorkspacePublicationEvent[]> {
  const raw = await typedInvoke(
    "list_agent_conversation_workspace_publication_events",
    { conversationId },
    AgentConversationWorkspacePublicationEventListResponseSchema
  );
  return raw.map(transformAgentConversationWorkspacePublicationEvent);
}

export async function getAgentConversationWorkspaceFreshness(
  conversationId: string
): Promise<AgentConversationWorkspaceFreshness> {
  const raw = await typedInvoke(
    "get_agent_conversation_workspace_freshness",
    { conversationId },
    AgentConversationWorkspaceFreshnessResponseSchema
  );
  return transformAgentConversationWorkspaceFreshness(raw);
}

export async function updateAgentConversationWorkspaceFromBase(
  conversationId: string
): Promise<UpdateAgentConversationWorkspaceFromBaseResult> {
  const raw = await typedInvoke(
    "update_agent_conversation_workspace_from_base",
    { conversationId },
    UpdateAgentConversationWorkspaceFromBaseResponseSchema
  );
  return transformUpdateAgentConversationWorkspaceFromBaseResponse(raw);
}

export async function publishAgentConversationWorkspace(
  conversationId: string
): Promise<PublishAgentConversationWorkspaceResult> {
  const raw = await typedInvoke(
    "publish_agent_conversation_workspace",
    { conversationId },
    PublishAgentConversationWorkspaceResponseSchema
  );
  return transformPublishAgentConversationWorkspaceResponse(raw);
}

export async function closeAgentWorkspacePr(
  conversationId: string
): Promise<AgentConversationWorkspace> {
  const raw = await typedInvoke(
    "close_agent_workspace_pr",
    { conversationId },
    AgentConversationWorkspaceResponseSchema
  );
  return transformAgentConversationWorkspace(raw);
}

export async function startAgentConversation(
  input: StartAgentConversationInput
): Promise<StartAgentConversationResult> {
  const raw = await typedInvoke(
    "start_agent_conversation",
    {
      input: {
        projectId: input.projectId,
        content: input.content,
        ...(input.conversationId ? { conversationId: input.conversationId } : {}),
        ...(input.providerHarness ? { providerHarness: input.providerHarness } : {}),
        ...(input.modelId ? { modelOverride: input.modelId } : {}),
        ...(input.logicalEffort ? { logicalEffort: input.logicalEffort } : {}),
        ...(input.mode ? { mode: input.mode } : {}),
        ...(input.base
          ? {
              baseRefKind: input.base.kind,
              baseRef: input.base.ref,
              baseDisplayName: input.base.displayName,
            }
          : {}),
      },
    },
    StartAgentConversationResponseSchema
  );
  return transformStartAgentConversationResponse(raw);
}

export async function switchAgentConversationMode(
  input: SwitchAgentConversationModeInput
): Promise<SwitchAgentConversationModeResult> {
  const raw = await typedInvoke(
    "switch_agent_conversation_mode",
    {
      input: {
        conversationId: input.conversationId,
        mode: input.mode,
        ...(input.base
          ? {
              baseRefKind: input.base.kind,
              baseRef: input.base.ref,
              baseDisplayName: input.base.displayName,
            }
          : {}),
      },
    },
    SwitchAgentConversationModeResponseSchema
  );
  return transformSwitchAgentConversationModeResponse(raw);
}

/**
 * Send a message using the unified agent API
 * Returns immediately with conversation_id and agent_run_id.
 * Processing happens in background with events emitted via Tauri.
 *
 * @param contextType The context type (ideation, task, project, task_execution)
 * @param contextId The context ID
 * @param content The message content
 * @param attachmentIds Optional array of attachment IDs to link to this message
 */
export async function sendAgentMessage(
  contextType: ContextType,
  contextId: string,
  content: string,
  attachmentIds?: string[],
  target?: string,
  options?: {
    conversationId?: string | null;
    providerHarness?: string | null;
    modelId?: string | null;
    logicalEffort?: string | null;
  }
): Promise<SendAgentMessageResult> {
  const raw = await typedInvoke(
    "send_agent_message",
    {
      input: {
        contextType,
        contextId,
        content,
        ...(attachmentIds !== undefined && attachmentIds.length > 0 && { attachmentIds }),
        ...(target !== undefined && { target }),
        ...(options?.conversationId ? { conversationId: options.conversationId } : {}),
        ...(options?.providerHarness ? { providerHarness: options.providerHarness } : {}),
        ...(options?.modelId ? { modelOverride: options.modelId } : {}),
        ...(options?.logicalEffort ? { logicalEffort: options.logicalEffort } : {}),
      },
    },
    SendAgentMessageResponseSchema
  );
  return transformSendAgentMessageResponse(raw);
}

/**
 * Get all queued messages for a context
 *
 * @param contextType The context type
 * @param contextId The context ID
 */
export async function getQueuedAgentMessages(
  contextType: ContextType,
  contextId: string
): Promise<QueuedMessageResponse[]> {
  const raw = await typedInvoke(
    "get_queued_agent_messages",
    { contextType, contextId },
    z.array(QueuedMessageResponseSchema)
  );
  return raw.map(transformQueuedMessage);
}

/**
 * Delete a queued message before it's sent
 *
 * @param contextType The context type
 * @param contextId The context ID
 * @param messageId The message ID to delete
 */
export async function deleteQueuedAgentMessage(
  contextType: ContextType,
  contextId: string,
  messageId: string
): Promise<boolean> {
  return typedInvoke(
    "delete_queued_agent_message",
    { contextType, contextId, messageId },
    z.boolean()
  );
}

/**
 * Check if the chat service is available (Claude CLI installed)
 */
export async function isChatServiceAvailable(): Promise<boolean> {
  return typedInvoke(
    "is_chat_service_available",
    {},
    z.boolean()
  );
}

/**
 * Stop a running agent for a context
 * Sends SIGTERM to the running agent process.
 *
 * @param contextType The context type (ideation, task, project, task_execution)
 * @param contextId The context ID
 * @returns True if an agent was stopped, false if no agent was running
 */
export async function stopAgent(
  contextType: ContextType,
  contextId: string
): Promise<boolean> {
  return typedInvoke(
    "stop_agent",
    { contextType, contextId },
    z.boolean()
  );
}

/**
 * Check if an agent is currently running for a context
 *
 * @param contextType The context type
 * @param contextId The context ID
 */
export async function isAgentRunning(
  contextType: ContextType,
  contextId: string
): Promise<boolean> {
  return typedInvoke(
    "is_agent_running",
    { contextType, contextId },
    z.boolean()
  );
}

// ============================================================================
// Chat Attachments API
// ============================================================================

/**
 * Chat attachment response from backend
 */
export interface ChatAttachmentResponse {
  id: string;
  conversationId: string;
  messageId: string | null;
  fileName: string;
  filePath: string;
  mimeType: string | null;
  fileSize: number;
  createdAt: string;
}

const ChatAttachmentResponseSchema = z.object({
  id: z.string(),
  conversationId: z.string(),
  messageId: z.string().nullable(),
  fileName: z.string(),
  filePath: z.string(),
  mimeType: z.string().nullable(),
  fileSize: z.number(),
  createdAt: z.string(),
});

/**
 * List all attachments for a specific message
 *
 * @param messageId The message ID
 * @returns Array of attachments
 */
export async function listMessageAttachments(
  messageId: string
): Promise<ChatAttachmentResponse[]> {
  return typedInvoke(
    "list_message_attachments",
    { messageId },
    z.array(ChatAttachmentResponseSchema)
  );
}
