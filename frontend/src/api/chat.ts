// Tauri invoke wrappers for unified chat API with type safety using Zod schemas

import { invoke } from "@tauri-apps/api/core";
import { z } from "zod";
import type {
  AgentConversationMode,
  ChatConversation,
  CoordinationMode,
  AgentRun,
  ContextType,
} from "../types/chat-conversation";
import {
  AgentConversationModeSchema,
  CoordinationModeSchema,
  ContextTypeSchema,
  normalizeConversationProviderMetadata,
} from "../types/chat-conversation";
import type { ToolCall } from "../components/Chat/ToolCallIndicator";
import type { ToolCallDetailRef } from "../components/Chat/tool-widgets/shared.constants";
import type { ContentBlockItem } from "../components/Chat/MessageItem";
import type { MessageAttachment } from "../components/Chat/MessageAttachments";
import { isWebMode } from "@/lib/tauri-detection";
import { backendFetch } from "@/api/backend";
import {
  getTransportEnvironmentId,
  isRemoteEnvironmentId,
} from "@/lib/remote/active-environment";
import { RemoteTransportError } from "@/lib/remote/transport-errors";
import {
  ArtifactResponseSchema,
  transformArtifactResponse,
} from "@/api/artifact";
import type { Artifact } from "@/types/artifact";
import type { ManualRoleRuntimeSelection } from "@/api/manual-role-defaults.types";
import { FileDiffSchema, transformFileDiff, type FileDiff } from "./diff";
import {
  RunningIdeationSessionSchema,
  RunningProcessSchema,
} from "./running-processes.schemas";
import {
  transformRunningIdeationSession,
  transformRunningProcess,
} from "./running-processes.transforms";
import type {
  RunningIdeationSession,
  RunningProcess,
} from "./running-processes.types";

// ============================================================================
// Typed Invoke Helper
// ============================================================================

async function typedInvoke<T>(
  cmd: string,
  args: Record<string, unknown>,
  schema: z.ZodType<T>,
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
  /** Optimistic frontend-only attachments for messages not yet hydrated from backend. */
  attachments?: MessageAttachment[];
  /** Optional upstream sender attribution. */
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
  usageProvenance?: UsageProvenance | null;
  timelineStatus?: string | null;
  timelineKind?: string | null;
  timelineSequence?: number | null;
  timelineBlockIndex?: number | null;
  runId?: string | null;
  createdAt: string;
}

export type UsageProvenance =
  | "provider_turn_delta"
  | "derived_cumulative_delta"
  | "provider_snapshot_fallback"
  | "cumulative_baseline_only";

export interface AgentToolCallDetailResponse {
  toolCall: ToolCall;
}

export interface ChatTimelineItemResponse {
  id: string;
  conversationId: string;
  messageId: string | null;
  runId: string | null;
  sequence: number;
  blockIndex: number;
  role: string;
  kind: string;
  status: string;
  content: string;
  contentBlocks: ContentBlockItem[];
  toolCall: ToolCall | null;
  metadata: string | null;
  providerHarness: string | null;
  providerSessionId: string | null;
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
  updatedAt: string;
  finalizedAt: string | null;
  asMessage: ChatMessageResponse;
}

// ============================================================================
// Parsing Utilities
// ============================================================================

function getRecord(value: unknown): Record<string, unknown> | null {
  return value != null && typeof value === "object"
    ? (value as Record<string, unknown>)
    : null;
}

function getNumberField(
  record: Record<string, unknown>,
  snake: string,
  camel: string,
): number | undefined {
  const value = record[snake] ?? record[camel];
  return typeof value === "number" ? value : undefined;
}

function getStringArrayField(
  record: Record<string, unknown>,
  snake: string,
  camel: string,
): string[] | undefined {
  const value = record[snake] ?? record[camel];
  return Array.isArray(value) && value.every((item) => typeof item === "string")
    ? value
    : undefined;
}

function normalizeToolCallDetailRef(
  raw: unknown,
): ToolCallDetailRef | undefined {
  const record = getRecord(raw);
  if (!record) return undefined;

  const conversationId = record.conversation_id ?? record.conversationId;
  const messageId = record.message_id ?? record.messageId;
  if (typeof conversationId !== "string" || typeof messageId !== "string") {
    return undefined;
  }

  const toolCallId = record.tool_call_id ?? record.toolCallId;
  const contentBlockIndex =
    record.content_block_index ?? record.contentBlockIndex;
  const detailRef: ToolCallDetailRef = { conversationId, messageId };
  if (typeof toolCallId === "string") {
    detailRef.toolCallId = toolCallId;
  }
  if (typeof contentBlockIndex === "number") {
    detailRef.contentBlockIndex = contentBlockIndex;
  }
  const timelineItemId = record.timeline_item_id ?? record.timelineItemId;
  if (typeof timelineItemId === "string") {
    detailRef.timelineItemId = timelineItemId;
  }
  return detailRef;
}

type ToolPreviewMetadataTarget = {
  resultPreviewTruncated?: boolean | undefined;
  resultPreviewOriginalBytes?: number | undefined;
  resultPreviewLineCount?: number | undefined;
  resultPreviewOmittedLines?: number | undefined;
  resultPreviewPaths?: string[] | undefined;
  argumentsPreviewTruncated?: boolean | undefined;
  argumentsPreviewOriginalBytes?: number | undefined;
  argumentsPreviewLineCount?: number | undefined;
  argumentsPreviewOmittedLines?: number | undefined;
  diffPreview?: FileDiff | undefined;
  detailRef?: ToolCallDetailRef | undefined;
};

function normalizeDiffPreview(raw: unknown): FileDiff | undefined {
  const parsed = FileDiffSchema.safeParse(raw);
  return parsed.success ? transformFileDiff(parsed.data) : undefined;
}

function applyToolPreviewMetadata(
  target: ToolPreviewMetadataTarget,
  raw: unknown,
) {
  const record = getRecord(raw);
  if (!record) return;

  const previewTruncated =
    record.result_preview_truncated ?? record.resultPreviewTruncated;
  if (previewTruncated === true) {
    target.resultPreviewTruncated = true;
  }

  const originalBytes = getNumberField(
    record,
    "result_preview_original_bytes",
    "resultPreviewOriginalBytes",
  );
  if (originalBytes != null) target.resultPreviewOriginalBytes = originalBytes;

  const lineCount = getNumberField(
    record,
    "result_preview_line_count",
    "resultPreviewLineCount",
  );
  if (lineCount != null) target.resultPreviewLineCount = lineCount;

  const omittedLines = getNumberField(
    record,
    "result_preview_omitted_lines",
    "resultPreviewOmittedLines",
  );
  if (omittedLines != null) target.resultPreviewOmittedLines = omittedLines;

  const previewPaths = getStringArrayField(
    record,
    "result_preview_paths",
    "resultPreviewPaths",
  );
  if (previewPaths != null) target.resultPreviewPaths = previewPaths;

  const argumentsPreviewTruncated =
    record.arguments_preview_truncated ?? record.argumentsPreviewTruncated;
  if (argumentsPreviewTruncated === true) {
    target.argumentsPreviewTruncated = true;
  }

  const argumentsOriginalBytes = getNumberField(
    record,
    "arguments_preview_original_bytes",
    "argumentsPreviewOriginalBytes",
  );
  if (argumentsOriginalBytes != null) {
    target.argumentsPreviewOriginalBytes = argumentsOriginalBytes;
  }

  const argumentsLineCount = getNumberField(
    record,
    "arguments_preview_line_count",
    "argumentsPreviewLineCount",
  );
  if (argumentsLineCount != null) {
    target.argumentsPreviewLineCount = argumentsLineCount;
  }

  const argumentsOmittedLines = getNumberField(
    record,
    "arguments_preview_omitted_lines",
    "argumentsPreviewOmittedLines",
  );
  if (argumentsOmittedLines != null) {
    target.argumentsPreviewOmittedLines = argumentsOmittedLines;
  }

  const diffPreview = normalizeDiffPreview(
    record.diff_preview ?? record.diffPreview,
  );
  if (diffPreview) {
    target.diffPreview = diffPreview;
  }

  const detailRef = normalizeToolCallDetailRef(
    record.detail_ref ?? record.detailRef,
  );
  if (detailRef) target.detailRef = detailRef;
}

function normalizeToolCall(raw: unknown, idx = 0): ToolCall {
  const record = getRecord(raw) ?? {};
  const id = record.id;
  const name = record.name;
  const toolCall: ToolCall = {
    id: typeof id === "string" ? id : `tool-${idx}`,
    name: typeof name === "string" ? name : "unknown",
    arguments: record.arguments ?? record.input ?? {},
  };
  const blockIndex = getNumberField(record, "block_index", "blockIndex");
  if (blockIndex != null) {
    toolCall.blockIndex = blockIndex;
  }
  if ("result" in record) {
    toolCall.result = record.result;
  }
  const parentToolUseId = record.parent_tool_use_id ?? record.parentToolUseId;
  if (typeof parentToolUseId === "string") {
    toolCall.parentToolUseId = parentToolUseId;
  }
  if (typeof record.error === "string") {
    toolCall.error = record.error;
  }

  applyToolPreviewMetadata(toolCall, raw);

  const diffContext = record.diff_context ?? record.diffContext;
  if (diffContext != null && typeof diffContext === "object") {
    const diffRecord = diffContext as Record<string, unknown>;
    const filePath = diffRecord.file_path ?? diffRecord.filePath;
    if (typeof filePath === "string") {
      const oldContent = diffRecord.old_content ?? diffRecord.oldContent;
      const oldFileExists =
        diffRecord.old_file_exists ?? diffRecord.oldFileExists;
      toolCall.diffContext = { filePath };
      if (typeof oldContent === "string") {
        toolCall.diffContext.oldContent = oldContent;
      }
      if (typeof oldFileExists === "boolean") {
        toolCall.diffContext.oldFileExists = oldFileExists;
      }
    }
  }

  return toolCall;
}

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
      durationMs: typeof block.duration_ms === "number" ? block.duration_ms : block.durationMs,
      isSettled: typeof block.is_settled === "boolean" ? block.is_settled : block.isSettled,
      estimatedTokens: typeof block.estimated_tokens === "number" ? block.estimated_tokens : block.estimatedTokens,
      reasoningTokens: typeof block.reasoning_tokens === "number" ? block.reasoning_tokens : block.reasoningTokens,
      id: block.id,
      name: block.name,
      arguments: block.arguments ?? block.input,
      result: block.result,
      parentToolUseId: block.parent_tool_use_id ?? block.parentToolUseId,
    };
    applyToolPreviewMetadata(item, block);
    // Transform diff_context (snake_case) to diffContext (camelCase) for tool_use blocks
    const blockDiffContext = block.diff_context ?? block.diffContext;
    if (block.type === "tool_use" && blockDiffContext) {
      item.diffContext = {
        oldContent:
          blockDiffContext.old_content ??
          blockDiffContext.oldContent ??
          undefined,
        filePath: blockDiffContext.file_path ?? blockDiffContext.filePath,
      };
      const oldFileExists =
        blockDiffContext.old_file_exists ?? blockDiffContext.oldFileExists;
      if (typeof oldFileExists === "boolean") {
        item.diffContext.oldFileExists = oldFileExists;
      }
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

  return data.map((tc, idx) => normalizeToolCall(tc, idx));
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
  composerSelectionSnapshot?: ComposerSelectionSnapshot;
  attachmentIds: string[];
}

export interface ConversationListPageResponse {
  conversations: ChatConversation[];
  limit: number;
  offset: number;
  total: number;
  hasMore: boolean;
}

export type AgentSidebarPublicationState =
  "active" | "draft" | "merged" | "closed" | "uncommitted" | "unpushed";

export type AgentSidebarGroupBy =
  | "project"
  | "publication"
  | "automation"
  | "inbox";
export type AgentSidebarSort = "latest" | "az" | "za";
export type AgentSidebarAttentionLane = "needs" | "working" | "stale" | "done";

export interface AgentSidebarConversationsInput {
  projectIds: string[];
  includeArchived?: boolean;
  archivedOnly?: boolean;
  search?: string;
  publicationStates?: AgentSidebarPublicationState[];
  groupBy?: AgentSidebarGroupBy;
  sort?: AgentSidebarSort;
  limitPerGroup?: number;
  offsets?: Record<string, number>;
  pinnedConversationIds?: string[];
  priorityConversationIds?: string[];
}

export interface AgentSidebarConversationRow {
  conversation: ChatConversation;
  workspace: AgentConversationWorkspace | null;
  refKind: "pull-request" | "branch";
  refLabel: string;
  publicationState: AgentSidebarPublicationState;
  publicationLabel: string | null;
  attentionLane: AgentSidebarAttentionLane;
  parkedDelegateCount: number;
  actionVerb: string;
  isMuted: boolean;
}

export interface AgentSidebarConversationGroup {
  key: string;
  label: string;
  total: number;
  offset: number;
  limit: number;
  hasMore: boolean;
  rows: AgentSidebarConversationRow[];
}

export interface AgentSidebarConversationGroupsResponse {
  groups: AgentSidebarConversationGroup[];
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
  started_at?: string;
  completed_at?: string;
  timestamp_provenance?: "delegated_run" | "delegation_job";
  seq?: number;
}

/**
 * Response from GET /api/conversations/:id/active-state HTTP endpoint.
 * Used to hydrate streaming UI when navigating to an active agent execution.
 */
export interface ConversationActiveStateResponse {
  is_active: boolean;
  runId?: string;
  tool_calls: unknown[];
  streaming_tasks: ActiveStreamingTaskResponse[];
  partial_text: string;
  partial_text_segments?: string[];
  partial_thinking_segments?: string[];
}

const ConversationActiveStateResponseSchema = z.object({
  is_active: z.boolean(),
  run_id: z.string().min(1).optional(),
  tool_calls: z.array(z.unknown()).default([]),
  streaming_tasks: z.array(z.custom<ActiveStreamingTaskResponse>((value) => {
    if (value == null || typeof value !== "object") return false;
    const record = value as Record<string, unknown>;
    return typeof record.tool_use_id === "string"
      && typeof record.status === "string"
      && (record.started_at == null || typeof record.started_at === "string")
      && (record.completed_at == null || typeof record.completed_at === "string")
      && (record.timestamp_provenance == null
        || record.timestamp_provenance === "delegated_run"
        || record.timestamp_provenance === "delegation_job")
      && (record.seq == null || (typeof record.seq === "number" && Number.isFinite(record.seq)));
  })).default([]),
  partial_text: z.string().default(""),
  partial_text_segments: z.array(z.string()).default([]),
  partial_thinking_segments: z.array(z.string()).default([]),
});

/**
 * Fetch the active streaming state for a conversation.
 * Called when navigating to a conversation with an active agent execution
 * to hydrate the streaming UI with missed events.
 *
 * @param conversationId - The conversation ID
 * @returns The active state response
 */
export async function getConversationActiveState(
  conversationId: string,
): Promise<ConversationActiveStateResponse> {
  const res = await backendFetch(
    `conversations/${conversationId}/active-state`,
  );
  if (!res.ok) {
    throw new Error(`Failed to get conversation active state: ${res.status}`);
  }
  const parsed = ConversationActiveStateResponseSchema.parse(await res.json());
  return {
    is_active: parsed.is_active,
    ...(parsed.run_id ? { runId: parsed.run_id } : {}),
    tool_calls: parsed.tool_calls,
    streaming_tasks: parsed.streaming_tasks,
    partial_text: parsed.partial_text,
    partial_text_segments: parsed.partial_text_segments.length > 0
      ? parsed.partial_text_segments
      : parsed.partial_text.length > 0
        ? [parsed.partial_text]
        : [],
    partial_thinking_segments: parsed.partial_thinking_segments,
  };
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
  sessionId: string,
): Promise<ChildSessionStatusResponse> {
  if (isWebMode()) {
    const mockedResponse =
      await window.__mockChatApi?.getChildSessionStatus(sessionId);
    if (mockedResponse) {
      return mockedResponse;
    }
  }

  const res = await backendFetch(
    `ideation/sessions/${sessionId}/child-status?include_messages=true&message_limit=5`,
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
    lastEffectiveModel:
      raw.last_effective_model ?? raw.session?.last_effective_model ?? null,
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
  logical_model: z.string().nullable().optional(),
  effective_model_id: z.string().nullable().optional(),
  logical_effort: z.string().nullable().optional(),
  effective_effort: z.string().nullable().optional(),
  service_tier: z.string().nullable().optional(),
  agent_mode: AgentConversationModeSchema.nullable().optional(),
  bound_agent_name: z.string().nullable().optional(),
  persona_id: z.string().nullable().optional(),
  builder_draft_id: z.string().nullable().optional(),
  builder_result_persona_id: z.string().nullable().optional(),
  last_run_persona_run_id: z.string().nullable().optional(),
  last_run_persona_id: z.string().nullable().optional(),
  last_run_persona_slug: z.string().nullable().optional(),
  last_run_persona_version: z.number().int().nullable().optional(),
  last_run_persona_content_hash: z.string().nullable().optional(),
  last_run_persona_injected: z.boolean().nullable().optional(),
  last_run_persona_skipped_reason: z.string().nullable().optional(),
  persona_runs: z
    .array(
      z.object({
        run_id: z.string(),
        persona_id: z.string(),
        persona_slug: z.string(),
        persona_version: z.number().int(),
        persona_content_hash: z.string(),
        injected: z.boolean(),
        skipped_reason: z.string().nullable().optional(),
      }),
    )
    .optional()
    .default([]),
  coordination_mode: CoordinationModeSchema.optional().default("solo"),
  automation_id: z.string().nullable().optional(),
  automation_run_id: z.string().nullable().optional(),
  parent_conversation_id: z.string().nullable().optional(),
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
  persona_id: z.string().nullable().optional(),
  persona_slug: z.string().nullable().optional(),
  persona_version: z.number().int().nullable().optional(),
  persona_content_hash: z.string().nullable().optional(),
  persona_injected: z.boolean().nullable().optional(),
  persona_skipped_reason: z.string().nullable().optional(),
});

type RawConversation = z.infer<typeof ChatConversationResponseSchema>;
type RawConversationListPage = z.infer<
  typeof ConversationListPageResponseSchema
>;
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
    logicalModel: raw.logical_model ?? null,
    effectiveModelId: raw.effective_model_id ?? null,
    logicalEffort: raw.logical_effort ?? null,
    effectiveEffort: raw.effective_effort ?? null,
    serviceTier: raw.service_tier ?? null,
    agentMode: raw.agent_mode ?? null,
    boundAgentName: raw.bound_agent_name ?? null,
    personaId: raw.persona_id ?? null,
    builderDraftId: raw.builder_draft_id ?? null,
    builderResultPersonaId: raw.builder_result_persona_id ?? null,
    lastRunPersonaRunId: raw.last_run_persona_run_id ?? null,
    lastRunPersonaId: raw.last_run_persona_id ?? null,
    lastRunPersonaSlug: raw.last_run_persona_slug ?? null,
    lastRunPersonaVersion: raw.last_run_persona_version ?? null,
    lastRunPersonaContentHash: raw.last_run_persona_content_hash ?? null,
    lastRunPersonaInjected: raw.last_run_persona_injected ?? null,
    lastRunPersonaSkippedReason:
      raw.last_run_persona_skipped_reason ?? null,
    personaRuns: raw.persona_runs.map((run) => ({
      id: run.run_id,
      personaId: run.persona_id,
      personaSlug: run.persona_slug,
      personaVersion: run.persona_version,
      personaContentHash: run.persona_content_hash,
      personaInjected: run.injected,
      personaSkippedReason: run.skipped_reason ?? null,
    })),
    coordinationMode: raw.coordination_mode ?? "solo",
    automationId: raw.automation_id ?? null,
    automationRunId: raw.automation_run_id ?? null,
    parentConversationId: raw.parent_conversation_id ?? null,
    title: raw.title,
    messageCount: raw.message_count,
    lastMessageAt: raw.last_message_at,
    createdAt: raw.created_at,
    updatedAt: raw.updated_at,
    archivedAt: raw.archived_at ?? null,
  };
}

function transformConversationListPage(
  raw: RawConversationListPage,
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
  processedTokens: number | null;
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
  effectiveRunConversationCount: number;
  effectiveMessageConversationCount: number;
  legacyEstimatedSampleCount: number;
  fallbackEstimatedSampleCount: number;
  uncountedSampleCount: number;
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

export interface ConversationTimelinePageResponse {
  conversation: ChatConversation;
  items: ChatTimelineItemResponse[];
  messages: ChatMessageResponse[];
  limit: number;
  beforeSequence: number | null;
  totalItemCount: number;
  hasOlder: boolean;
  oldestLoadedSequence: number | null;
  newestLoadedSequence: number | null;
}

const SnakeUsageTotalsResponseSchema = z.object({
  input_tokens: z.number(),
  output_tokens: z.number(),
  cache_creation_tokens: z.number(),
  cache_read_tokens: z.number(),
  processed_tokens: z.number().nullable(),
  estimated_usd: z.number().nullable(),
});

const CamelUsageTotalsResponseSchema = z.object({
  inputTokens: z.number(),
  outputTokens: z.number(),
  cacheCreationTokens: z.number(),
  cacheReadTokens: z.number(),
  processedTokens: z.number().nullable(),
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
  effective_run_conversation_count: z.number(),
  effective_message_conversation_count: z.number(),
  legacy_estimated_sample_count: z.number(),
  fallback_estimated_sample_count: z.number(),
  uncounted_sample_count: z.number(),
  effective_totals_source: z.string(),
});

const CamelConversationUsageCoverageResponseSchema = z.object({
  providerMessageCount: z.number(),
  providerMessagesWithUsage: z.number(),
  runCount: z.number(),
  runsWithUsage: z.number(),
  effectiveRunConversationCount: z.number(),
  effectiveMessageConversationCount: z.number(),
  legacyEstimatedSampleCount: z.number(),
  fallbackEstimatedSampleCount: z.number(),
  uncountedSampleCount: z.number(),
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

function transformUsageTotals(
  raw: z.infer<typeof UsageTotalsResponseSchema>,
): UsageTotalsResponse {
  if ("inputTokens" in raw) {
    return {
      inputTokens: raw.inputTokens,
      outputTokens: raw.outputTokens,
      cacheCreationTokens: raw.cacheCreationTokens,
      cacheReadTokens: raw.cacheReadTokens,
      processedTokens: raw.processedTokens,
      estimatedUsd: raw.estimatedUsd,
    };
  }

  return {
    inputTokens: raw.input_tokens,
    outputTokens: raw.output_tokens,
    cacheCreationTokens: raw.cache_creation_tokens,
    cacheReadTokens: raw.cache_read_tokens,
    processedTokens: raw.processed_tokens,
    estimatedUsd: raw.estimated_usd,
  };
}

function transformUsageBucket(
  raw: z.infer<typeof UsageBucketResponseSchema>,
): UsageBucketResponse {
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
      effectiveRunConversationCount: raw.effectiveRunConversationCount,
      effectiveMessageConversationCount: raw.effectiveMessageConversationCount,
      legacyEstimatedSampleCount: raw.legacyEstimatedSampleCount,
      fallbackEstimatedSampleCount: raw.fallbackEstimatedSampleCount,
      uncountedSampleCount: raw.uncountedSampleCount,
      effectiveTotalsSource: raw.effectiveTotalsSource,
    };
  }

  return {
    providerMessageCount: raw.provider_message_count,
    providerMessagesWithUsage: raw.provider_messages_with_usage,
    runCount: raw.run_count,
    runsWithUsage: raw.runs_with_usage,
    effectiveRunConversationCount: raw.effective_run_conversation_count,
    effectiveMessageConversationCount: raw.effective_message_conversation_count,
    legacyEstimatedSampleCount: raw.legacy_estimated_sample_count,
    fallbackEstimatedSampleCount: raw.fallback_estimated_sample_count,
    uncountedSampleCount: raw.uncounted_sample_count,
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
    providerMessagesWithAttribution: raw.provider_messages_with_attribution,
    runCount: raw.run_count,
    runsWithAttribution: raw.runs_with_attribution,
  };
}

function transformConversationStats(
  raw: RawConversationStats,
): ConversationStatsResponse {
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
      attributionCoverage: transformAttributionCoverage(
        raw.attributionCoverage,
      ),
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
    personaId: raw.persona_id ?? null,
    personaSlug: raw.persona_slug ?? null,
    personaVersion: raw.persona_version ?? null,
    personaContentHash: raw.persona_content_hash ?? null,
    personaInjected: raw.persona_injected ?? null,
    personaSkippedReason: raw.persona_skipped_reason ?? null,
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
  usage_provenance: z.enum([
    "provider_turn_delta",
    "derived_cumulative_delta",
    "provider_snapshot_fallback",
    "cumulative_baseline_only",
  ]).nullable().optional(),
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

const AgentToolCallDetailResponseSchema = z.object({
  tool_call: z.any(),
});

type RawConversationMessagesPage = z.infer<
  typeof ConversationMessagesPageResponseSchema
>;

const AgentTimelineItemSchema = z.object({
  id: z.string(),
  conversation_id: z.string(),
  message_id: z.string().nullable().optional(),
  run_id: z.string().nullable().optional(),
  sequence: z.number().int(),
  block_index: z.number().int(),
  role: z.string(),
  kind: z.string(),
  status: z.string(),
  content: z.string(),
  content_blocks: z.any(),
  tool_call: z.any().nullable().optional(),
  metadata: z.string().nullable().optional(),
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
  usage_provenance: z.enum([
    "provider_turn_delta",
    "derived_cumulative_delta",
    "provider_snapshot_fallback",
    "cumulative_baseline_only",
  ]).nullable().optional(),
  created_at: z.string(),
  updated_at: z.string(),
  finalized_at: z.string().nullable().optional(),
});

const ConversationTimelinePageResponseSchema = z.object({
  conversation: ChatConversationResponseSchema,
  items: z.array(AgentTimelineItemSchema),
  limit: z.number().int().nonnegative(),
  before_sequence: z.number().int().nullable().optional(),
  total_item_count: z.number().int().nonnegative(),
  has_older: z.boolean(),
  oldest_loaded_sequence: z.number().int().nullable().optional(),
  newest_loaded_sequence: z.number().int().nullable().optional(),
});

type RawAgentTimelineItem = z.infer<typeof AgentTimelineItemSchema>;
type RawConversationTimelinePage = z.infer<
  typeof ConversationTimelinePageResponseSchema
>;

function transformAgentMessage(
  raw: RawAgentMessage,
  fallbackConversationId?: string,
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
    usageProvenance: raw.usage_provenance ?? null,
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
  raw: RawConversationMessagesPage,
): ConversationMessagesPageResponse {
  const conversationId = raw.conversation.id;
  return {
    conversation: transformConversation(raw.conversation),
    messages: raw.messages.map((message) =>
      transformAgentMessage(message, conversationId),
    ),
    limit: raw.limit,
    offset: raw.offset,
    totalMessageCount: raw.total_message_count,
    hasOlder: raw.has_older,
  };
}

function transformTimelineItem(
  raw: RawAgentTimelineItem,
  fallbackConversationId?: string,
): ChatTimelineItemResponse {
  const conversationId = raw.conversation_id ?? fallbackConversationId ?? null;
  const contentBlocks = parseContentBlocks(raw.content_blocks);
  const toolCall = raw.tool_call ? normalizeToolCall(raw.tool_call) : null;
  if (toolCall && toolCall.blockIndex == null) {
    toolCall.blockIndex = raw.block_index;
  }
  const asMessage: ChatMessageResponse = {
    id: raw.id,
    sessionId: null,
    projectId: null,
    taskId: null,
    role: raw.role,
    sender: null,
    attributionSource: null,
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
    usageProvenance: raw.usage_provenance ?? null,
    timelineStatus: raw.status,
    timelineKind: raw.kind,
    timelineSequence: raw.sequence,
    timelineBlockIndex: raw.block_index,
    runId: raw.run_id ?? null,
    content: raw.content,
    metadata: raw.metadata ?? null,
    parentMessageId: raw.message_id ?? null,
    conversationId,
    toolCalls: toolCall ? [toolCall] : null,
    contentBlocks,
    createdAt: raw.created_at,
  };

  return {
    id: raw.id,
    conversationId: raw.conversation_id,
    messageId: raw.message_id ?? null,
    runId: raw.run_id ?? null,
    sequence: raw.sequence,
    blockIndex: raw.block_index,
    role: raw.role,
    kind: raw.kind,
    status: raw.status,
    content: raw.content,
    contentBlocks,
    toolCall,
    metadata: raw.metadata ?? null,
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
    createdAt: raw.created_at,
    updatedAt: raw.updated_at,
    finalizedAt: raw.finalized_at ?? null,
    asMessage,
  };
}

function transformConversationTimelinePage(
  raw: RawConversationTimelinePage,
): ConversationTimelinePageResponse {
  const conversationId = raw.conversation.id;
  const items = raw.items.map((item) =>
    transformTimelineItem(item, conversationId),
  );
  return {
    conversation: transformConversation(raw.conversation),
    items,
    messages: items.map((item) => item.asMessage),
    limit: raw.limit,
    beforeSequence: raw.before_sequence ?? null,
    totalItemCount: raw.total_item_count,
    hasOlder: raw.has_older,
    oldestLoadedSequence: raw.oldest_loaded_sequence ?? null,
    newestLoadedSequence: raw.newest_loaded_sequence ?? null,
  };
}

/**
 * The remote half of the transcript reads (the client side of PR 3.2's read surface).
 *
 * The local `get_agent_conversation*` / `list_agent_conversations*` commands are
 * DELIBERATELY unregistered on the facade — each opens by waking the conversation's
 * agent workspace, which reaches a live-agent steer sink, and their absence is asserted
 * rather than merely omitted (`remote_server/registry.rs`,
 * `the_local_transcript_reads_stay_unregistered`). Calling them from a paired device
 * answers `REMOTE_COMMAND_UNAVAILABLE`, i.e. the Agents surface cannot load a
 * conversation at all.
 *
 * The host exposes spawn-free twins in `commands/remote_transcript_commands.rs` which
 * delegate to the SAME `*_for_app_state` seams the local commands use — identical
 * argument names, identical response payloads, no forked logic. Switching transports is
 * therefore a command-name choice and nothing else: the zod schemas and transforms below
 * are shared verbatim, and no extra await enters the transcript-hydration path.
 *
 * Each call site branches into TWO `typedInvoke` calls with literal command names rather
 * than computing one name. P-11 requires every production command name to be statically
 * enumerable — `scripts/check-remote-transport-drift.mjs` reads the invoke ARGUMENT, so a
 * helper returning the name is a hole in the proof that no command reaches the facade
 * unclassified, and the scanner fails the build on it. The duplication is the gate's price
 * and is deliberate; this mirrors the `sendAgentMessage`/`sendRemoteChatMessage` split.
 */
function remoteTranscriptReadsEnabled(): boolean {
  return isRemoteEnvironmentId(getTransportEnvironmentId());
}

/**
 * Page bounds the host enforces on the remote reads (`remote_transcript_commands.rs`
 * `DEFAULT_PAGE_LIMIT`/`MAX_PAGE_LIMIT`). The host CLAMPS rather than rejects, so an
 * over-wide request would come back narrower than asked without saying so, and the
 * infinite-query cursors would be computed against a page the client never requested.
 * Clamping here keeps the request we send the request the host actually runs.
 *
 * Local reads are untouched: the local commands apply their own bounds, and narrowing
 * them from the client would be a behaviour change nobody asked for.
 */
const REMOTE_MIN_PAGE_LIMIT = 1;
const REMOTE_MAX_PAGE_LIMIT = 200;

function wirePageLimit(limit: number): number {
  if (!remoteTranscriptReadsEnabled()) return limit;
  if (!Number.isFinite(limit)) return REMOTE_MAX_PAGE_LIMIT;
  return Math.min(
    Math.max(Math.trunc(limit), REMOTE_MIN_PAGE_LIMIT),
    REMOTE_MAX_PAGE_LIMIT,
  );
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
  includeArchived = false,
): Promise<ChatConversation[]> {
  const args = { contextType, contextId, includeArchived };
  const schema = z.array(ChatConversationResponseSchema);
  const raw = remoteTranscriptReadsEnabled()
    ? await typedInvoke("list_remote_agent_conversations", args, schema)
    : await typedInvoke("list_agent_conversations", args, schema);
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
  archivedOnly = false,
): Promise<ConversationListPageResponse> {
  const normalizedSearch = search?.trim();
  const args = {
    contextType,
    contextId,
    includeArchived,
    ...(archivedOnly ? { archivedOnly } : {}),
    limit: wirePageLimit(limit),
    offset,
    ...(normalizedSearch ? { search: normalizedSearch } : {}),
  };
  const raw = remoteTranscriptReadsEnabled()
    ? await typedInvoke(
        "list_remote_agent_conversations_page",
        args,
        ConversationListPageResponseSchema,
      )
    : await typedInvoke(
        "list_agent_conversations_page",
        args,
        ConversationListPageResponseSchema,
      );
  return transformConversationListPage(raw);
}

/**
 * Get lightweight conversation metadata without loading messages.
 */
export async function getConversationSummary(
  conversationId: string,
): Promise<ChatConversation | null> {
  const raw = await typedInvoke(
    "get_agent_conversation_summary",
    { conversationId },
    ChatConversationResponseSchema.nullable(),
  );
  return raw ? transformConversation(raw) : null;
}

/**
 * Get a conversation with its messages
 * @param conversationId The conversation ID
 * @returns The conversation with messages
 */
export async function getConversation(conversationId: string): Promise<{
  conversation: ChatConversation;
  messages: ChatMessageResponse[];
}> {
  const args = { conversationId };
  const schema = z.object({
    conversation: ChatConversationResponseSchema,
    messages: z.array(AgentMessageSchema),
  });
  const raw = remoteTranscriptReadsEnabled()
    ? await typedInvoke("get_remote_agent_conversation", args, schema)
    : await typedInvoke("get_agent_conversation", args, schema);

  return {
    conversation: transformConversation(raw.conversation),
    messages: raw.messages.map((message) =>
      transformAgentMessage(message, raw.conversation.id),
    ),
  };
}

/**
 * Legacy compatibility: get a tail-first page of conversation messages.
 * Visible Agent transcripts should prefer getConversationTimelinePage().
 * `offset` counts how many newest messages to skip before loading older history.
 */
export async function getConversationMessagesPage(
  conversationId: string,
  limit: number,
  offset = 0,
): Promise<ConversationMessagesPageResponse> {
  const args = { conversationId, limit: wirePageLimit(limit), offset };
  // Both commands return `Option`, so an unknown conversation id answers `null` on the
  // wire. Parsing it as a required object turned that into a raw schema dump; name the
  // condition instead. Null is NOT coerced to an empty page — that would render a real
  // conversation as empty and hide the mismatch.
  const raw = remoteTranscriptReadsEnabled()
    ? await typedInvoke(
        "get_remote_agent_conversation_messages_page",
        args,
        ConversationMessagesPageResponseSchema.nullable(),
      )
    : await typedInvoke(
        "get_agent_conversation_messages_page",
        args,
        ConversationMessagesPageResponseSchema.nullable(),
      );
  if (raw === null) {
    throw new Error(`Conversation ${conversationId} was not found on this host.`);
  }

  return transformConversationMessagesPage(raw);
}

/**
 * Get a tail-first page of normalized visible timeline items.
 * `beforeSequence` loads items older than the currently oldest loaded item.
 */
export async function getConversationTimelinePage(
  conversationId: string,
  limit: number,
  beforeSequence: number | null = null,
): Promise<ConversationTimelinePageResponse> {
  const args = { conversationId, limit: wirePageLimit(limit), beforeSequence };
  // `Option` on both sides — see `getConversationMessagesPage` for why null is named
  // rather than coerced into an empty page.
  const raw = remoteTranscriptReadsEnabled()
    ? await typedInvoke(
        "get_remote_agent_conversation_timeline_page",
        args,
        ConversationTimelinePageResponseSchema.nullable(),
      )
    : await typedInvoke(
        "get_agent_conversation_timeline_page",
        args,
        ConversationTimelinePageResponseSchema.nullable(),
      );
  if (raw === null) {
    throw new Error(`Conversation ${conversationId} was not found on this host.`);
  }

  return transformConversationTimelinePage(raw);
}

export async function getAgentMessageToolCallDetail(
  detailRef: ToolCallDetailRef,
): Promise<AgentToolCallDetailResponse | null> {
  if (detailRef.timelineItemId) {
    return getAgentTimelineItemToolCallDetail(
      detailRef.conversationId,
      detailRef.timelineItemId,
    );
  }

  const raw = await typedInvoke(
    "get_agent_message_tool_call_detail",
    {
      conversationId: detailRef.conversationId,
      messageId: detailRef.messageId,
      toolCallId: detailRef.toolCallId ?? null,
      contentBlockIndex: detailRef.contentBlockIndex ?? null,
    },
    AgentToolCallDetailResponseSchema.nullable(),
  );

  if (!raw) return null;
  return {
    toolCall: normalizeToolCall(raw.tool_call),
  };
}

export async function getAgentTimelineItemToolCallDetail(
  conversationId: string,
  timelineItemId: string,
): Promise<AgentToolCallDetailResponse | null> {
  const raw = await typedInvoke(
    "get_agent_timeline_item_tool_call_detail",
    { conversationId, timelineItemId },
    AgentToolCallDetailResponseSchema.nullable(),
  );

  if (!raw) return null;
  return {
    toolCall: normalizeToolCall(raw.tool_call),
  };
}

export async function getConversationStats(
  conversationId: string,
): Promise<ConversationStatsResponse | null> {
  const raw = await typedInvoke(
    "get_agent_conversation_stats",
    { conversationId },
    ConversationStatsResponseSchema.nullable(),
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
  contextId?: string | null,
  title?: string,
  mode?: AgentConversationMode,
): Promise<ChatConversation> {
  const raw = await typedInvoke(
    "create_agent_conversation",
    {
      input: {
        contextType,
        ...(contextId ? { contextId } : {}),
        ...(title !== undefined &&
          title.trim().length > 0 && { title: title.trim() }),
        ...(mode !== undefined && { mode }),
      },
    },
    ChatConversationResponseSchema,
  );
  return transformConversation(raw);
}

export async function updateConversationTitle(
  conversationId: string,
  title: string,
): Promise<ChatConversation> {
  const raw = await typedInvoke(
    "update_agent_conversation_title",
    {
      input: {
        conversationId,
        title: title.trim(),
      },
    },
    ChatConversationResponseSchema,
  );
  return transformConversation(raw);
}

export async function spawnConversationSessionNamer(
  conversationId: string,
  firstMessage: string,
  providerHarness?: string | null,
): Promise<void> {
  await invoke("spawn_session_namer", {
    conversationId,
    firstMessage,
    ...(providerHarness ? { providerHarness } : {}),
  });
}

const TerminalCleanupClaimSchema = z.enum([
  "claimed",
  "already_in_progress",
  "already_cleaned",
  "not_claimed",
]);

const TerminalLocalCleanupResultSchema = z.enum([
  "cleaned",
  "pending",
  "failed_unsafe",
  "failed_operational",
]);

const ArchiveConversationResponseSchema = z.object({
  conversation: ChatConversationResponseSchema,
  cleanup: z.object({
    runtime_shutdown_succeeded: z.boolean(),
    cleanup_claim: TerminalCleanupClaimSchema,
    local_cleanup: TerminalLocalCleanupResultSchema,
    message: z.string().nullable(),
  }),
});

export interface ArchiveConversationResult {
  conversation: ChatConversation;
  cleanup: {
    runtimeShutdownSucceeded: boolean;
    cleanupClaim: z.infer<typeof TerminalCleanupClaimSchema>;
    localCleanup: z.infer<typeof TerminalLocalCleanupResultSchema>;
    message: string | null;
  };
}

export async function archiveConversation(
  conversationId: string,
  options: { closePullRequest: boolean },
): Promise<ArchiveConversationResult> {
  const raw = await typedInvoke(
    "archive_agent_conversation",
    { conversationId, closePullRequest: options.closePullRequest },
    ArchiveConversationResponseSchema,
  );
  return {
    conversation: transformConversation(raw.conversation),
    cleanup: {
      runtimeShutdownSucceeded: raw.cleanup.runtime_shutdown_succeeded,
      cleanupClaim: raw.cleanup.cleanup_claim,
      localCleanup: raw.cleanup.local_cleanup,
      message: raw.cleanup.message,
    },
  };
}

export async function restoreConversation(
  conversationId: string,
): Promise<ChatConversation> {
  const raw = await typedInvoke(
    "restore_agent_conversation",
    { conversationId },
    ChatConversationResponseSchema,
  );
  return transformConversation(raw);
}

export async function setAgentConversationMuted(
  conversationId: string,
  muted: boolean,
): Promise<void> {
  await typedInvoke(
    "set_agent_conversation_muted",
    { input: { conversationId, muted } },
    z.null(),
  );
}

/**
 * Get the current agent run status for a conversation
 * @param conversationId The conversation ID
 * @returns The agent run if one is active, null otherwise
 */
export async function getAgentRunStatus(
  conversationId: string,
): Promise<AgentRun | null> {
  const raw = await typedInvoke(
    "get_agent_run_status_unified",
    { conversationId },
    AgentRunResponseSchema.nullable(),
  );
  return raw ? transformAgentRun(raw) : null;
}

// ============================================================================
// Namespace Export for Alternative Usage Pattern
// ============================================================================

const ComposerSelectionSnapshotSchema = z.object({
  sourceType: z.enum(["artifact", "ticket"]),
  sourceKind: z.enum(["plan", "jira", "linear", "clickup"]),
  sourceId: z.string(),
  sourceTitle: z.string().optional(),
  sourceKey: z.string().optional(),
  provider: z.enum(["atlassian", "linear", "clickup"]).optional(),
  artifactVersion: z.number().int().positive().optional(),
  sourceRevision: z.string().optional(),
  startLine: z.number().int().positive(),
  endLine: z.number().int().positive(),
  content: z.string(),
});

const QueuedMessageResponseSchema = z.object({
  id: z.string(),
  content: z.string(),
  created_at: z.string(),
  is_editing: z.boolean(),
  composer_selection_snapshot: ComposerSelectionSnapshotSchema.optional(),
  attachment_ids: z.array(z.string()).optional().default([]),
});

type RawQueuedMessage = z.infer<typeof QueuedMessageResponseSchema>;

function transformQueuedMessage(raw: RawQueuedMessage): QueuedMessageResponse {
  const selection = raw.composer_selection_snapshot;
  return {
    id: raw.id,
    content: raw.content,
    createdAt: raw.created_at,
    isEditing: raw.is_editing,
    ...(selection
      ? {
          composerSelectionSnapshot: {
            sourceType: selection.sourceType,
            sourceKind: selection.sourceKind,
            sourceId: selection.sourceId,
            ...(selection.sourceTitle
              ? { sourceTitle: selection.sourceTitle }
              : {}),
            ...(selection.sourceKey ? { sourceKey: selection.sourceKey } : {}),
            ...(selection.provider ? { provider: selection.provider } : {}),
            ...(selection.artifactVersion
              ? { artifactVersion: selection.artifactVersion }
              : {}),
            ...(selection.sourceRevision
              ? { sourceRevision: selection.sourceRevision }
              : {}),
            startLine: selection.startLine,
            endLine: selection.endLine,
            content: selection.content,
          },
        }
      : {}),
    attachmentIds: raw.attachment_ids,
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
  getConversationSummary,
  getConversation,
  getConversationMessagesPage,
  getConversationTimelinePage,
  getAgentMessageToolCallDetail,
  getAgentTimelineItemToolCallDetail,
  getConversationStats,
  createConversation,
  updateConversationTitle,
  spawnConversationSessionNamer,
  archiveConversation,
  restoreConversation,
  setAgentConversationMuted,
  getAgentConversationWorkspace,
  listWorkspaceOpenTargets,
  openAgentConversationWorkspace,
  openAgentConversationWorkspacePath,
  listAgentConversationWorkspacesByProject,
  listAgentSidebarConversations,
  listAgentConversationWorkspacePublicationEvents,
  getAgentConversationWorkspaceFreshness,
  reconcileAgentConversationWorkspacePublication,
  updateAgentConversationWorkspaceFromBase,
  precomputeAgentConversationWorkspacePrDescription,
  publishAgentConversationWorkspace,
  commitAgentConversationWorkspaceLocally,
  setAgentConversationWorkspaceAutoPublish,
  setAgentConversationWorkspacePrSupervision,
  setAgentConversationWorkspaceReviewAutomation,
  closeAgentWorkspacePr,
  getAgentWorkspacePrReviewContext,
  setAgentWorkspacePrReviewAutoApprove,
  setAgentWorkspacePrReviewMonitoring,
  getAgentWorkspaceReviewContext,
  getAgentWorkspaceReviewStartPreview,
  startAgentWorkspaceReview,
  startAgentWorkspaceReviewFixer,
  approveAgentWorkspaceReviewAnyway,
  listAgentConversationIssues,
  updateAgentConversationIssueStatus,
  convertAgentConversationIssueFollowup,
  submitAgentWorkspacePrReviewAction,
  skipAgentWorkspacePrReviewAction,
  getAgentRunStatus,
  getAgentRunningStates,
  getAgentConversationRuntimeIndex,
  getAgentConversationRuntimeStatuses,
  getBulkWorkspacePublicationStates,
  // Message sending & queue
  startAgentConversation,
  forkAgentConversation,
  switchAgentConversationMode,
  updateAgentConversationCoordinationMode,
  copyAgentConversationPlan,
  importAgentConversationPlan,
  activateAgentPlanDirectImplementation,
  activateAgentTaskPipeline,
  startAgentTaskPipeline,
  sendAgentMessage,
  getQueuedAgentMessages,
  deleteQueuedAgentMessage,
  sendQueuedAgentMessageNow,
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

export interface ComposerProjectReference {
  path: string;
  kind?: "file" | "directory";
}

export interface ComposerIntegrationReference {
  provider: "atlassian" | "linear" | "clickup" | "granola";
  kind: "jira" | "confluence" | "linear" | "clickup" | "note";
  id: string;
  key?: string;
  title?: string;
  url?: string;
  summaryExcerpt?: string;
  includeTranscript?: boolean;
}

export interface ComposerSelectionSnapshot {
  sourceType: "artifact" | "ticket" | "note";
  sourceKind: "plan" | "jira" | "linear" | "clickup" | "granola";
  sourceId: string;
  sourceTitle?: string;
  sourceKey?: string;
  provider?: "atlassian" | "linear" | "clickup" | "granola";
  artifactVersion?: number;
  sourceRevision?: string;
  startLine: number;
  endLine: number;
  content: string;
}

export interface ComposerArtifactReference {
  artifactId: string;
  kind: string;
  title?: string;
  sessionId?: string;
  version?: number;
  status?: string;
}

export type ComposerExcerptSourceKind =
  | "plan"
  | "review"
  | "issue"
  | "task"
  | "automation_spec"
  | "pull_request"
  | "workspace_diff"
  | "jira"
  | "linear"
  | "granola";

export interface ComposerExcerptReference {
  sourceKind: ComposerExcerptSourceKind;
  sourceId: string;
  sourceLabel: string;
  title?: string;
  excerpt: string;
  artifactId?: string;
  sessionId?: string;
  version?: number;
  url?: string;
  filePath?: string;
  revision?: string;
  locator?: string;
}

export type TeamIntentStrategy = "research" | "debate" | "execution";

export interface CapabilityIntent {
  coordinationMode: CoordinationMode;
  strategy?: TeamIntentStrategy | null;
}

export type TeamIntent = CapabilityIntent;

export type TeamMessageTargetKind = "coordinator" | "member" | "broadcast";

export interface TeamMessageTarget {
  kind: TeamMessageTargetKind;
  memberName?: string | null;
}

export interface SendAgentMessageOptions {
  conversationId?: string | null;
  providerHarness?: string | null;
  modelId?: string | null;
  logicalEffort?: string | null;
  codexFastMode?: boolean | null;
  runtimeOverride?: ManualRoleRuntimeSelection;
  suppressUserMessage?: boolean;
  requireApprovedLinkedPlan?: boolean;
  expectedLinkedPlanFingerprint?: string;
  capabilityIntent?: CapabilityIntent | null;
  teamIntent?: TeamIntent | null;
  teamMessageTarget?: TeamMessageTarget | null;
  composerProjectReferences?: ComposerProjectReference[];
  composerIntegrationReferences?: ComposerIntegrationReference[];
  composerArtifactReferences?: ComposerArtifactReference[];
  composerSelectionSnapshot?: ComposerSelectionSnapshot;
  composerExcerptReferences?: ComposerExcerptReference[];
}

export type AgentConversationWorkspaceMode = AgentConversationMode;
export type AgentConversationBaseRefKind =
  "project_default" | "current_branch" | "local_branch";
export type AgentConversationBranchMode = "isolated" | "linked";

export interface AgentConversationBaseSelection {
  kind: AgentConversationBaseRefKind;
  branchMode?: AgentConversationBranchMode;
  ref: string;
  displayName: string;
  sourcePullRequest?: AgentConversationSourcePullRequest | null;
}

export interface AgentConversationSourcePullRequest {
  number: number;
  url?: string | null;
  title?: string | null;
  headRefName: string;
  baseRefName?: string | null;
  headRefOid?: string | null;
}

export type AgentWorkspaceMaintenanceOperationSource =
  | "base_update"
  | "publish"
  | "pr_conflict"
  | "pr_autofix"
  | "legacy";
export type AgentWorkspaceMaintenanceOperationStage =
  | "updating_base"
  | "repairing"
  | "validating"
  | "reviewing"
  | "publishing"
  | "ready"
  | "blocked";
export type AgentWorkspaceMaintenanceOperationStatus =
  | "active"
  | "ready"
  | "blocked";
export type AgentWorkspaceMaintenanceOperationHoldReason =
  | "health_evidence"
  | "publish_redrive";

export interface AgentWorkspaceMaintenanceOperation {
  operationId: string;
  generation: number;
  source: AgentWorkspaceMaintenanceOperationSource;
  stage: AgentWorkspaceMaintenanceOperationStage;
  status: AgentWorkspaceMaintenanceOperationStatus;
  holdReason?: AgentWorkspaceMaintenanceOperationHoldReason | null;
  summary: string | null;
  blocker: string | null;
  automaticContinuation: boolean;
  startedAt: string;
  updatedAt: string;
}

export interface AgentWorkspacePrAutofixFingerprintSpend {
  generations: number;
  minutes: number;
  budgetMinutes: number;
  isExhausted: boolean;
}

export interface AgentConversationWorkspace {
  conversationId: string;
  projectId: string;
  mode: AgentConversationWorkspaceMode;
  branchMode: AgentConversationBranchMode;
  baseRefKind: string;
  baseRef: string;
  baseDisplayName: string | null;
  baseCommit: string | null;
  branchName: string;
  worktreePath: string;
  linkedIdeationSessionId: string | null;
  taskPipelineSessionId?: string | null;
  taskPipelineAvailable?: boolean;
  linkedPlanBranchId: string | null;
  sourcePullRequest?: AgentConversationSourcePullRequest | null;
  modeSwitchLocked?: boolean;
  modeSwitchLockReason?: string | null;
  publicationPrNumber: number | null;
  publicationPrUrl: string | null;
  publicationPrStatus: string | null;
  publicationPushStatus: string | null;
  maintenanceOperation?: AgentWorkspaceMaintenanceOperation | null;
  prAutofixFingerprintSpend?: AgentWorkspacePrAutofixFingerprintSpend | null;
  publicationMetadataAttemptId: string | null;
  publicationMetadataPhase: AgentWorkspacePublicationMetadataPhase | null;
  publicationMetadataState: AgentWorkspacePublicationMetadataState | null;
  autoPublishEnabled?: boolean;
  autoPublishInitialPrEnabled?: boolean;
  autoPublishPausedPrAutofixEnabled?: boolean | null;
  autoPublishPausedPrAutoMergeDesired?: boolean | null;
  prAutofixEnabled?: boolean;
  prAutoMergeDesired?: boolean;
  prAutoMergeMethod?: string;
  prAutoMergeCurrent?: boolean | null;
  prSupervisionStatus?: string | null;
  prSupervisionSummary?: string | null;
  prSupervisionUpdatedAt?: string | null;
  reviewAutomationOverride: boolean | null;
  status: string;
  createdAt: string;
  updatedAt: string;
}

export type AgentWorkspacePublicationMetadataPhase =
  | "prepared"
  | "mutating"
  | "reconciling"
  | "settled";

export type AgentWorkspacePublicationMetadataState =
  | "not_attempted"
  | "applied"
  | "not_applied"
  | "unknown"
  | "reconciled"
  | "conflicted";

export type WorkspaceOpenTargetKind = "editor" | "terminal" | "fileManager";

export interface WorkspaceOpenTarget {
  id: string;
  label: string;
  kind: WorkspaceOpenTargetKind;
}

export interface StartAgentConversationInput {
  projectId?: string | null;
  content: string;
  conversationId?: string | null;
  parentConversationId?: string | null;
  title?: string | null;
  providerHarness?: string | null;
  modelId?: string | null;
  logicalEffort?: string | null;
  personaId?: string | null;
  sourcePersonaId?: string | null;
  codexFastMode?: boolean | null;
  mode?: AgentConversationWorkspaceMode;
  base?: AgentConversationBaseSelection | null;
  capabilityIntent?: CapabilityIntent | null;
  teamIntent?: TeamIntent | null;
  composerProjectReferences?: ComposerProjectReference[];
  composerIntegrationReferences?: ComposerIntegrationReference[];
  composerArtifactReferences?: ComposerArtifactReference[];
  composerSelectionSnapshot?: ComposerSelectionSnapshot;
}

export interface StartAgentConversationResult {
  conversation: ChatConversation;
  workspace: AgentConversationWorkspace | null;
  sendResult: SendAgentMessageResult;
}

export interface ForkAgentConversationResult {
  parentConversation: ChatConversation;
  conversation: ChatConversation;
  workspace: AgentConversationWorkspace | null;
  providerSessionForked: boolean;
  copiedMessageCount: number;
  copiedTimelineItemCount: number;
}

export interface SwitchAgentConversationModeInput {
  conversationId: string;
  mode: AgentConversationWorkspaceMode;
  base?: AgentConversationBaseSelection | null;
  runtimeOverride?: ManualRoleRuntimeSelection;
}

export interface SwitchAgentConversationModeResult {
  conversation: ChatConversation;
  workspace: AgentConversationWorkspace | null;
}

export interface UpdateAgentConversationCoordinationModeInput {
  conversationId: string;
  coordinationMode: CoordinationMode;
  modelOverride?: string;
}

export interface CopyAgentConversationPlanInput {
  conversationId: string;
  sourceSessionId: string;
  sourceArtifactId: string;
  sourceVersion: number;
}

export interface ImportAgentConversationPlanInput {
  conversationId: string;
  title: string;
  content: string;
}

export interface AgentConversationPlanSeedResult {
  conversation: ChatConversation;
  workspace: AgentConversationWorkspace;
  sessionId: string;
  artifact: Artifact;
  blueprintArtifact: Artifact | null;
}

export interface PublishAgentConversationWorkspaceResult {
  workspace: AgentConversationWorkspace;
  commitSha: string | null;
  pushed: boolean;
  createdPr: boolean;
  prNumber: number | null;
  prUrl: string | null;
}

export interface CommitAgentConversationWorkspaceLocallyInput {
  expectedHeadSha: string;
  reviewArtifactId: string | null;
  reviewArtifactVersion: number | null;
  reviewedHeadSha: string | null;
  reviewedDiffFingerprint: string | null;
  attemptToken: string;
}

export interface CommitAgentConversationWorkspaceLocallyResult {
  workspace: AgentConversationWorkspace;
  outcome: "committed_local" | "already_committed" | "no_changes";
  branchName: string;
  previousHeadSha: string;
  commitSha: string;
  hadChanges: boolean;
  attemptToken: string;
}

export interface PrecomputeAgentConversationWorkspacePrDescriptionResult {
  conversationId: string;
  status: "ready" | "skipped";
  cacheStatus: string | null;
  reason: string | null;
}

export interface AgentConversationWorkspacePublicationEvent {
  id: string;
  conversationId: string;
  step: string;
  status: string;
  summary: string;
  classification: string | null;
  attemptId: string | null;
  createdAt: string;
}

export type AgentConversationWorkspaceBaseStatus =
  "valid" | "retargeted" | "blocked";
export type AgentConversationWorkspaceFreshnessScope = "local" | "full";

export interface AgentConversationWorkspaceFreshness {
  conversationId: string;
  freshnessScope: AgentConversationWorkspaceFreshnessScope;
  baseRef: string;
  baseDisplayName: string | null;
  targetRef: string;
  capturedBaseCommit: string | null;
  targetBaseCommit: string;
  isBaseAhead: boolean;
  hasUncommittedChanges: boolean;
  unpublishedCommitCount: number | null;
  remoteRefreshed: boolean;
  worktreeStatusChecked: boolean;
  baseStatus: AgentConversationWorkspaceBaseStatus;
  effectiveBaseRef: string | null;
  effectiveBaseDisplayName: string | null;
  baseBlockReason: string | null;
  recommendedActions: readonly string[];
}

export interface UpdateAgentConversationWorkspaceFromBaseResult {
  workspace: AgentConversationWorkspace;
  updated: boolean;
  repairStarted: boolean;
  targetRef: string;
  baseCommit: string;
  baseStatus: AgentConversationWorkspaceBaseStatus;
  effectiveBaseDisplayName: string | null;
}

export interface SetAgentConversationWorkspacePrSupervisionInput {
  autoFixEnabled: boolean;
  autoMergeDesired: boolean;
  autoMergeMethod?: string | null;
}

export interface SetAgentConversationWorkspaceAutoPublishInput {
  autoPublishEnabled: boolean;
}

export interface SetAgentConversationWorkspaceReviewAutomationInput {
  enabled: boolean | null;
}

export type AgentWorkspacePrReviewMonitorStatus =
  | "idle"
  | "reviewing"
  | "awaiting_user"
  | "watching"
  | "submitting"
  | "blocked"
  | "paused"
  | "terminal";

export type AgentWorkspaceReviewMonitorStatus =
  "idle" | "reviewing" | "ready" | "blocked";

export type AgentWorkspaceReviewOutcome =
  "none" | "passed" | "blocking" | "no_changes" | "run_failed";

export type AgentWorkspaceReviewGateStatus =
  "not_required" | "required" | "reviewing" | "passed" | "blocking" | "failed";

export type AgentWorkspaceReviewTargetScope =
  "selected_source" | "workspace_delta";

export type AgentWorkspaceReviewAutoMergeGuardStatus =
  | "pausing"
  | "paused_for_review"
  | "awaiting_publish"
  | "restoring"
  | "restore_failed";

export type AgentWorkspacePrReviewActionKind =
  "request_changes" | "approve" | "comment";

export type AgentWorkspacePrReviewActionStatus =
  | "pending"
  | "approved"
  | "skipped"
  | "submitting"
  | "submitted"
  | "failed"
  | "superseded";

export type AgentWorkspacePrReviewActionHeadStatus =
  | "current"
  | "stale"
  | "unverified";

export interface AgentWorkspacePrReviewMonitor {
  conversationId: string;
  projectId: string;
  prNumber: number;
  status: AgentWorkspacePrReviewMonitorStatus;
  monitorEnabled: boolean;
  autoApproveEnabled: boolean;
  firstReviewCompleted: boolean;
  firstActionResolved: boolean;
  lastSeenHeadSha: string | null;
  lastReviewedHeadSha: string | null;
  lastReviewRunId: string | null;
  lastReviewOutcome: string | null;
  lastSubmittedReviewId: string | null;
  reviewArtifactId: string | null;
  reviewArtifactHeadSha: string | null;
  reviewArtifactVersion: number | null;
  reviewArtifactUpdatedAt: string | null;
  lastError: string | null;
  createdAt: string;
  updatedAt: string;
}

export interface AgentWorkspacePrReviewAction {
  id: string;
  conversationId: string;
  prNumber: number;
  headSha: string;
  proposedAction: AgentWorkspacePrReviewActionKind;
  summary: string;
  reviewBody: string;
  findingsJson: string | null;
  status: AgentWorkspacePrReviewActionStatus;
  submittedReviewId: string | null;
  createdByRunId: string | null;
  createdAt: string;
  updatedAt: string;
  resolvedAt: string | null;
}

export interface AgentWorkspacePrReviewContext {
  success: boolean;
  workspace: AgentConversationWorkspace;
  events: AgentConversationWorkspacePublicationEvent[];
  prNumber: number;
  prUrl: string | null;
  currentHeadSha: string | null;
  pendingActionHeadStatus: AgentWorkspacePrReviewActionHeadStatus | null;
  health: unknown | null;
  reviewFeedback: unknown | null;
  monitor: AgentWorkspacePrReviewMonitor | null;
  pendingAction: AgentWorkspacePrReviewAction | null;
  recentActions: AgentWorkspacePrReviewAction[];
  issueCommentEvidence: unknown[];
}

export interface AgentWorkspaceReviewTarget {
  scope: AgentWorkspaceReviewTargetScope;
  baseRef: string;
  baseSha: string | null;
  headRef: string;
  headSha: string | null;
  diffFingerprint: string;
  sourcePullRequestNumber: number | null;
}

export interface AgentWorkspaceReviewMonitor {
  conversationId: string;
  projectId: string;
  status: AgentWorkspaceReviewMonitorStatus;
  reviewOutcome: AgentWorkspaceReviewOutcome;
  reviewGateStatus: AgentWorkspaceReviewGateStatus;
  currentTargetScope: AgentWorkspaceReviewTargetScope | null;
  reviewedTargetScope: AgentWorkspaceReviewTargetScope | null;
  reviewConversationId: string | null;
  reviewArtifactId: string | null;
  reviewArtifactVersion: number | null;
  reviewArtifactUpdatedAt: string | null;
  reviewRequestedChangesArtifactId?: string | null;
  reviewRequestedChangesArtifactVersion?: number | null;
  reviewRequestedChangesArtifactUpdatedAt?: string | null;
  reviewGateBypassedAt: string | null;
  reviewGateBypassedTargetScope: AgentWorkspaceReviewTargetScope | null;
  reviewGateBypassedDiffFingerprint: string | null;
  reviewGateBypassedArtifactId: string | null;
  reviewGateBypassedArtifactVersion: number | null;
  reviewedHeadSha: string | null;
  reviewedDiffFingerprint: string | null;
  selectedSourceBaseRef: string | null;
  selectedSourceBaseSha: string | null;
  selectedSourceHeadRef: string | null;
  selectedSourceHeadSha: string | null;
  selectedSourcePullRequestNumber: number | null;
  workspaceBaseRef: string | null;
  workspaceBaseSha: string | null;
  workspaceHeadRef: string | null;
  workspaceHeadSha: string | null;
  currentDiffFingerprint: string | null;
  previousVersionId: string | null;
  reviewRequestedChangesPreviousVersionId?: string | null;
  reviewBlockingSummary: string | null;
  reviewBlockingFingerprint: string | null;
  reviewFixerRunId: string | null;
  reviewFixerConversationId: string | null;
  reviewFixerStatus: string | null;
  reviewFixerCycleCount: number;
  lastRunId: string | null;
  lastError: string | null;
  autoMergeGuardStatus: AgentWorkspaceReviewAutoMergeGuardStatus | null;
  autoMergeGuardPrNumber: number | null;
  autoMergeGuardMethod: string | null;
  autoMergeGuardTargetScope: AgentWorkspaceReviewTargetScope | null;
  autoMergeGuardDiffFingerprint: string | null;
  autoMergeGuardHeadSha: string | null;
  autoMergeGuardLastError: string | null;
  createdAt: string;
  updatedAt: string;
}

export interface AgentWorkspaceReviewContext {
  success: boolean;
  workspace: AgentConversationWorkspace;
  events: AgentConversationWorkspacePublicationEvent[];
  target: AgentWorkspaceReviewTarget | null;
  monitor: AgentWorkspaceReviewMonitor;
  reviewArtifactIsCurrent: boolean;
  reviewArtifactIsOutdated: boolean;
  canMutateReviewState: boolean;
  reviewRuntimeState: AgentWorkspaceReviewRuntimeState;
  isCurrent: boolean;
  isOutdated: boolean;
  shouldShowTab: boolean;
}

export type AgentWorkspaceReviewRuntimeState =
  | "active_owned"
  | "terminal"
  | "missing_runtime_identity"
  | "malformed_runtime_identity"
  | "stale_runtime";

export interface AgentWorkspaceReviewStartConfirmation {
  targetScope: AgentWorkspaceReviewTargetScope | null;
  diffFingerprint: string | null;
  headSha: string | null;
  prNumber: number | null;
  willDisableAutoMerge: boolean;
  mergeMethod: string | null;
  restoreAfterPublish: boolean;
}

export interface AgentWorkspaceReviewStartPreview {
  success: boolean;
  target: AgentWorkspaceReviewTarget | null;
  willDisableAutoMerge: boolean;
  prNumber: number | null;
  mergeMethod: string | null;
  restoreAfterPublish: boolean;
  confirmation: AgentWorkspaceReviewStartConfirmation;
}

export interface StartAgentWorkspaceReviewResult {
  success: boolean;
  target: AgentWorkspaceReviewTarget | null;
  monitor: AgentWorkspaceReviewMonitor;
  reviewArtifactIsCurrent: boolean;
  reviewArtifactIsOutdated: boolean;
  canMutateReviewState: boolean;
  reviewRuntimeState: AgentWorkspaceReviewRuntimeState;
  isCurrent: boolean;
  isOutdated: boolean;
  shouldShowTab: boolean;
  started: boolean;
  skippedReason: string | null;
  wasQueued: boolean;
}

export interface StartAgentWorkspaceReviewFixerResult {
  success: boolean;
  target: AgentWorkspaceReviewTarget | null;
  monitor: AgentWorkspaceReviewMonitor;
  reviewArtifactIsCurrent: boolean;
  reviewArtifactIsOutdated: boolean;
  canMutateReviewState: boolean;
  reviewRuntimeState: AgentWorkspaceReviewRuntimeState;
  isCurrent: boolean;
  isOutdated: boolean;
  shouldShowTab: boolean;
  started: boolean;
  skippedReason: string | null;
}

export interface AgentWorkspaceReviewFixerConfirmation {
  targetScope: AgentWorkspaceReviewTargetScope;
  diffFingerprint: string;
  artifactId: string;
  artifactVersion: number;
  blockingFingerprint: string;
}

function roleRuntimeOverrideInvokeInput(value: ManualRoleRuntimeSelection) {
  return {
    harness: value.provider,
    model: value.model,
    effort: value.effort,
    serviceTier: value.serviceTier,
    coordinationMode: value.coordinationMode,
    personaId: value.personaId,
  };
}

function roleRuntimeOverrideHttpInput(value: ManualRoleRuntimeSelection) {
  return {
    provider: value.provider,
    model: value.model,
    effort: value.effort,
    service_tier: value.serviceTier,
    coordination_mode: value.coordinationMode,
    persona_id: value.personaId,
  };
}

export interface ApproveAgentWorkspaceReviewAnywayInput {
  targetScope: AgentWorkspaceReviewTargetScope;
  diffFingerprint: string;
  artifactId: string;
  artifactVersion: number;
}

export interface ApproveAgentWorkspaceReviewAnywayResult {
  success: boolean;
  monitor: AgentWorkspaceReviewMonitor;
}

export interface SubmitAgentWorkspacePrReviewActionResult {
  success: boolean;
  monitor: AgentWorkspacePrReviewMonitor;
  action: AgentWorkspacePrReviewAction;
  submittedReviewId: string;
  submittedReviewUrl: string | null;
}

export interface SkipAgentWorkspacePrReviewActionResult {
  success: boolean;
  monitor: AgentWorkspacePrReviewMonitor;
  action: AgentWorkspacePrReviewAction;
}

export interface SetAgentWorkspacePrReviewAutoApproveResult {
  success: boolean;
  monitor: AgentWorkspacePrReviewMonitor;
}

const SendAgentMessageResponseSchema = z.object({
  conversation_id: z.string(),
  agent_run_id: z.string(),
  is_new_conversation: z.boolean(),
  was_queued: z.boolean().optional().default(false),
  queued_as_pending: z.boolean().optional().default(false),
  queued_message_id: z.string().optional().nullable(),
});

type RawSendAgentMessageResponse = z.infer<
  typeof SendAgentMessageResponseSchema
>;

const AgentConversationWorkspaceSourcePullRequestResponseSchema = z.object({
  number: z.number(),
  url: z.string().nullable().optional().default(null),
  title: z.string().nullable().optional().default(null),
  head_ref_name: z.string(),
  base_ref_name: z.string().nullable().optional().default(null),
  head_ref_oid: z.string().nullable().optional().default(null),
});

export const AgentWorkspaceMaintenanceOperationResponseSchema = z.object({
  operation_id: z.string(),
  generation: z.number().int().positive(),
  source: z.enum(["base_update", "publish", "pr_conflict", "pr_autofix", "legacy"]),
  stage: z.enum([
    "updating_base",
    "repairing",
    "validating",
    "reviewing",
    "publishing",
    "ready",
    "blocked",
  ]),
  status: z.enum(["active", "ready", "blocked"]),
  hold_reason: z
    .enum(["health_evidence", "publish_redrive"])
    .nullable()
    .optional()
    .default(null),
  summary: z.string().nullable(),
  blocker: z.string().nullable(),
  automatic_continuation: z.boolean(),
  started_at: z.string(),
  updated_at: z.string(),
});

export const AgentWorkspacePrAutofixFingerprintSpendResponseSchema = z.object({
  generations: z.number().int().nonnegative(),
  minutes: z.number().int().nonnegative(),
  budget_minutes: z.number().int().nonnegative(),
  is_exhausted: z.boolean(),
});

export const AgentConversationWorkspaceResponseSchema = z.object({
  conversation_id: z.string(),
  project_id: z.string(),
  mode: z.string(),
  branch_mode: z.enum(["isolated", "linked"]).optional().default("isolated"),
  base_ref_kind: z.string(),
  base_ref: z.string(),
  base_display_name: z.string().nullable(),
  base_commit: z.string().nullable(),
  branch_name: z.string(),
  worktree_path: z.string(),
  linked_ideation_session_id: z.string().nullable(),
  task_pipeline_session_id: z.string().nullable().optional().default(null),
  task_pipeline_available: z.boolean().optional().default(false),
  linked_plan_branch_id: z.string().nullable(),
  source_pull_request:
    AgentConversationWorkspaceSourcePullRequestResponseSchema.nullable()
      .optional()
      .default(null),
  mode_switch_locked: z.boolean().optional().default(false),
  mode_switch_lock_reason: z.string().nullable().optional().default(null),
  publication_pr_number: z.number().nullable(),
  publication_pr_url: z.string().nullable(),
  publication_pr_status: z.string().nullable(),
  publication_push_status: z.string().nullable(),
  maintenance_operation: AgentWorkspaceMaintenanceOperationResponseSchema.nullable()
    .optional()
    .default(null),
  pr_autofix_fingerprint_spend:
    AgentWorkspacePrAutofixFingerprintSpendResponseSchema.nullable()
      .optional()
      .default(null),
  publication_metadata_attempt_id: z.string().nullable().optional().default(null),
  publication_metadata_phase: z
    .enum(["prepared", "mutating", "reconciling", "settled"])
    .nullable()
    .optional()
    .default(null),
  publication_metadata_state: z
    .enum([
      "not_attempted",
      "applied",
      "not_applied",
      "unknown",
      "reconciled",
      "conflicted",
    ])
    .nullable()
    .optional()
    .default(null),
  auto_publish_enabled: z.boolean().optional().default(true),
  auto_publish_initial_pr_enabled: z.boolean().optional().default(false),
  auto_publish_paused_pr_autofix_enabled: z
    .boolean()
    .nullable()
    .optional()
    .default(null),
  auto_publish_paused_pr_auto_merge_desired: z
    .boolean()
    .nullable()
    .optional()
    .default(null),
  pr_autofix_enabled: z.boolean().optional().default(false),
  pr_auto_merge_desired: z.boolean().optional().default(false),
  pr_auto_merge_method: z.string().optional().default("squash"),
  pr_auto_merge_current: z.boolean().nullable().optional().default(null),
  pr_supervision_status: z.string().nullable().optional().default(null),
  pr_supervision_summary: z.string().nullable().optional().default(null),
  pr_supervision_updated_at: z.string().nullable().optional().default(null),
  review_automation_override: z.boolean().nullable().optional().default(null),
  status: z.string(),
  created_at: z.string(),
  updated_at: z.string(),
});
const AgentConversationWorkspaceListResponseSchema = z.array(
  AgentConversationWorkspaceResponseSchema,
);
const AgentSidebarConversationRowResponseSchema = z.object({
  conversation: ChatConversationResponseSchema,
  workspace: AgentConversationWorkspaceResponseSchema.nullable(),
  ref_kind: z.enum(["pull_request", "branch"]),
  ref_label: z.string(),
  publication_state: z.enum([
    "active",
    "draft",
    "merged",
    "closed",
    "uncommitted",
    "unpushed",
  ]),
  publication_label: z.string().nullable(),
  attention_lane: z.enum(["needs", "working", "stale", "done"]),
  parked_delegate_count: z.number().int().nonnegative(),
  action_verb: z.string(),
  is_muted: z.boolean(),
});
const AgentSidebarConversationGroupResponseSchema = z.object({
  key: z.string(),
  label: z.string(),
  total: z.number(),
  offset: z.number(),
  limit: z.number(),
  has_more: z.boolean(),
  rows: z.array(AgentSidebarConversationRowResponseSchema),
});
const AgentSidebarConversationGroupsResponseSchema = z.object({
  groups: z.array(AgentSidebarConversationGroupResponseSchema),
});
const AgentConversationWorkspacePublicationEventResponseSchema = z.object({
  id: z.string(),
  conversation_id: z.string(),
  step: z.string(),
  status: z.string(),
  summary: z.string(),
  classification: z.string().nullable(),
  attempt_id: z.string().nullable().optional().default(null),
  created_at: z.string(),
});
const AgentConversationWorkspacePublicationEventListResponseSchema = z.array(
  AgentConversationWorkspacePublicationEventResponseSchema,
);
const AgentConversationWorkspaceFreshnessResponseSchema = z.object({
  conversation_id: z.string(),
  freshness_scope: z.enum(["local", "full"]).optional().default("full"),
  base_ref: z.string(),
  base_display_name: z.string().nullable(),
  target_ref: z.string(),
  captured_base_commit: z.string().nullable(),
  target_base_commit: z.string(),
  is_base_ahead: z.boolean(),
  has_uncommitted_changes: z.boolean(),
  unpublished_commit_count: z.number().nullable(),
  remote_refreshed: z.boolean().optional().default(true),
  worktree_status_checked: z.boolean().optional().default(true),
  base_status: z
    .enum(["valid", "retargeted", "blocked"])
    .optional()
    .default("valid"),
  effective_base_ref: z.string().nullable().optional().default(null),
  effective_base_display_name: z.string().nullable().optional().default(null),
  base_block_reason: z.string().nullable().optional().default(null),
  recommended_actions: z.array(z.string()).optional().default([]),
});
const AgentWorkspacePrReviewMonitorResponseSchema = z.object({
  conversation_id: z.string(),
  project_id: z.string(),
  pr_number: z.number(),
  status: z.enum([
    "idle",
    "reviewing",
    "awaiting_user",
    "watching",
    "submitting",
    "blocked",
    "paused",
    "terminal",
  ]),
  monitor_enabled: z.boolean(),
  auto_approve_enabled: z.boolean(),
  first_review_completed: z.boolean(),
  first_action_resolved: z.boolean(),
  last_seen_head_sha: z.string().nullable(),
  last_reviewed_head_sha: z.string().nullable(),
  last_review_run_id: z.string().nullable(),
  last_review_outcome: z.string().nullable(),
  last_submitted_review_id: z.string().nullable(),
  review_artifact_id: z.string().nullable().optional().default(null),
  review_artifact_head_sha: z.string().nullable().optional().default(null),
  review_artifact_version: z.number().nullable().optional().default(null),
  review_artifact_updated_at: z.string().nullable().optional().default(null),
  last_error: z.string().nullable(),
  created_at: z.string(),
  updated_at: z.string(),
});
const AgentWorkspacePrReviewActionResponseSchema = z.object({
  id: z.string(),
  conversation_id: z.string(),
  pr_number: z.number(),
  head_sha: z.string(),
  proposed_action: z.enum(["request_changes", "approve", "comment"]),
  summary: z.string(),
  review_body: z.string(),
  findings_json: z.string().nullable(),
  status: z.enum([
    "pending",
    "approved",
    "skipped",
    "submitting",
    "submitted",
    "failed",
    "superseded",
  ]),
  submitted_review_id: z.string().nullable(),
  created_by_run_id: z.string().nullable(),
  created_at: z.string(),
  updated_at: z.string(),
  resolved_at: z.string().nullable(),
});
const AgentWorkspacePrReviewContextResponseSchema = z.object({
  success: z.boolean(),
  workspace: AgentConversationWorkspaceResponseSchema,
  events: AgentConversationWorkspacePublicationEventListResponseSchema,
  pr_number: z.number(),
  pr_url: z.string().nullable(),
  current_head_sha: z.string().nullable(),
  pending_action_head_status: z
    .enum(["current", "stale", "unverified"])
    .nullable(),
  health: z.unknown().nullable(),
  review_feedback: z.unknown().nullable(),
  monitor: AgentWorkspacePrReviewMonitorResponseSchema.nullable(),
  pending_action: AgentWorkspacePrReviewActionResponseSchema.nullable(),
  recent_actions: z.array(AgentWorkspacePrReviewActionResponseSchema),
  issue_comment_evidence: z.array(z.unknown()),
});
const AgentWorkspaceReviewTargetResponseSchema = z.object({
  scope: z.enum(["selected_source", "workspace_delta"]),
  base_ref: z.string(),
  base_sha: z.string().nullable(),
  head_ref: z.string(),
  head_sha: z.string().nullable(),
  diff_fingerprint: z.string(),
  source_pull_request_number: z.number().nullable(),
});
const AgentWorkspaceReviewMonitorResponseSchema = z.object({
  conversation_id: z.string(),
  project_id: z.string(),
  status: z.enum(["idle", "reviewing", "ready", "blocked"]),
  review_outcome: z
    .enum(["none", "passed", "blocking", "no_changes", "run_failed"])
    .optional()
    .default("none"),
  review_gate_status: z
    .enum([
      "not_required",
      "required",
      "reviewing",
      "passed",
      "blocking",
      "failed",
    ])
    .optional()
    .default("not_required"),
  current_target_scope: z
    .enum(["selected_source", "workspace_delta"])
    .nullable(),
  reviewed_target_scope: z
    .enum(["selected_source", "workspace_delta"])
    .nullable(),
  review_conversation_id: z.string().nullable().optional(),
  review_artifact_id: z.string().nullable(),
  review_artifact_version: z.number().nullable(),
  review_artifact_updated_at: z.string().nullable(),
  review_requested_changes_artifact_id: z
    .string()
    .nullable()
    .optional()
    .default(null),
  review_requested_changes_artifact_version: z
    .number()
    .nullable()
    .optional()
    .default(null),
  review_requested_changes_artifact_updated_at: z
    .string()
    .nullable()
    .optional()
    .default(null),
  review_gate_bypassed_at: z.string().nullable().optional().default(null),
  review_gate_bypassed_target_scope: z
    .enum(["selected_source", "workspace_delta"])
    .nullable()
    .optional()
    .default(null),
  review_gate_bypassed_diff_fingerprint: z
    .string()
    .nullable()
    .optional()
    .default(null),
  review_gate_bypassed_artifact_id: z.string().nullable().optional().default(null),
  review_gate_bypassed_artifact_version: z
    .number()
    .nullable()
    .optional()
    .default(null),
  reviewed_head_sha: z.string().nullable(),
  reviewed_diff_fingerprint: z.string().nullable(),
  selected_source_base_ref: z.string().nullable(),
  selected_source_base_sha: z.string().nullable(),
  selected_source_head_ref: z.string().nullable(),
  selected_source_head_sha: z.string().nullable(),
  selected_source_pull_request_number: z.number().nullable(),
  workspace_base_ref: z.string().nullable(),
  workspace_base_sha: z.string().nullable(),
  workspace_head_ref: z.string().nullable(),
  workspace_head_sha: z.string().nullable(),
  current_diff_fingerprint: z.string().nullable(),
  previous_version_id: z.string().nullable(),
  review_requested_changes_previous_version_id: z
    .string()
    .nullable()
    .optional()
    .default(null),
  review_blocking_summary: z.string().nullable().optional().default(null),
  review_blocking_fingerprint: z.string().nullable().optional().default(null),
  review_fixer_run_id: z.string().nullable().optional().default(null),
  review_fixer_conversation_id: z.string().nullable().optional().default(null),
  review_fixer_status: z.string().nullable().optional().default(null),
  review_fixer_cycle_count: z.number().optional().default(0),
  last_run_id: z.string().nullable(),
  last_error: z.string().nullable(),
  auto_merge_guard_status: z
    .enum(["pausing", "paused_for_review", "awaiting_publish", "restoring", "restore_failed"])
    .nullable()
    .optional()
    .default(null),
  auto_merge_guard_pr_number: z.number().nullable().optional().default(null),
  auto_merge_guard_method: z.string().nullable().optional().default(null),
  auto_merge_guard_target_scope: z
    .enum(["selected_source", "workspace_delta"])
    .nullable()
    .optional()
    .default(null),
  auto_merge_guard_diff_fingerprint: z.string().nullable().optional().default(null),
  auto_merge_guard_head_sha: z.string().nullable().optional().default(null),
  auto_merge_guard_last_error: z.string().nullable().optional().default(null),
  created_at: z.string(),
  updated_at: z.string(),
});
const AgentWorkspaceReviewContextResponseSchema = z.object({
  success: z.boolean(),
  workspace: AgentConversationWorkspaceResponseSchema,
  events: AgentConversationWorkspacePublicationEventListResponseSchema,
  target: AgentWorkspaceReviewTargetResponseSchema.nullable(),
  monitor: AgentWorkspaceReviewMonitorResponseSchema,
  review_artifact_is_current: z.boolean().optional(),
  review_artifact_is_outdated: z.boolean().optional(),
  can_mutate_review_state: z.boolean().optional().default(false),
  review_runtime_state: z
    .enum([
      "active_owned",
      "terminal",
      "missing_runtime_identity",
      "malformed_runtime_identity",
      "stale_runtime",
    ])
    .optional()
    .default("missing_runtime_identity"),
  is_current: z.boolean(),
  is_outdated: z.boolean(),
  should_show_tab: z.boolean(),
});
const StartAgentWorkspaceReviewResponseSchema = z.object({
  success: z.boolean(),
  target: AgentWorkspaceReviewTargetResponseSchema.nullable(),
  monitor: AgentWorkspaceReviewMonitorResponseSchema,
  review_artifact_is_current: z.boolean().optional(),
  review_artifact_is_outdated: z.boolean().optional(),
  can_mutate_review_state: z.boolean().optional().default(false),
  review_runtime_state: z
    .enum([
      "active_owned",
      "terminal",
      "missing_runtime_identity",
      "malformed_runtime_identity",
      "stale_runtime",
    ])
    .optional()
    .default("missing_runtime_identity"),
  is_current: z.boolean(),
  is_outdated: z.boolean(),
  should_show_tab: z.boolean(),
  started: z.boolean(),
  skipped_reason: z.string().nullable(),
  was_queued: z.boolean(),
});
const AgentWorkspaceReviewStartConfirmationSchema = z.object({
  target_scope: z
    .enum(["selected_source", "workspace_delta"])
    .nullable()
    .optional()
    .default(null),
  diff_fingerprint: z.string().nullable().optional().default(null),
  head_sha: z.string().nullable().optional().default(null),
  pr_number: z.number().nullable().optional().default(null),
  will_disable_auto_merge: z.boolean(),
  merge_method: z.string().nullable(),
  restore_after_publish: z.boolean(),
});
const AgentWorkspaceReviewStartPreviewResponseSchema = z.object({
  success: z.boolean(),
  target: AgentWorkspaceReviewTargetResponseSchema.nullable(),
  will_disable_auto_merge: z.boolean(),
  pr_number: z.number().nullable(),
  merge_method: z.string().nullable(),
  restore_after_publish: z.boolean(),
  confirmation: AgentWorkspaceReviewStartConfirmationSchema,
});
const StartAgentWorkspaceReviewFixerResponseSchema = z.object({
  success: z.boolean(),
  target: AgentWorkspaceReviewTargetResponseSchema.nullable(),
  monitor: AgentWorkspaceReviewMonitorResponseSchema,
  review_artifact_is_current: z.boolean().optional(),
  review_artifact_is_outdated: z.boolean().optional(),
  can_mutate_review_state: z.boolean().optional().default(false),
  review_runtime_state: z
    .enum([
      "active_owned",
      "terminal",
      "missing_runtime_identity",
      "malformed_runtime_identity",
      "stale_runtime",
    ])
    .optional()
    .default("missing_runtime_identity"),
  is_current: z.boolean(),
  is_outdated: z.boolean(),
  should_show_tab: z.boolean(),
  started: z.boolean(),
  skipped_reason: z.string().nullable(),
});
const ApproveAgentWorkspaceReviewAnywayResponseSchema = z.object({
  success: z.boolean(),
  monitor: AgentWorkspaceReviewMonitorResponseSchema,
});
const SubmitAgentWorkspacePrReviewActionResponseSchema = z.object({
  success: z.boolean(),
  monitor: AgentWorkspacePrReviewMonitorResponseSchema,
  action: AgentWorkspacePrReviewActionResponseSchema,
  submitted_review_id: z.string(),
  submitted_review_url: z.string().nullable(),
});
const SkipAgentWorkspacePrReviewActionResponseSchema = z.object({
  success: z.boolean(),
  monitor: AgentWorkspacePrReviewMonitorResponseSchema,
  action: AgentWorkspacePrReviewActionResponseSchema,
});
const SetAgentWorkspacePrReviewAutoApproveResponseSchema = z.object({
  success: z.boolean(),
  monitor: AgentWorkspacePrReviewMonitorResponseSchema,
});
const WorkspaceOpenTargetResponseSchema = z.object({
  id: z.string(),
  label: z.string(),
  kind: z.enum(["editor", "terminal", "fileManager"]),
});

export const StartAgentConversationResponseSchema = z.object({
  conversation: ChatConversationResponseSchema,
  workspace: AgentConversationWorkspaceResponseSchema.nullable(),
  send_result: SendAgentMessageResponseSchema,
});

/**
 * The remote spawn-free conversation-start seam (contract §2).
 *
 * A paired remote client cannot reach the local process-spawn sink, so it never calls
 * `start_agent_conversation`. Instead it PERSISTS a start-intent through
 * `request_remote_agent_conversation_start` (the host seeds its own conversation and a
 * host-owned dispatcher does the actual spawn), then POLLS
 * `get_remote_conversation_start_request` until the intent reaches a terminal state.
 *
 * Status values are the host enum serialized camelCase
 * (`RemoteConversationStartStatus`, `serde(rename_all = "camelCase")`).
 */
export const RemoteConversationStartStatusSchema = z.enum([
  "pending",
  "starting",
  "started",
  "failed",
  "cancelled",
  "failedStale",
]);
export type RemoteConversationStartStatus = z.infer<
  typeof RemoteConversationStartStatusSchema
>;

/** Non-`started` terminal states — each carries an `errorCode` the composer can retry from. */
const REMOTE_CONVERSATION_START_TERMINAL_STATUSES = [
  "started",
  "failed",
  "cancelled",
  "failedStale",
] as const;

/** Response of `request_remote_agent_conversation_start` — the persisted-intent handle. */
const RequestRemoteAgentConversationStartResponseSchema = z
  .object({
    startRequestId: z.string(),
    conversationId: z.string(),
    status: RemoteConversationStartStatusSchema,
    createdAt: z.string(),
  })
  .strict();

/** Response of `get_remote_conversation_start_request` — the post-submit poll target (§2.7). */
const GetRemoteConversationStartRequestResponseSchema = z
  .object({
    id: z.string(),
    conversationId: z.string(),
    status: RemoteConversationStartStatusSchema,
    errorCode: z.string().nullable(),
    agentRunId: z.string().nullable(),
    createdAt: z.string(),
    updatedAt: z.string(),
  })
  .strict();

type RemoteConversationStartRequest = z.infer<
  typeof GetRemoteConversationStartRequestResponseSchema
>;

/**
 * Remote CONTINUATION of an existing conversation (WP1).
 *
 * `send_remote_chat_message` only reaches a conversation a run is already serving; once the
 * agent finished its turn the remote surface used to dead-end on
 * `REMOTE_CHAT_SEND_NOT_STEERABLE`. The idle case now persists a continuation intent through
 * `request_remote_agent_conversation_message` and polls
 * `get_remote_conversation_message_request` to a terminal state — the host owns the send.
 *
 * Status values are the host enum serialized camelCase (`RemoteConversationMessageStatus`).
 */
export const RemoteConversationMessageStatusSchema = z.enum([
  "pending",
  "dispatching",
  "dispatched",
  "failed",
  "cancelled",
  "failedStale",
]);
export type RemoteConversationMessageStatus = z.infer<
  typeof RemoteConversationMessageStatusSchema
>;

/**
 * Terminal states. `dispatched` is the only success; every other terminal state is a VISIBLE
 * failure the composer must surface, because a persisted-but-never-delivered turn shown as a
 * sent message is precisely the hazard this intent table exists to prevent.
 */
const REMOTE_CONVERSATION_MESSAGE_TERMINAL_STATUSES = [
  "dispatched",
  "failed",
  "cancelled",
  "failedStale",
] as const;

/** Response of `request_remote_agent_conversation_message` — the persisted-intent handle. */
const RequestRemoteAgentConversationMessageResponseSchema = z
  .object({
    messageRequestId: z.string(),
    conversationId: z.string(),
    status: RemoteConversationMessageStatusSchema,
    createdAt: z.string(),
  })
  .strict();

/** Response of `get_remote_conversation_message_request` — the post-submit poll target. */
const GetRemoteConversationMessageRequestResponseSchema = z
  .object({
    id: z.string(),
    conversationId: z.string(),
    status: RemoteConversationMessageStatusSchema,
    errorCode: z.string().nullable(),
    agentRunId: z.string().nullable(),
    createdAt: z.string(),
    updatedAt: z.string(),
  })
  .strict();

type RemoteConversationMessageRequest = z.infer<
  typeof GetRemoteConversationMessageRequestResponseSchema
>;

/**
 * Remote conversation MODE SWITCH (WP5a).
 *
 * `switch_agent_conversation_mode` prepares the conversation workspace (`GitService::ref_exists`,
 * `ensure_git_worktree`) and is host-denied by the absolute process floor. Combined with the
 * start intent host-pinning `mode` to `"chat"`, a paired device could reach chat and NOTHING
 * else — Edit, Plan and Ideation were unreachable. The switch now persists an intent through
 * `request_remote_agent_conversation_mode_switch` and polls
 * `get_remote_conversation_mode_switch_request` to a terminal state — the host owns the worktree.
 *
 * Status values are the host enum serialized camelCase
 * (`RemoteConversationModeSwitchStatus`).
 */
export const RemoteConversationModeSwitchStatusSchema = z.enum([
  "pending",
  "switching",
  "switched",
  "alreadyInMode",
  "failed",
  "cancelled",
  "failedStale",
]);
export type RemoteConversationModeSwitchStatus = z.infer<
  typeof RemoteConversationModeSwitchStatusSchema
>;

/**
 * Terminal states. `switched` and `alreadyInMode` are BOTH successes: the second means the
 * conversation was already where the user asked it to be, which is the ordinary outcome of a
 * re-fired picker or a second device. Treating it as a failure would show an error toast for a
 * no-op. Every other terminal state is a VISIBLE failure.
 */
const REMOTE_CONVERSATION_MODE_SWITCH_TERMINAL_STATUSES = [
  "switched",
  "alreadyInMode",
  "failed",
  "cancelled",
  "failedStale",
] as const;

/** The two terminal states the client may render as "the mode is now what you asked for". */
const REMOTE_CONVERSATION_MODE_SWITCH_SUCCESS_STATUSES = [
  "switched",
  "alreadyInMode",
] as const;

/** Response of `request_remote_agent_conversation_mode_switch` — the persisted-intent handle. */
const RequestRemoteAgentConversationModeSwitchResponseSchema = z
  .object({
    modeSwitchRequestId: z.string(),
    conversationId: z.string(),
    status: RemoteConversationModeSwitchStatusSchema,
    deduplicated: z.boolean(),
    createdAt: z.string(),
  })
  .strict();

/** Response of `get_remote_conversation_mode_switch_request` — the post-submit poll target. */
const GetRemoteConversationModeSwitchRequestResponseSchema = z
  .object({
    id: z.string(),
    conversationId: z.string(),
    targetMode: z.string(),
    status: RemoteConversationModeSwitchStatusSchema,
    errorCode: z.string().nullable(),
    createdAt: z.string(),
    updatedAt: z.string(),
  })
  .strict();

type RemoteConversationModeSwitchRequest = z.infer<
  typeof GetRemoteConversationModeSwitchRequestResponseSchema
>;

const ForkAgentConversationResponseSchema = z.object({
  parent_conversation: ChatConversationResponseSchema,
  conversation: ChatConversationResponseSchema,
  workspace: AgentConversationWorkspaceResponseSchema.nullable(),
  provider_session_forked: z.boolean(),
  copied_message_count: z.number(),
  copied_timeline_item_count: z.number(),
});

const SwitchAgentConversationModeResponseSchema = z.object({
  conversation: ChatConversationResponseSchema,
  workspace: AgentConversationWorkspaceResponseSchema.nullable(),
});

const AgentConversationPlanSeedResponseSchema = z.object({
  conversation: ChatConversationResponseSchema,
  workspace: AgentConversationWorkspaceResponseSchema,
  session_id: z.string(),
  artifact: ArtifactResponseSchema,
  blueprint_artifact: ArtifactResponseSchema.nullable().optional().default(null),
});

const PublishAgentConversationWorkspaceResponseSchema = z.object({
  workspace: AgentConversationWorkspaceResponseSchema,
  commit_sha: z.string().nullable(),
  pushed: z.boolean(),
  created_pr: z.boolean(),
  pr_number: z.number().nullable(),
  pr_url: z.string().nullable(),
});
const CommitAgentConversationWorkspaceLocallyResponseSchema = z.object({
  workspace: AgentConversationWorkspaceResponseSchema,
  outcome: z.enum(["committed_local", "already_committed", "no_changes"]),
  branch_name: z.string(),
  previous_head_sha: z.string(),
  commit_sha: z.string(),
  had_changes: z.boolean(),
  attempt_token: z.string(),
});
const PrecomputeAgentConversationWorkspacePrDescriptionResponseSchema =
  z.object({
    conversation_id: z.string(),
    status: z.enum(["ready", "skipped"]),
    cache_status: z.string().nullable(),
    reason: z.string().nullable(),
  });
const UpdateAgentConversationWorkspaceFromBaseResponseSchema = z.object({
  workspace: AgentConversationWorkspaceResponseSchema,
  updated: z.boolean(),
  repair_started: z.boolean().optional().default(false),
  target_ref: z.string(),
  base_commit: z.string(),
  base_status: z
    .enum(["valid", "retargeted", "blocked"])
    .optional()
    .default("valid"),
  effective_base_display_name: z.string().nullable().optional().default(null),
});

type RawAgentConversationWorkspace = z.infer<
  typeof AgentConversationWorkspaceResponseSchema
>;
type RawAgentSidebarConversationGroups = z.infer<
  typeof AgentSidebarConversationGroupsResponseSchema
>;
type RawStartAgentConversationResponse = z.infer<
  typeof StartAgentConversationResponseSchema
>;
type RawForkAgentConversationResponse = z.infer<
  typeof ForkAgentConversationResponseSchema
>;
type RawSwitchAgentConversationModeResponse = z.infer<
  typeof SwitchAgentConversationModeResponseSchema
>;
type RawAgentConversationPlanSeedResponse = z.infer<
  typeof AgentConversationPlanSeedResponseSchema
>;
type RawPublishAgentConversationWorkspaceResponse = z.infer<
  typeof PublishAgentConversationWorkspaceResponseSchema
>;
type RawCommitAgentConversationWorkspaceLocallyResponse = z.infer<
  typeof CommitAgentConversationWorkspaceLocallyResponseSchema
>;
type RawPrecomputeAgentConversationWorkspacePrDescriptionResponse = z.infer<
  typeof PrecomputeAgentConversationWorkspacePrDescriptionResponseSchema
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
type RawAgentWorkspacePrReviewMonitor = z.infer<
  typeof AgentWorkspacePrReviewMonitorResponseSchema
>;
type RawAgentWorkspacePrReviewAction = z.infer<
  typeof AgentWorkspacePrReviewActionResponseSchema
>;
type RawAgentWorkspacePrReviewContext = z.infer<
  typeof AgentWorkspacePrReviewContextResponseSchema
>;
type RawAgentWorkspaceReviewTarget = z.infer<
  typeof AgentWorkspaceReviewTargetResponseSchema
>;
type RawAgentWorkspaceReviewMonitor = z.infer<
  typeof AgentWorkspaceReviewMonitorResponseSchema
>;
type RawAgentWorkspaceReviewContext = z.infer<
  typeof AgentWorkspaceReviewContextResponseSchema
>;
type RawStartAgentWorkspaceReviewResponse = z.infer<
  typeof StartAgentWorkspaceReviewResponseSchema
>;
type RawAgentWorkspaceReviewStartPreviewResponse = z.infer<
  typeof AgentWorkspaceReviewStartPreviewResponseSchema
>;
type RawStartAgentWorkspaceReviewFixerResponse = z.infer<
  typeof StartAgentWorkspaceReviewFixerResponseSchema
>;
type RawSubmitAgentWorkspacePrReviewActionResponse = z.infer<
  typeof SubmitAgentWorkspacePrReviewActionResponseSchema
>;
type RawSkipAgentWorkspacePrReviewActionResponse = z.infer<
  typeof SkipAgentWorkspacePrReviewActionResponseSchema
>;

function transformSendAgentMessageResponse(
  raw: RawSendAgentMessageResponse,
): SendAgentMessageResult {
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
  raw: RawAgentConversationWorkspace,
): AgentConversationWorkspace {
  return {
    conversationId: raw.conversation_id,
    projectId: raw.project_id,
    mode: raw.mode as AgentConversationWorkspaceMode,
    branchMode: raw.branch_mode,
    baseRefKind: raw.base_ref_kind,
    baseRef: raw.base_ref,
    baseDisplayName: raw.base_display_name,
    baseCommit: raw.base_commit,
    branchName: raw.branch_name,
    worktreePath: raw.worktree_path,
    linkedIdeationSessionId: raw.linked_ideation_session_id,
    taskPipelineSessionId: raw.task_pipeline_session_id,
    taskPipelineAvailable: raw.task_pipeline_available,
    linkedPlanBranchId: raw.linked_plan_branch_id,
    sourcePullRequest: raw.source_pull_request
      ? {
          number: raw.source_pull_request.number,
          url: raw.source_pull_request.url,
          title: raw.source_pull_request.title,
          headRefName: raw.source_pull_request.head_ref_name,
          baseRefName: raw.source_pull_request.base_ref_name,
          headRefOid: raw.source_pull_request.head_ref_oid,
        }
      : null,
    modeSwitchLocked: raw.mode_switch_locked,
    modeSwitchLockReason: raw.mode_switch_lock_reason,
    publicationPrNumber: raw.publication_pr_number,
    publicationPrUrl: raw.publication_pr_url,
    publicationPrStatus: raw.publication_pr_status,
    publicationPushStatus: raw.publication_push_status,
    maintenanceOperation: raw.maintenance_operation
      ? {
          operationId: raw.maintenance_operation.operation_id,
          generation: raw.maintenance_operation.generation,
          source: raw.maintenance_operation.source,
          stage: raw.maintenance_operation.stage,
          status: raw.maintenance_operation.status,
          holdReason: raw.maintenance_operation.hold_reason,
          summary: raw.maintenance_operation.summary,
          blocker: raw.maintenance_operation.blocker,
          automaticContinuation: raw.maintenance_operation.automatic_continuation,
          startedAt: raw.maintenance_operation.started_at,
          updatedAt: raw.maintenance_operation.updated_at,
        }
      : null,
    prAutofixFingerprintSpend: raw.pr_autofix_fingerprint_spend
      ? {
          generations: raw.pr_autofix_fingerprint_spend.generations,
          minutes: raw.pr_autofix_fingerprint_spend.minutes,
          budgetMinutes: raw.pr_autofix_fingerprint_spend.budget_minutes,
          isExhausted: raw.pr_autofix_fingerprint_spend.is_exhausted,
        }
      : null,
    publicationMetadataAttemptId: raw.publication_metadata_attempt_id,
    publicationMetadataPhase: raw.publication_metadata_phase,
    publicationMetadataState: raw.publication_metadata_state,
    autoPublishEnabled: raw.auto_publish_enabled,
    autoPublishInitialPrEnabled: raw.auto_publish_initial_pr_enabled,
    autoPublishPausedPrAutofixEnabled:
      raw.auto_publish_paused_pr_autofix_enabled,
    autoPublishPausedPrAutoMergeDesired:
      raw.auto_publish_paused_pr_auto_merge_desired,
    prAutofixEnabled: raw.pr_autofix_enabled,
    prAutoMergeDesired: raw.pr_auto_merge_desired,
    prAutoMergeMethod: raw.pr_auto_merge_method,
    prAutoMergeCurrent: raw.pr_auto_merge_current,
    prSupervisionStatus: raw.pr_supervision_status,
    prSupervisionSummary: raw.pr_supervision_summary,
    prSupervisionUpdatedAt: raw.pr_supervision_updated_at,
    reviewAutomationOverride: raw.review_automation_override,
    status: raw.status,
    createdAt: raw.created_at,
    updatedAt: raw.updated_at,
  };
}

function transformCommitAgentConversationWorkspaceLocallyResponse(
  raw: RawCommitAgentConversationWorkspaceLocallyResponse,
): CommitAgentConversationWorkspaceLocallyResult {
  return {
    workspace: transformAgentConversationWorkspace(raw.workspace),
    outcome: raw.outcome,
    branchName: raw.branch_name,
    previousHeadSha: raw.previous_head_sha,
    commitSha: raw.commit_sha,
    hadChanges: raw.had_changes,
    attemptToken: raw.attempt_token,
  };
}

export function sourcePullRequestInvokeInput(
  sourcePullRequest: AgentConversationSourcePullRequest,
) {
  return {
    number: sourcePullRequest.number,
    url: sourcePullRequest.url ?? null,
    title: sourcePullRequest.title ?? null,
    headRefName: sourcePullRequest.headRefName,
    baseRefName: sourcePullRequest.baseRefName ?? null,
    headRefOid: sourcePullRequest.headRefOid ?? null,
  };
}

function transformAgentSidebarConversationGroups(
  raw: RawAgentSidebarConversationGroups,
): AgentSidebarConversationGroupsResponse {
  return {
    groups: raw.groups.map((group) => ({
      key: group.key,
      label: group.label,
      total: group.total,
      offset: group.offset,
      limit: group.limit,
      hasMore: group.has_more,
      rows: group.rows.map((row) => ({
        conversation: transformConversation(row.conversation),
        workspace: row.workspace
          ? transformAgentConversationWorkspace(row.workspace)
          : null,
        refKind: row.ref_kind === "pull_request" ? "pull-request" : "branch",
        refLabel: row.ref_label,
        publicationState: row.publication_state,
        publicationLabel: row.publication_label,
        attentionLane: row.attention_lane,
        parkedDelegateCount: row.parked_delegate_count,
        actionVerb: row.action_verb,
        isMuted: row.is_muted,
      })),
    })),
  };
}

export function transformStartAgentConversationResponse(
  raw: RawStartAgentConversationResponse,
): StartAgentConversationResult {
  return {
    conversation: transformConversation(raw.conversation),
    workspace: raw.workspace
      ? transformAgentConversationWorkspace(raw.workspace)
      : null,
    sendResult: transformSendAgentMessageResponse(raw.send_result),
  };
}

export function startAgentConversationInvokeInput(
  input: StartAgentConversationInput,
) {
  return {
    ...(input.projectId ? { projectId: input.projectId } : {}),
    content: input.content,
    ...(input.conversationId ? { conversationId: input.conversationId } : {}),
    ...(input.providerHarness
      ? { providerHarness: input.providerHarness }
      : {}),
    ...(input.modelId ? { modelOverride: input.modelId } : {}),
    ...(input.logicalEffort ? { logicalEffort: input.logicalEffort } : {}),
    ...(input.personaId ? { personaId: input.personaId } : {}),
    ...(input.sourcePersonaId ? { sourcePersonaId: input.sourcePersonaId } : {}),
    ...(input.codexFastMode != null
      ? { codexFastMode: input.codexFastMode }
      : {}),
    ...(input.mode ? { mode: input.mode } : {}),
    ...(input.capabilityIntent
      ? { capabilityIntent: input.capabilityIntent }
      : input.teamIntent
        ? { teamIntent: input.teamIntent }
        : {}),
    ...(input.composerProjectReferences?.length
      ? { composerProjectReferences: input.composerProjectReferences }
      : {}),
    ...(input.composerIntegrationReferences?.length
      ? { composerIntegrationReferences: input.composerIntegrationReferences }
      : {}),
    ...(input.composerArtifactReferences?.length
      ? { composerArtifactReferences: input.composerArtifactReferences }
      : {}),
    ...(input.composerSelectionSnapshot
      ? { composerSelectionSnapshot: input.composerSelectionSnapshot }
      : {}),
    ...(input.base
      ? {
          baseRefKind: input.base.kind,
          ...(input.base.branchMode
            ? { baseBranchMode: input.base.branchMode }
            : {}),
          baseRef: input.base.ref,
          baseDisplayName: input.base.displayName,
          ...(input.base.sourcePullRequest
            ? {
                baseSourcePullRequest: sourcePullRequestInvokeInput(
                  input.base.sourcePullRequest,
                ),
              }
            : {}),
        }
      : {}),
  };
}

function transformForkAgentConversationResponse(
  raw: RawForkAgentConversationResponse,
): ForkAgentConversationResult {
  return {
    parentConversation: transformConversation(raw.parent_conversation),
    conversation: transformConversation(raw.conversation),
    workspace: raw.workspace
      ? transformAgentConversationWorkspace(raw.workspace)
      : null,
    providerSessionForked: raw.provider_session_forked,
    copiedMessageCount: raw.copied_message_count,
    copiedTimelineItemCount: raw.copied_timeline_item_count,
  };
}

function transformSwitchAgentConversationModeResponse(
  raw: RawSwitchAgentConversationModeResponse,
): SwitchAgentConversationModeResult {
  return {
    conversation: transformConversation(raw.conversation),
    workspace: raw.workspace
      ? transformAgentConversationWorkspace(raw.workspace)
      : null,
  };
}

function transformAgentConversationPlanSeedResponse(
  raw: RawAgentConversationPlanSeedResponse,
): AgentConversationPlanSeedResult {
  return {
    conversation: transformConversation(raw.conversation),
    workspace: transformAgentConversationWorkspace(raw.workspace),
    sessionId: raw.session_id,
    artifact: transformArtifactResponse(raw.artifact),
    blueprintArtifact: raw.blueprint_artifact
      ? transformArtifactResponse(raw.blueprint_artifact)
      : null,
  };
}

function transformPublishAgentConversationWorkspaceResponse(
  raw: RawPublishAgentConversationWorkspaceResponse,
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

function transformPrecomputeAgentConversationWorkspacePrDescriptionResponse(
  raw: RawPrecomputeAgentConversationWorkspacePrDescriptionResponse,
): PrecomputeAgentConversationWorkspacePrDescriptionResult {
  return {
    conversationId: raw.conversation_id,
    status: raw.status,
    cacheStatus: raw.cache_status,
    reason: raw.reason,
  };
}

function transformAgentConversationWorkspacePublicationEvent(
  raw: RawAgentConversationWorkspacePublicationEvent,
): AgentConversationWorkspacePublicationEvent {
  return {
    id: raw.id,
    conversationId: raw.conversation_id,
    step: raw.step,
    status: raw.status,
    summary: raw.summary,
    classification: raw.classification,
    attemptId: raw.attempt_id,
    createdAt: raw.created_at,
  };
}

function transformAgentConversationWorkspaceFreshness(
  raw: RawAgentConversationWorkspaceFreshness,
): AgentConversationWorkspaceFreshness {
  return {
    conversationId: raw.conversation_id,
    freshnessScope: raw.freshness_scope,
    baseRef: raw.base_ref,
    baseDisplayName: raw.base_display_name,
    targetRef: raw.target_ref,
    capturedBaseCommit: raw.captured_base_commit,
    targetBaseCommit: raw.target_base_commit,
    isBaseAhead: raw.is_base_ahead,
    hasUncommittedChanges: raw.has_uncommitted_changes,
    unpublishedCommitCount: raw.unpublished_commit_count,
    remoteRefreshed: raw.remote_refreshed,
    worktreeStatusChecked: raw.worktree_status_checked,
    baseStatus: raw.base_status,
    effectiveBaseRef: raw.effective_base_ref,
    effectiveBaseDisplayName: raw.effective_base_display_name,
    baseBlockReason: raw.base_block_reason,
    recommendedActions: raw.recommended_actions,
  };
}

function transformAgentWorkspacePrReviewMonitor(
  raw: RawAgentWorkspacePrReviewMonitor,
): AgentWorkspacePrReviewMonitor {
  return {
    conversationId: raw.conversation_id,
    projectId: raw.project_id,
    prNumber: raw.pr_number,
    status: raw.status,
    monitorEnabled: raw.monitor_enabled,
    autoApproveEnabled: raw.auto_approve_enabled,
    firstReviewCompleted: raw.first_review_completed,
    firstActionResolved: raw.first_action_resolved,
    lastSeenHeadSha: raw.last_seen_head_sha,
    lastReviewedHeadSha: raw.last_reviewed_head_sha,
    lastReviewRunId: raw.last_review_run_id,
    lastReviewOutcome: raw.last_review_outcome,
    lastSubmittedReviewId: raw.last_submitted_review_id,
    reviewArtifactId: raw.review_artifact_id,
    reviewArtifactHeadSha: raw.review_artifact_head_sha,
    reviewArtifactVersion: raw.review_artifact_version,
    reviewArtifactUpdatedAt: raw.review_artifact_updated_at,
    lastError: raw.last_error,
    createdAt: raw.created_at,
    updatedAt: raw.updated_at,
  };
}

function transformAgentWorkspacePrReviewAction(
  raw: RawAgentWorkspacePrReviewAction,
): AgentWorkspacePrReviewAction {
  return {
    id: raw.id,
    conversationId: raw.conversation_id,
    prNumber: raw.pr_number,
    headSha: raw.head_sha,
    proposedAction: raw.proposed_action,
    summary: raw.summary,
    reviewBody: raw.review_body,
    findingsJson: raw.findings_json,
    status: raw.status,
    submittedReviewId: raw.submitted_review_id,
    createdByRunId: raw.created_by_run_id,
    createdAt: raw.created_at,
    updatedAt: raw.updated_at,
    resolvedAt: raw.resolved_at,
  };
}

function transformAgentWorkspacePrReviewContext(
  raw: RawAgentWorkspacePrReviewContext,
): AgentWorkspacePrReviewContext {
  return {
    success: raw.success,
    workspace: transformAgentConversationWorkspace(raw.workspace),
    events: raw.events.map(transformAgentConversationWorkspacePublicationEvent),
    prNumber: raw.pr_number,
    prUrl: raw.pr_url,
    currentHeadSha: raw.current_head_sha,
    pendingActionHeadStatus: raw.pending_action_head_status,
    health: raw.health,
    reviewFeedback: raw.review_feedback,
    monitor: raw.monitor
      ? transformAgentWorkspacePrReviewMonitor(raw.monitor)
      : null,
    pendingAction: raw.pending_action
      ? transformAgentWorkspacePrReviewAction(raw.pending_action)
      : null,
    recentActions: raw.recent_actions.map(
      transformAgentWorkspacePrReviewAction,
    ),
    issueCommentEvidence: raw.issue_comment_evidence,
  };
}

function transformAgentWorkspaceReviewTarget(
  raw: RawAgentWorkspaceReviewTarget,
): AgentWorkspaceReviewTarget {
  return {
    scope: raw.scope,
    baseRef: raw.base_ref,
    baseSha: raw.base_sha,
    headRef: raw.head_ref,
    headSha: raw.head_sha,
    diffFingerprint: raw.diff_fingerprint,
    sourcePullRequestNumber: raw.source_pull_request_number,
  };
}

function transformAgentWorkspaceReviewMonitor(
  raw: RawAgentWorkspaceReviewMonitor,
): AgentWorkspaceReviewMonitor {
  return {
    conversationId: raw.conversation_id,
    projectId: raw.project_id,
    status: raw.status,
    reviewOutcome: raw.review_outcome,
    reviewGateStatus: raw.review_gate_status,
    currentTargetScope: raw.current_target_scope,
    reviewedTargetScope: raw.reviewed_target_scope,
    reviewConversationId: raw.review_conversation_id ?? null,
    reviewArtifactId: raw.review_artifact_id,
    reviewArtifactVersion: raw.review_artifact_version,
    reviewArtifactUpdatedAt: raw.review_artifact_updated_at,
    reviewRequestedChangesArtifactId:
      raw.review_requested_changes_artifact_id,
    reviewRequestedChangesArtifactVersion:
      raw.review_requested_changes_artifact_version,
    reviewRequestedChangesArtifactUpdatedAt:
      raw.review_requested_changes_artifact_updated_at,
    reviewGateBypassedAt: raw.review_gate_bypassed_at,
    reviewGateBypassedTargetScope: raw.review_gate_bypassed_target_scope,
    reviewGateBypassedDiffFingerprint:
      raw.review_gate_bypassed_diff_fingerprint,
    reviewGateBypassedArtifactId: raw.review_gate_bypassed_artifact_id,
    reviewGateBypassedArtifactVersion: raw.review_gate_bypassed_artifact_version,
    reviewedHeadSha: raw.reviewed_head_sha,
    reviewedDiffFingerprint: raw.reviewed_diff_fingerprint,
    selectedSourceBaseRef: raw.selected_source_base_ref,
    selectedSourceBaseSha: raw.selected_source_base_sha,
    selectedSourceHeadRef: raw.selected_source_head_ref,
    selectedSourceHeadSha: raw.selected_source_head_sha,
    selectedSourcePullRequestNumber: raw.selected_source_pull_request_number,
    workspaceBaseRef: raw.workspace_base_ref,
    workspaceBaseSha: raw.workspace_base_sha,
    workspaceHeadRef: raw.workspace_head_ref,
    workspaceHeadSha: raw.workspace_head_sha,
    currentDiffFingerprint: raw.current_diff_fingerprint,
    previousVersionId: raw.previous_version_id,
    reviewRequestedChangesPreviousVersionId:
      raw.review_requested_changes_previous_version_id,
    reviewBlockingSummary: raw.review_blocking_summary,
    reviewBlockingFingerprint: raw.review_blocking_fingerprint,
    reviewFixerRunId: raw.review_fixer_run_id,
    reviewFixerConversationId: raw.review_fixer_conversation_id,
    reviewFixerStatus: raw.review_fixer_status,
    reviewFixerCycleCount: raw.review_fixer_cycle_count,
    lastRunId: raw.last_run_id,
    lastError: raw.last_error,
    autoMergeGuardStatus: raw.auto_merge_guard_status,
    autoMergeGuardPrNumber: raw.auto_merge_guard_pr_number,
    autoMergeGuardMethod: raw.auto_merge_guard_method,
    autoMergeGuardTargetScope: raw.auto_merge_guard_target_scope,
    autoMergeGuardDiffFingerprint: raw.auto_merge_guard_diff_fingerprint,
    autoMergeGuardHeadSha: raw.auto_merge_guard_head_sha,
    autoMergeGuardLastError: raw.auto_merge_guard_last_error,
    createdAt: raw.created_at,
    updatedAt: raw.updated_at,
  };
}

function transformAgentWorkspaceReviewContext(
  raw: RawAgentWorkspaceReviewContext,
): AgentWorkspaceReviewContext {
  return {
    success: raw.success,
    workspace: transformAgentConversationWorkspace(raw.workspace),
    events: raw.events.map(transformAgentConversationWorkspacePublicationEvent),
    target: raw.target ? transformAgentWorkspaceReviewTarget(raw.target) : null,
    monitor: transformAgentWorkspaceReviewMonitor(raw.monitor),
    reviewArtifactIsCurrent: raw.review_artifact_is_current ?? raw.is_current,
    reviewArtifactIsOutdated:
      raw.review_artifact_is_outdated ?? raw.is_outdated,
    canMutateReviewState: raw.can_mutate_review_state,
    reviewRuntimeState: raw.review_runtime_state,
    isCurrent: raw.is_current,
    isOutdated: raw.is_outdated,
    shouldShowTab: raw.should_show_tab,
  };
}

function transformStartAgentWorkspaceReviewResponse(
  raw: RawStartAgentWorkspaceReviewResponse,
): StartAgentWorkspaceReviewResult {
  return {
    success: raw.success,
    target: raw.target ? transformAgentWorkspaceReviewTarget(raw.target) : null,
    monitor: transformAgentWorkspaceReviewMonitor(raw.monitor),
    reviewArtifactIsCurrent: raw.review_artifact_is_current ?? raw.is_current,
    reviewArtifactIsOutdated:
      raw.review_artifact_is_outdated ?? raw.is_outdated,
    canMutateReviewState: raw.can_mutate_review_state,
    reviewRuntimeState: raw.review_runtime_state,
    isCurrent: raw.is_current,
    isOutdated: raw.is_outdated,
    shouldShowTab: raw.should_show_tab,
    started: raw.started,
    skippedReason: raw.skipped_reason,
    wasQueued: raw.was_queued,
  };
}

function transformAgentWorkspaceReviewStartPreview(
  raw: RawAgentWorkspaceReviewStartPreviewResponse,
): AgentWorkspaceReviewStartPreview {
  return {
    success: raw.success,
    target: raw.target ? transformAgentWorkspaceReviewTarget(raw.target) : null,
    willDisableAutoMerge: raw.will_disable_auto_merge,
    prNumber: raw.pr_number,
    mergeMethod: raw.merge_method,
    restoreAfterPublish: raw.restore_after_publish,
    confirmation: {
      targetScope: raw.confirmation.target_scope,
      diffFingerprint: raw.confirmation.diff_fingerprint,
      headSha: raw.confirmation.head_sha,
      prNumber: raw.confirmation.pr_number,
      willDisableAutoMerge: raw.confirmation.will_disable_auto_merge,
      mergeMethod: raw.confirmation.merge_method,
      restoreAfterPublish: raw.confirmation.restore_after_publish,
    },
  };
}

function transformStartAgentWorkspaceReviewFixerResponse(
  raw: RawStartAgentWorkspaceReviewFixerResponse,
): StartAgentWorkspaceReviewFixerResult {
  return {
    success: raw.success,
    target: raw.target ? transformAgentWorkspaceReviewTarget(raw.target) : null,
    monitor: transformAgentWorkspaceReviewMonitor(raw.monitor),
    reviewArtifactIsCurrent: raw.review_artifact_is_current ?? raw.is_current,
    reviewArtifactIsOutdated:
      raw.review_artifact_is_outdated ?? raw.is_outdated,
    canMutateReviewState: raw.can_mutate_review_state,
    reviewRuntimeState: raw.review_runtime_state,
    isCurrent: raw.is_current,
    isOutdated: raw.is_outdated,
    shouldShowTab: raw.should_show_tab,
    started: raw.started,
    skippedReason: raw.skipped_reason,
  };
}

function transformSubmitAgentWorkspacePrReviewActionResponse(
  raw: RawSubmitAgentWorkspacePrReviewActionResponse,
): SubmitAgentWorkspacePrReviewActionResult {
  return {
    success: raw.success,
    monitor: transformAgentWorkspacePrReviewMonitor(raw.monitor),
    action: transformAgentWorkspacePrReviewAction(raw.action),
    submittedReviewId: raw.submitted_review_id,
    submittedReviewUrl: raw.submitted_review_url,
  };
}

function transformSkipAgentWorkspacePrReviewActionResponse(
  raw: RawSkipAgentWorkspacePrReviewActionResponse,
): SkipAgentWorkspacePrReviewActionResult {
  return {
    success: raw.success,
    monitor: transformAgentWorkspacePrReviewMonitor(raw.monitor),
    action: transformAgentWorkspacePrReviewAction(raw.action),
  };
}

function transformUpdateAgentConversationWorkspaceFromBaseResponse(
  raw: RawUpdateAgentConversationWorkspaceFromBaseResponse,
): UpdateAgentConversationWorkspaceFromBaseResult {
  return {
    workspace: transformAgentConversationWorkspace(raw.workspace),
    updated: raw.updated,
    repairStarted: raw.repair_started,
    targetRef: raw.target_ref,
    baseCommit: raw.base_commit,
    baseStatus: raw.base_status,
    effectiveBaseDisplayName: raw.effective_base_display_name,
  };
}

export async function getAgentConversationWorkspace(
  conversationId: string,
): Promise<AgentConversationWorkspace | null> {
  const args = { conversationId };
  const remote = remoteTranscriptReadsEnabled();
  const raw = remote
    ? await typedInvoke(
        "get_remote_agent_conversation_workspace",
        args,
        AgentConversationWorkspaceResponseSchema.nullable(),
      )
    : await typedInvoke(
        "get_agent_conversation_workspace",
        args,
        AgentConversationWorkspaceResponseSchema.nullable(),
      );
  if (remote && !raw) {
    throw new Error(`Agent workspace ${conversationId} was not found on this host.`);
  }
  return raw ? transformAgentConversationWorkspace(raw) : null;
}

export async function listWorkspaceOpenTargets(): Promise<
  WorkspaceOpenTarget[]
> {
  return typedInvoke(
    "list_workspace_open_targets",
    {},
    z.array(WorkspaceOpenTargetResponseSchema),
  );
}

export async function openAgentConversationWorkspace(
  conversationId: string,
  targetId: string,
): Promise<void> {
  await typedInvoke(
    "open_agent_conversation_workspace",
    { conversationId, targetId },
    z.null(),
  );
}

export async function openAgentConversationWorkspacePath(
  conversationId: string,
  targetId: string,
  path: string,
): Promise<void> {
  await typedInvoke(
    "open_agent_conversation_workspace_path",
    { conversationId, targetId, path },
    z.null(),
  );
}

export async function listAgentConversationWorkspacesByProject(
  projectId: string,
): Promise<AgentConversationWorkspace[]> {
  const raw = await typedInvoke(
    "list_agent_conversation_workspaces_by_project",
    { projectId },
    AgentConversationWorkspaceListResponseSchema,
  );
  return raw.map(transformAgentConversationWorkspace);
}

export async function listAgentSidebarConversations(
  input: AgentSidebarConversationsInput,
): Promise<AgentSidebarConversationGroupsResponse> {
  const normalizedSearch = input.search?.trim();
  // Pins and priorities are client-local, env-scoped UI state (agentSessionStore). Sending a
  // remote viewer's own pins to the host reorders the host's conversations away from the order the
  // host itself renders — same data, different within-group boosting. On a remote environment we
  // omit them so the host's native sort/grouping is preserved verbatim. (Mirroring the host's
  // pinned-to-top items would require host-persisted pin state; that is a separate follow-up.)
  const isRemote = remoteTranscriptReadsEnabled();
  const args = {
    input: {
      projectIds: input.projectIds,
      includeArchived: input.includeArchived ?? false,
      archivedOnly: input.archivedOnly ?? false,
      ...(normalizedSearch ? { search: normalizedSearch } : {}),
      ...(input.publicationStates
        ? { publicationStates: input.publicationStates }
        : {}),
      ...(input.groupBy ? { groupBy: input.groupBy } : {}),
      ...(input.sort ? { sort: input.sort } : {}),
      ...(input.limitPerGroup != null
        ? { limitPerGroup: input.limitPerGroup }
        : {}),
      ...(input.offsets ? { offsets: input.offsets } : {}),
      ...(!isRemote && input.pinnedConversationIds
        ? { pinnedConversationIds: input.pinnedConversationIds }
        : {}),
      ...(!isRemote && input.priorityConversationIds
        ? { priorityConversationIds: input.priorityConversationIds }
        : {}),
    },
  };
  // Two literal command names, not a computed one: the local
  // `list_agent_sidebar_conversations` is unregistered on the facade (it schedules PR-supervision
  // recovery), so a paired device must call the spawn-free, worktree_path-blanking twin
  // `list_remote_agent_sidebar_conversations`. The projected payload differs only in that
  // `worktree_path` is blanked, which the sidebar UI does not render, so the schema and transform
  // below are shared verbatim. Duplicating the invoke keeps every command name statically
  // enumerable for the P-11 transport-drift scan (see `remoteTranscriptReadsEnabled`).
  const raw = isRemote
    ? await typedInvoke(
        "list_remote_agent_sidebar_conversations",
        args,
        AgentSidebarConversationGroupsResponseSchema,
      )
    : await typedInvoke(
        "list_agent_sidebar_conversations",
        args,
        AgentSidebarConversationGroupsResponseSchema,
      );
  return transformAgentSidebarConversationGroups(raw);
}

export async function listAgentConversationWorkspacePublicationEvents(
  conversationId: string,
): Promise<AgentConversationWorkspacePublicationEvent[]> {
  const raw = await typedInvoke(
    "list_agent_conversation_workspace_publication_events",
    { conversationId },
    AgentConversationWorkspacePublicationEventListResponseSchema,
  );
  return raw.map(transformAgentConversationWorkspacePublicationEvent);
}

export class AgentWorkspaceHttpError extends Error {
  readonly status: number;
  readonly detail: string | null;

  constructor(status: number, statusText: string, detail: string | null) {
    super(
      detail ? `${status} ${statusText}: ${detail}` : `${status} ${statusText}`,
    );
    this.name = "AgentWorkspaceHttpError";
    this.status = status;
    this.detail = detail;
  }
}

async function fetchAgentWorkspaceJson<T>(
  path: string,
  schema: z.ZodType<T>,
  init?: RequestInit,
): Promise<T> {
  const response = await backendFetch(path, init);
  if (!response.ok) {
    let detail: string | null = null;
    let envelopeCode: string | null = null;
    try {
      const raw = (await response.json()) as {
        error?: string;
        message?: string;
        detail?: string;
        code?: string;
      };
      detail = raw.detail ?? raw.message ?? raw.error ?? null;
      envelopeCode = typeof raw.code === "string" ? raw.code : null;
    } catch {
      detail = null;
    }
    // A remote host answers routes it does not mount with the typed
    // `REMOTE_COMMAND_UNAVAILABLE` envelope (`remote_server/mod.rs`). Surface it as
    // the typed transport error so the hydration barrier and the remote gates read
    // it as a capability boundary — flattening it into the generic HTTP error made
    // the barrier treat "this host doesn't expose that route" as host unhealth and
    // loop "Reconnecting…" forever.
    const environmentId = getTransportEnvironmentId();
    if (
      isRemoteEnvironmentId(environmentId) &&
      envelopeCode === "REMOTE_COMMAND_UNAVAILABLE"
    ) {
      throw new RemoteTransportError({
        code: "REMOTE_COMMAND_UNAVAILABLE",
        message: detail ?? "This remote route is not available.",
        environmentId,
        cmd: path,
      });
    }
    throw new AgentWorkspaceHttpError(
      response.status,
      response.statusText,
      detail,
    );
  }
  return schema.parse(await response.json());
}

export async function getAgentWorkspacePrReviewContext(
  conversationId: string,
): Promise<AgentWorkspacePrReviewContext> {
  const raw = await fetchAgentWorkspaceJson(
    `agent-workspaces/${encodeURIComponent(conversationId)}/pr-review-context`,
    AgentWorkspacePrReviewContextResponseSchema,
  );
  return transformAgentWorkspacePrReviewContext(raw);
}

export async function getAgentWorkspaceReviewContext(
  conversationId: string,
  options: {
    signal?: AbortSignal;
    refreshTarget?: boolean;
  } = {},
): Promise<AgentWorkspaceReviewContext> {
  const query = options.refreshTarget ? "?refresh_target=true" : "";
  const raw = await fetchAgentWorkspaceJson(
    `agent-workspaces/${encodeURIComponent(conversationId)}/workspace-review-context${query}`,
    AgentWorkspaceReviewContextResponseSchema,
    options.signal ? { signal: options.signal } : undefined,
  );
  return transformAgentWorkspaceReviewContext(raw);
}

export async function getAgentWorkspaceReviewStartPreview(
  conversationId: string,
): Promise<AgentWorkspaceReviewStartPreview> {
  const raw = await fetchAgentWorkspaceJson(
    `agent-workspaces/${encodeURIComponent(conversationId)}/workspace-review-start-preview`,
    AgentWorkspaceReviewStartPreviewResponseSchema,
  );
  return transformAgentWorkspaceReviewStartPreview(raw);
}

export async function startAgentWorkspaceReview(
  conversationId: string,
  options: {
    force?: boolean;
    confirmation?: AgentWorkspaceReviewStartConfirmation;
    runtimeOverride?: ManualRoleRuntimeSelection;
    enableReviewAutomation?: boolean;
  } = {},
): Promise<StartAgentWorkspaceReviewResult> {
  const raw = await fetchAgentWorkspaceJson(
    `agent-workspaces/${encodeURIComponent(conversationId)}/workspace-review-runs`,
    StartAgentWorkspaceReviewResponseSchema,
    {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        force: options.force ?? false,
        ...(options.enableReviewAutomation !== undefined
          ? { enable_review_automation: options.enableReviewAutomation }
          : {}),
        confirmation: options.confirmation
          ? {
              target_scope: options.confirmation.targetScope,
              diff_fingerprint: options.confirmation.diffFingerprint,
              head_sha: options.confirmation.headSha,
              pr_number: options.confirmation.prNumber,
              will_disable_auto_merge:
                options.confirmation.willDisableAutoMerge,
              merge_method: options.confirmation.mergeMethod,
              restore_after_publish:
                options.confirmation.restoreAfterPublish,
            }
          : undefined,
        runtime_override: options.runtimeOverride
          ? roleRuntimeOverrideHttpInput(options.runtimeOverride)
          : undefined,
      }),
    },
  );
  return transformStartAgentWorkspaceReviewResponse(raw);
}

export async function startAgentWorkspaceReviewFixer(
  conversationId: string,
  input: {
    confirmation: AgentWorkspaceReviewFixerConfirmation;
    runtimeOverride?: ManualRoleRuntimeSelection;
  },
): Promise<StartAgentWorkspaceReviewFixerResult> {
  const raw = await fetchAgentWorkspaceJson(
    `agent-workspaces/${encodeURIComponent(conversationId)}/workspace-review-fixer-runs`,
    StartAgentWorkspaceReviewFixerResponseSchema,
    {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        confirmation: {
          target_scope: input.confirmation.targetScope,
          diff_fingerprint: input.confirmation.diffFingerprint,
          artifact_id: input.confirmation.artifactId,
          artifact_version: input.confirmation.artifactVersion,
          blocking_fingerprint: input.confirmation.blockingFingerprint,
        },
        runtime_override: input.runtimeOverride
          ? roleRuntimeOverrideHttpInput(input.runtimeOverride)
          : undefined,
      }),
    },
  );
  return transformStartAgentWorkspaceReviewFixerResponse(raw);
}

export async function approveAgentWorkspaceReviewAnyway(
  conversationId: string,
  input: ApproveAgentWorkspaceReviewAnywayInput,
): Promise<ApproveAgentWorkspaceReviewAnywayResult> {
  const raw = await fetchAgentWorkspaceJson(
    `agent-workspaces/${encodeURIComponent(conversationId)}/workspace-review-approve-anyway`,
    ApproveAgentWorkspaceReviewAnywayResponseSchema,
    {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        target_scope: input.targetScope,
        diff_fingerprint: input.diffFingerprint,
        artifact_id: input.artifactId,
        artifact_version: input.artifactVersion,
      }),
    },
  );
  return {
    success: raw.success,
    monitor: transformAgentWorkspaceReviewMonitor(raw.monitor),
  };
}

const AgentConversationIssueOccurrenceResponseSchema = z.object({
  id: z.string(),
  issue_id: z.string(),
  source_task_id: z.string().nullable(),
  source_context_type: z.string().nullable(),
  source_context_id: z.string().nullable(),
  source_agent_name: z.string().nullable(),
  issue_kind: z.string(),
  severity: z.string(),
  blocking_scope: z.string(),
  title: z.string(),
  summary: z.string(),
  evidence: z.string().nullable(),
  recommendation: z.string().nullable(),
  raw_blocker_fingerprint: z.string().nullable(),
  canonical_fingerprint: z.string().nullable(),
  dedupe_decision: z.string().nullable(),
  created_at: z.string(),
});

const AgentConversationIssueResponseSchema = z.object({
  id: z.string(),
  project_id: z.string(),
  conversation_id: z.string(),
  source_task_id: z.string().nullable(),
  source_context_type: z.string().nullable(),
  source_context_id: z.string().nullable(),
  source_agent_name: z.string().nullable(),
  issue_kind: z.string(),
  severity: z.string(),
  status: z.string(),
  blocking_scope: z.string(),
  title: z.string(),
  summary: z.string(),
  evidence: z.string().nullable(),
  recommendation: z.string().nullable(),
  blocker_fingerprint: z.string().nullable(),
  canonical_fingerprint: z.string().nullable().optional().default(null),
  canonical_scope_kind: z.string().nullable().optional().default(null),
  canonical_scope_subject: z.string().nullable().optional().default(null),
  canonical_family: z.string().nullable().optional().default(null),
  superseded_by_issue_id: z.string().nullable().optional().default(null),
  occurrence_count: z.number().nullable().optional().default(null),
  occurrences: z
    .array(AgentConversationIssueOccurrenceResponseSchema)
    .optional()
    .default([]),
  followup_title: z.string().nullable(),
  followup_prompt: z.string().nullable(),
  auto_followup_eligible: z.boolean(),
  linked_followup_conversation_id: z.string().nullable(),
  created_at: z.string(),
  updated_at: z.string(),
  resolved_at: z.string().nullable(),
});

const AgentConversationIssueListResponseSchema = z.object({
  issues: z.array(AgentConversationIssueResponseSchema),
});

const AgentConversationIssueMutationResponseSchema = z.object({
  issue: AgentConversationIssueResponseSchema,
});

type RawAgentConversationIssue = z.infer<
  typeof AgentConversationIssueResponseSchema
>;
type RawAgentConversationIssueOccurrence = z.infer<
  typeof AgentConversationIssueOccurrenceResponseSchema
>;

export interface AgentConversationIssueOccurrence {
  id: string;
  issueId: string;
  sourceTaskId: string | null;
  sourceContextType: string | null;
  sourceContextId: string | null;
  sourceAgentName: string | null;
  issueKind: string;
  severity: string;
  blockingScope: string;
  title: string;
  summary: string;
  evidence: string | null;
  recommendation: string | null;
  rawBlockerFingerprint: string | null;
  canonicalFingerprint: string | null;
  dedupeDecision: string | null;
  createdAt: string;
}

export interface AgentConversationIssue {
  id: string;
  projectId: string;
  conversationId: string;
  sourceTaskId: string | null;
  sourceContextType: string | null;
  sourceContextId: string | null;
  sourceAgentName: string | null;
  issueKind: string;
  severity: string;
  status: string;
  blockingScope: string;
  title: string;
  summary: string;
  evidence: string | null;
  recommendation: string | null;
  blockerFingerprint: string | null;
  canonicalFingerprint: string | null;
  canonicalScopeKind: string | null;
  canonicalScopeSubject: string | null;
  canonicalFamily: string | null;
  supersededByIssueId: string | null;
  occurrenceCount: number | null;
  occurrences: AgentConversationIssueOccurrence[];
  followupTitle: string | null;
  followupPrompt: string | null;
  autoFollowupEligible: boolean;
  linkedFollowupConversationId: string | null;
  createdAt: string;
  updatedAt: string;
  resolvedAt: string | null;
}

function transformAgentConversationIssueOccurrence(
  raw: RawAgentConversationIssueOccurrence,
): AgentConversationIssueOccurrence {
  return {
    id: raw.id,
    issueId: raw.issue_id,
    sourceTaskId: raw.source_task_id,
    sourceContextType: raw.source_context_type,
    sourceContextId: raw.source_context_id,
    sourceAgentName: raw.source_agent_name,
    issueKind: raw.issue_kind,
    severity: raw.severity,
    blockingScope: raw.blocking_scope,
    title: raw.title,
    summary: raw.summary,
    evidence: raw.evidence,
    recommendation: raw.recommendation,
    rawBlockerFingerprint: raw.raw_blocker_fingerprint,
    canonicalFingerprint: raw.canonical_fingerprint,
    dedupeDecision: raw.dedupe_decision,
    createdAt: raw.created_at,
  };
}

function transformAgentConversationIssue(
  raw: RawAgentConversationIssue,
): AgentConversationIssue {
  return {
    id: raw.id,
    projectId: raw.project_id,
    conversationId: raw.conversation_id,
    sourceTaskId: raw.source_task_id,
    sourceContextType: raw.source_context_type,
    sourceContextId: raw.source_context_id,
    sourceAgentName: raw.source_agent_name,
    issueKind: raw.issue_kind,
    severity: raw.severity,
    status: raw.status,
    blockingScope: raw.blocking_scope,
    title: raw.title,
    summary: raw.summary,
    evidence: raw.evidence,
    recommendation: raw.recommendation,
    blockerFingerprint: raw.blocker_fingerprint,
    canonicalFingerprint: raw.canonical_fingerprint,
    canonicalScopeKind: raw.canonical_scope_kind,
    canonicalScopeSubject: raw.canonical_scope_subject,
    canonicalFamily: raw.canonical_family,
    supersededByIssueId: raw.superseded_by_issue_id,
    occurrenceCount: raw.occurrence_count,
    occurrences: raw.occurrences.map(transformAgentConversationIssueOccurrence),
    followupTitle: raw.followup_title,
    followupPrompt: raw.followup_prompt,
    autoFollowupEligible: raw.auto_followup_eligible,
    linkedFollowupConversationId: raw.linked_followup_conversation_id,
    createdAt: raw.created_at,
    updatedAt: raw.updated_at,
    resolvedAt: raw.resolved_at,
  };
}

export async function listAgentConversationIssues(
  conversationId: string,
  options: { includeResolved?: boolean } = {},
): Promise<AgentConversationIssue[]> {
  const raw = await fetchAgentWorkspaceJson(
    "agent_conversation_issues/list",
    AgentConversationIssueListResponseSchema,
    {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        conversation_id: conversationId,
        include_resolved: options.includeResolved ?? false,
      }),
    },
  );
  return raw.issues.map(transformAgentConversationIssue);
}

export async function updateAgentConversationIssueStatus(
  issueId: string,
  status: "open" | "resolved" | "dismissed",
): Promise<AgentConversationIssue> {
  const raw = await fetchAgentWorkspaceJson(
    "agent_conversation_issues/status",
    AgentConversationIssueMutationResponseSchema,
    {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ issue_id: issueId, status }),
    },
  );
  return transformAgentConversationIssue(raw.issue);
}

export async function convertAgentConversationIssueFollowup(
  issueId: string,
): Promise<AgentConversationIssue> {
  const raw = await fetchAgentWorkspaceJson(
    "agent_conversation_issues/convert_followup",
    AgentConversationIssueMutationResponseSchema.extend({
      followup: z.unknown(),
    }),
    {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ issue_id: issueId }),
    },
  );
  return transformAgentConversationIssue(raw.issue);
}

export async function submitAgentWorkspacePrReviewAction(
  conversationId: string,
  actionId: string,
  actionKind?: AgentWorkspacePrReviewActionKind | null,
): Promise<SubmitAgentWorkspacePrReviewActionResult> {
  const raw = await fetchAgentWorkspaceJson(
    `agent-workspaces/${encodeURIComponent(conversationId)}/pr-review-actions/${encodeURIComponent(actionId)}/submit`,
    SubmitAgentWorkspacePrReviewActionResponseSchema,
    {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ action_kind: actionKind ?? null }),
    },
  );
  return transformSubmitAgentWorkspacePrReviewActionResponse(raw);
}

export async function skipAgentWorkspacePrReviewAction(
  conversationId: string,
  actionId: string,
  reason?: string | null,
): Promise<SkipAgentWorkspacePrReviewActionResult> {
  const raw = await fetchAgentWorkspaceJson(
    `agent-workspaces/${encodeURIComponent(conversationId)}/pr-review-actions/${encodeURIComponent(actionId)}/skip`,
    SkipAgentWorkspacePrReviewActionResponseSchema,
    {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ reason: reason ?? null }),
    },
  );
  return transformSkipAgentWorkspacePrReviewActionResponse(raw);
}

export async function setAgentWorkspacePrReviewAutoApprove(
  conversationId: string,
  autoApproveEnabled: boolean,
): Promise<SetAgentWorkspacePrReviewAutoApproveResult> {
  const raw = await fetchAgentWorkspaceJson(
    `agent-workspaces/${encodeURIComponent(conversationId)}/pr-review-settings`,
    SetAgentWorkspacePrReviewAutoApproveResponseSchema,
    {
      method: "PUT",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({ auto_approve_enabled: autoApproveEnabled }),
    },
  );
  return {
    success: raw.success,
    monitor: transformAgentWorkspacePrReviewMonitor(raw.monitor),
  };
}

export async function setAgentWorkspacePrReviewMonitoring(
  conversationId: string,
  monitorEnabled: boolean,
  activeReviewPolicy?: "finish_current" | "cancel_current",
): Promise<SetAgentWorkspacePrReviewAutoApproveResult> {
  const raw = await fetchAgentWorkspaceJson(
    `agent-workspaces/${encodeURIComponent(conversationId)}/pr-review-settings`,
    SetAgentWorkspacePrReviewAutoApproveResponseSchema,
    {
      method: "PUT",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        monitor_enabled: monitorEnabled,
        ...(activeReviewPolicy
          ? { active_review_policy: activeReviewPolicy }
          : {}),
      }),
    },
  );
  return {
    success: raw.success,
    monitor: transformAgentWorkspacePrReviewMonitor(raw.monitor),
  };
}

export async function getAgentConversationWorkspaceFreshness(
  conversationId: string,
  options: { scope?: AgentConversationWorkspaceFreshnessScope } = {},
): Promise<AgentConversationWorkspaceFreshness> {
  const raw = await typedInvoke(
    "get_agent_conversation_workspace_freshness",
    {
      conversationId,
      ...(options.scope ? { freshnessScope: options.scope } : {}),
    },
    AgentConversationWorkspaceFreshnessResponseSchema,
  );
  return transformAgentConversationWorkspaceFreshness(raw);
}

export async function reconcileAgentConversationWorkspacePublication(
  conversationId: string,
): Promise<void> {
  await typedInvoke(
    "reconcile_agent_conversation_workspace_publication",
    { conversationId },
    z.void(),
  );
}

export async function updateAgentConversationWorkspaceFromBase(
  conversationId: string,
  base?: AgentConversationBaseSelection | null,
): Promise<UpdateAgentConversationWorkspaceFromBaseResult> {
  const raw = await typedInvoke(
    "update_agent_conversation_workspace_from_base",
    {
      conversationId,
      ...(base
        ? {
            baseRefKind: base.kind,
            baseRef: base.ref,
            baseDisplayName: base.displayName,
            ...(base.sourcePullRequest
              ? {
                  baseSourcePullRequest: {
                    number: base.sourcePullRequest.number,
                    url: base.sourcePullRequest.url ?? null,
                    title: base.sourcePullRequest.title ?? null,
                    headRefName: base.sourcePullRequest.headRefName,
                    baseRefName: base.sourcePullRequest.baseRefName ?? null,
                    headRefOid: base.sourcePullRequest.headRefOid ?? null,
                  },
                }
              : {}),
          }
        : {}),
    },
    UpdateAgentConversationWorkspaceFromBaseResponseSchema,
  );
  return transformUpdateAgentConversationWorkspaceFromBaseResponse(raw);
}

export async function publishAgentConversationWorkspace(
  conversationId: string,
): Promise<PublishAgentConversationWorkspaceResult> {
  const raw = await typedInvoke(
    "publish_agent_conversation_workspace",
    { conversationId },
    PublishAgentConversationWorkspaceResponseSchema,
  );
  return transformPublishAgentConversationWorkspaceResponse(raw);
}

export async function commitAgentConversationWorkspaceLocally(
  conversationId: string,
  input: CommitAgentConversationWorkspaceLocallyInput,
): Promise<CommitAgentConversationWorkspaceLocallyResult> {
  const raw = await typedInvoke(
    "commit_agent_conversation_workspace_locally",
    { input: { conversationId, ...input } },
    CommitAgentConversationWorkspaceLocallyResponseSchema,
  );
  return transformCommitAgentConversationWorkspaceLocallyResponse(raw);
}

export async function setAgentConversationWorkspacePrSupervision(
  conversationId: string,
  input: SetAgentConversationWorkspacePrSupervisionInput,
): Promise<AgentConversationWorkspace> {
  const raw = await typedInvoke(
    "set_agent_conversation_workspace_pr_supervision",
    {
      conversationId,
      input: {
        autoFixEnabled: input.autoFixEnabled,
        autoMergeDesired: input.autoMergeDesired,
        ...(input.autoMergeMethod
          ? { autoMergeMethod: input.autoMergeMethod }
          : {}),
      },
    },
    AgentConversationWorkspaceResponseSchema,
  );
  return transformAgentConversationWorkspace(raw);
}

export async function setAgentConversationWorkspaceAutoPublish(
  conversationId: string,
  input: SetAgentConversationWorkspaceAutoPublishInput,
): Promise<AgentConversationWorkspace> {
  const raw = await typedInvoke(
    "set_agent_conversation_workspace_auto_publish",
    {
      conversationId,
      input: {
        autoPublishEnabled: input.autoPublishEnabled,
      },
    },
    AgentConversationWorkspaceResponseSchema,
  );
  return transformAgentConversationWorkspace(raw);
}

export async function setAgentConversationWorkspaceReviewAutomation(
  conversationId: string,
  input: SetAgentConversationWorkspaceReviewAutomationInput,
): Promise<AgentConversationWorkspace> {
  const raw = await typedInvoke(
    "set_agent_conversation_workspace_review_automation",
    {
      conversationId,
      input: { enabled: input.enabled },
    },
    AgentConversationWorkspaceResponseSchema,
  );
  return transformAgentConversationWorkspace(raw);
}

export async function precomputeAgentConversationWorkspacePrDescription(
  conversationId: string,
): Promise<PrecomputeAgentConversationWorkspacePrDescriptionResult> {
  const raw = await typedInvoke(
    "precompute_agent_conversation_workspace_pr_description",
    { conversationId },
    PrecomputeAgentConversationWorkspacePrDescriptionResponseSchema,
  );
  return transformPrecomputeAgentConversationWorkspacePrDescriptionResponse(
    raw,
  );
}

export async function closeAgentWorkspacePr(
  conversationId: string,
): Promise<AgentConversationWorkspace> {
  const raw = await typedInvoke(
    "close_agent_workspace_pr",
    { conversationId },
    AgentConversationWorkspaceResponseSchema,
  );
  return transformAgentConversationWorkspace(raw);
}

/**
 * A remote conversation-start intent that reached a non-`started` terminal state. The host
 * classifies the failure as `errorCode`; the composer shows it with a retry affordance and
 * keeps the type (rather than a plain `Error`) so callers can branch on `status`.
 */
export class RemoteConversationStartError extends Error {
  readonly status: RemoteConversationStartStatus;
  readonly errorCode: string | null;
  constructor(status: RemoteConversationStartStatus, errorCode: string | null) {
    super(errorCode ?? `Conversation start ${status}`);
    this.name = "RemoteConversationStartError";
    this.status = status;
    this.errorCode = errorCode;
  }
}

/**
 * Projects a start request onto the spawn-free remote input (contract §2.1/§2.5). Only
 * client-settable fields cross: project, content, title, and the provider/model/effort
 * NAMES and known mode the host re-validates before spawn. First-turn role, team intent,
 * base/branch, persona, attachments, and `refreshRuntime` are host-forced or absent by design
 * and MUST NOT be forwarded.
 */
function remoteConversationStartInvokeInput(input: StartAgentConversationInput) {
  if (!input.projectId) {
    throw new Error("A project is required to start a conversation remotely.");
  }
  return {
    projectId: input.projectId,
    content: input.content,
    ...(input.title ? { title: input.title } : {}),
    ...(input.providerHarness ? { provider: input.providerHarness } : {}),
    ...(input.modelId ? { modelOverride: input.modelId } : {}),
    ...(input.logicalEffort ? { logicalEffort: input.logicalEffort } : {}),
    mode: input.mode ?? "chat",
  };
}

const REMOTE_CONVERSATION_START_POLL_INTERVAL_MS = 750;
/** ~3 minutes at the poll interval — a host that has not settled by then is surfaced as a timeout. */
const REMOTE_CONVERSATION_START_MAX_POLLS = 240;

function isTerminalRemoteConversationStartStatus(
  status: RemoteConversationStartStatus,
): boolean {
  return (
    REMOTE_CONVERSATION_START_TERMINAL_STATUSES as readonly string[]
  ).includes(status);
}

function remoteConversationStartPollDelay(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

/**
 * Polls the host-side start intent to a terminal state. The first read is immediate (so a
 * host that started synchronously returns without a delay); subsequent reads back off by a
 * fixed interval. Fail-closed: a repo/read error propagates rather than reading as "not
 * started yet" forever.
 */
async function pollRemoteConversationStart(
  startRequestId: string,
): Promise<RemoteConversationStartRequest> {
  for (
    let attempt = 0;
    attempt < REMOTE_CONVERSATION_START_MAX_POLLS;
    attempt += 1
  ) {
    // Literal command name (P-11): the status read is a pure host-side repository read.
    const request = await typedInvoke(
      "get_remote_conversation_start_request",
      { startRequestId },
      GetRemoteConversationStartRequestResponseSchema,
    );
    if (isTerminalRemoteConversationStartStatus(request.status)) {
      return request;
    }
    await remoteConversationStartPollDelay(
      REMOTE_CONVERSATION_START_POLL_INTERVAL_MS,
    );
  }
  throw new Error(
    "Timed out waiting for the host to start the conversation. It may still be starting — reopen it from the sidebar.",
  );
}

/**
 * The remote half of `startAgentConversation` (contract §3.1). Persists a start-intent on
 * the host, polls it to completion, then navigates into the seeded conversation through the
 * SAME remote transcript read the rest of the Agents surface uses. No local seeded-create
 * runs — the host command seeds its own conversation.
 */
async function startRemoteAgentConversation(
  input: StartAgentConversationInput,
): Promise<StartAgentConversationResult> {
  // Literal command name (P-11): persists a start-intent — no client-side spawn sink.
  const started = await typedInvoke(
    "request_remote_agent_conversation_start",
    { input: remoteConversationStartInvokeInput(input) },
    RequestRemoteAgentConversationStartResponseSchema,
  );
  const request = await pollRemoteConversationStart(started.startRequestId);
  if (request.status !== "started") {
    throw new RemoteConversationStartError(
      request.status,
      request.errorCode ?? null,
    );
  }
  const { conversation } = await getConversation(request.conversationId);
  return {
    conversation,
    workspace: null,
    sendResult: {
      conversationId: request.conversationId,
      agentRunId: request.agentRunId ?? "",
      isNewConversation: true,
      wasQueued: false,
      queuedAsPending: false,
    },
  };
}

export async function startAgentConversation(
  input: StartAgentConversationInput,
): Promise<StartAgentConversationResult> {
  // Two literal `typedInvoke` sites (P-11): a remote environment persists a start-intent and
  // polls the host to completion (the host owns the spawn); local starts inline. The remote
  // branch deliberately skips `startAgentConversationInvokeInput` — its base/persona/team/
  // attachment fields are host-forced or absent under the spawn-free contract.
  if (isRemoteEnvironmentId(getTransportEnvironmentId())) {
    return startRemoteAgentConversation(input);
  }
  const raw = await typedInvoke(
    "start_agent_conversation",
    {
      input: startAgentConversationInvokeInput(input),
    },
    StartAgentConversationResponseSchema,
  );
  return transformStartAgentConversationResponse(raw);
}

export async function forkAgentConversation(
  conversationId: string,
): Promise<ForkAgentConversationResult> {
  const raw = await typedInvoke(
    "fork_agent_conversation",
    {
      input: {
        conversationId,
      },
    },
    ForkAgentConversationResponseSchema,
  );
  return transformForkAgentConversationResponse(raw);
}

export class RemoteConversationModeSwitchError extends Error {
  readonly status: RemoteConversationModeSwitchStatus;
  readonly errorCode: string | null;
  constructor(
    status: RemoteConversationModeSwitchStatus,
    errorCode: string | null,
  ) {
    super(errorCode ?? `Mode switch ${status}`);
    this.name = "RemoteConversationModeSwitchError";
    this.status = status;
    this.errorCode = errorCode;
  }
}

const REMOTE_CONVERSATION_MODE_SWITCH_POLL_INTERVAL_MS = 750;
/** ~3 minutes — worktree preparation on the host can legitimately take a while. */
const REMOTE_CONVERSATION_MODE_SWITCH_MAX_POLLS = 240;

function isTerminalRemoteConversationModeSwitchStatus(
  status: RemoteConversationModeSwitchStatus,
): boolean {
  return (
    REMOTE_CONVERSATION_MODE_SWITCH_TERMINAL_STATUSES as readonly string[]
  ).includes(status);
}

/**
 * Polls the host-side mode-switch intent to a terminal state. Fail-closed: a repo/read error
 * propagates rather than reading as "still switching" forever.
 */
async function pollRemoteConversationModeSwitch(
  modeSwitchRequestId: string,
): Promise<RemoteConversationModeSwitchRequest> {
  for (
    let attempt = 0;
    attempt < REMOTE_CONVERSATION_MODE_SWITCH_MAX_POLLS;
    attempt += 1
  ) {
    // Literal command name (P-11): the status read is a pure host-side repository read.
    const request = await typedInvoke(
      "get_remote_conversation_mode_switch_request",
      { modeSwitchRequestId },
      GetRemoteConversationModeSwitchRequestResponseSchema,
    );
    if (isTerminalRemoteConversationModeSwitchStatus(request.status)) {
      return request;
    }
    await remoteConversationStartPollDelay(
      REMOTE_CONVERSATION_MODE_SWITCH_POLL_INTERVAL_MS,
    );
  }
  throw new Error(
    "Timed out waiting for the host to switch the conversation mode. It may still be switching — check the conversation shortly.",
  );
}

/**
 * The remote half of `switchAgentConversationMode` (WP5a). Persists a mode-switch intent on
 * the host, polls it to a terminal state (the host owns the worktree preparation), then
 * re-reads the conversation through the registered remote transcript read. `base` and
 * `runtimeOverride` are host-forced under the spawn-free contract and MUST NOT cross the
 * wire; no production remote caller passes them, so their presence is a programming error
 * surfaced loudly rather than dropped silently.
 */
async function switchRemoteAgentConversationMode(
  input: SwitchAgentConversationModeInput,
): Promise<SwitchAgentConversationModeResult> {
  if (input.base || input.runtimeOverride) {
    throw new Error(
      "base/runtimeOverride cannot be set on a remote mode switch — the host owns workspace preparation.",
    );
  }
  const { conversation: current } = await getConversation(input.conversationId);
  if (current.contextType !== "project") {
    throw new Error(
      "Only project conversations can switch modes on a remote host.",
    );
  }
  // Literal command name (P-11): persists a mode-switch intent — no client-side spawn sink.
  const requested = await typedInvoke(
    "request_remote_agent_conversation_mode_switch",
    {
      input: {
        conversationId: input.conversationId,
        projectId: current.contextId,
        mode: input.mode,
      },
    },
    RequestRemoteAgentConversationModeSwitchResponseSchema,
  );
  const request = await pollRemoteConversationModeSwitch(
    requested.modeSwitchRequestId,
  );
  if (
    !(REMOTE_CONVERSATION_MODE_SWITCH_SUCCESS_STATUSES as readonly string[]).includes(
      request.status,
    )
  ) {
    throw new RemoteConversationModeSwitchError(
      request.status,
      request.errorCode ?? null,
    );
  }
  const { conversation } = await getConversation(input.conversationId);
  // The workspace re-hydrates through the call sites' query invalidations; the switch result
  // deliberately reports `null` rather than a stale optimistic copy.
  return { conversation, workspace: null };
}

export async function switchAgentConversationMode(
  input: SwitchAgentConversationModeInput,
): Promise<SwitchAgentConversationModeResult> {
  // Two literal `typedInvoke` sites (P-11): a remote environment persists a mode-switch
  // intent and polls the host to completion (the host owns the worktree preparation);
  // local switches inline.
  if (isRemoteEnvironmentId(getTransportEnvironmentId())) {
    return switchRemoteAgentConversationMode(input);
  }
  const raw = await typedInvoke(
    "switch_agent_conversation_mode",
    {
      input: {
        conversationId: input.conversationId,
        mode: input.mode,
        ...(input.runtimeOverride
          ? { runtimeOverride: roleRuntimeOverrideInvokeInput(input.runtimeOverride) }
          : {}),
        ...(input.base
          ? {
              baseRefKind: input.base.kind,
              ...(input.base.branchMode
                ? { baseBranchMode: input.base.branchMode }
                : {}),
              baseRef: input.base.ref,
              baseDisplayName: input.base.displayName,
              ...(input.base.sourcePullRequest
                ? {
                    baseSourcePullRequest: sourcePullRequestInvokeInput(
                      input.base.sourcePullRequest,
                    ),
                  }
                : {}),
            }
          : {}),
      },
    },
    SwitchAgentConversationModeResponseSchema,
  );
  return transformSwitchAgentConversationModeResponse(raw);
}

export async function updateAgentConversationCoordinationMode(
  input: UpdateAgentConversationCoordinationModeInput,
): Promise<ChatConversation> {
  const raw = await typedInvoke(
    "update_agent_conversation_coordination_mode",
    {
      input: {
        conversationId: input.conversationId,
        coordinationMode: input.coordinationMode,
        ...(input.modelOverride ? { modelOverride: input.modelOverride } : {}),
      },
    },
    ChatConversationResponseSchema,
  );
  return transformConversation(raw);
}

export async function copyAgentConversationPlan(
  input: CopyAgentConversationPlanInput,
): Promise<AgentConversationPlanSeedResult> {
  const raw = await typedInvoke(
    "copy_agent_conversation_plan",
    {
      input: {
        conversationId: input.conversationId,
        sourceSessionId: input.sourceSessionId,
        sourceArtifactId: input.sourceArtifactId,
        sourceVersion: input.sourceVersion,
      },
    },
    AgentConversationPlanSeedResponseSchema,
  );
  return transformAgentConversationPlanSeedResponse(raw);
}

export async function importAgentConversationPlan(
  input: ImportAgentConversationPlanInput,
): Promise<AgentConversationPlanSeedResult> {
  const raw = await typedInvoke(
    "import_agent_conversation_plan",
    {
      input: {
        conversationId: input.conversationId,
        title: input.title,
        content: input.content,
      },
    },
    AgentConversationPlanSeedResponseSchema,
  );
  return transformAgentConversationPlanSeedResponse(raw);
}

export async function activateAgentTaskPipeline(input: {
  conversationId: string;
  sessionId: string;
  runtimeOverride?: ManualRoleRuntimeSelection;
}): Promise<AgentConversationWorkspace> {
  const raw = await typedInvoke(
    "activate_agent_task_pipeline",
    {
      input: {
        conversationId: input.conversationId,
        sessionId: input.sessionId,
        ...(input.runtimeOverride
          ? { runtimeOverride: roleRuntimeOverrideInvokeInput(input.runtimeOverride) }
          : {}),
      },
    },
    AgentConversationWorkspaceResponseSchema,
  );
  return transformAgentConversationWorkspace(raw);
}

export async function activateAgentPlanDirectImplementation(input: {
  conversationId: string;
  sessionId: string;
  retry: boolean;
}): Promise<{
  workspace: AgentConversationWorkspace;
  artifactReferences: ComposerArtifactReference[];
  planContextFingerprint: string;
}> {
  const responseSchema = z.object({
    workspace: AgentConversationWorkspaceResponseSchema,
    artifact_references: z.array(
      z.object({
        artifactId: z.string(),
        kind: z.string(),
        title: z.string().optional(),
        sessionId: z.string().optional(),
        version: z.number().int().positive().optional(),
        status: z.string().optional(),
      }),
    ),
    plan_context_fingerprint: z.string().min(1),
  });
  const raw = await typedInvoke(
    "activate_agent_plan_direct_implementation",
    {
      input: {
        conversationId: input.conversationId,
        sessionId: input.sessionId,
        retry: input.retry,
      },
    },
    responseSchema,
  );
  return {
    workspace: transformAgentConversationWorkspace(raw.workspace),
    artifactReferences: raw.artifact_references.map((reference) => ({
      artifactId: reference.artifactId,
      kind: reference.kind,
      ...(reference.title ? { title: reference.title } : {}),
      ...(reference.sessionId ? { sessionId: reference.sessionId } : {}),
      ...(reference.version ? { version: reference.version } : {}),
      ...(reference.status ? { status: reference.status } : {}),
    })),
    planContextFingerprint: raw.plan_context_fingerprint,
  };
}

export async function startAgentTaskPipeline(input: {
  conversationId: string;
  sessionId: string;
  proposalIds: string[];
  baseBranchOverride?: string | null;
}): Promise<{ tasksCreated: number; executionPlanId: string | null }> {
  const raw = await typedInvoke(
    "start_agent_task_pipeline",
    {
      input: {
        conversationId: input.conversationId,
        sessionId: input.sessionId,
        proposalIds: input.proposalIds,
        ...(input.baseBranchOverride
          ? { baseBranchOverride: input.baseBranchOverride }
          : {}),
      },
    },
    z
      .object({
        tasks_created: z.number().int().nonnegative(),
        execution_plan_id: z.string().nullable(),
      })
      .passthrough(),
  );
  return {
    tasksCreated: raw.tasks_created,
    executionPlanId: raw.execution_plan_id,
  };
}

const SendRemoteChatMessageResponseSchema = z
  .object({
    conversationId: z.string(),
    queuedMessageId: z.string(),
    agentRunId: z.string(),
    createdAt: z.string(),
  })
  .passthrough();

/**
 * Human copy for the host's spawn-free-send refusals.
 *
 * The host answers with stable codes; a user who taps send after the agent has finished
 * its turn should not be shown `REMOTE_CHAT_SEND_NOT_STEERABLE`. Unrecognised errors pass
 * through untouched — inventing prose for an error we do not understand would hide it.
 */
const REMOTE_CHAT_SEND_MESSAGES: Readonly<Record<string, string>> = {
  REMOTE_CHAT_SEND_NOT_STEERABLE:
    "That agent isn't running right now — remote sends can only reach a conversation the host has already started.",
  REMOTE_CHAT_SEND_CONVERSATION_ARCHIVED:
    "This conversation is archived. Restore it on the host to keep talking.",
  REMOTE_CHAT_SEND_CONVERSATION_NOT_FOUND:
    "That conversation no longer exists on the host.",
  REMOTE_CHAT_SEND_LOOKUP_FAILED:
    "The host couldn't confirm whether that conversation is live, so nothing was sent.",
  REMOTE_CHAT_SEND_ENQUEUE_FAILED: "The host couldn't queue that message. Try again.",
  REMOTE_CHAT_SEND_EMPTY_CONTENT: "Type a message before sending.",
  REMOTE_CHAT_SEND_ROLE_NOT_PERMITTED:
    "Remote devices can only send your own messages.",
};

/** The readable refusal for a known host code, or `null` to leave the error alone. */
function remoteChatSendRefusal(error: unknown): Error | null {
  if (error instanceof RemoteTransportError) return null;
  const raw = error instanceof Error ? error.message : String(error);
  const copy = REMOTE_CHAT_SEND_MESSAGES[raw.trim()];
  return copy === undefined ? null : new Error(copy);
}

/**
 * A remote continuation intent that reached a non-`dispatched` terminal state. The host
 * classifies the failure as `errorCode`; the type (rather than a plain `Error`) is kept so the
 * composer can branch on `status` and offer a retry.
 */
export class RemoteConversationMessageError extends Error {
  readonly status: RemoteConversationMessageStatus;
  readonly errorCode: string | null;
  constructor(status: RemoteConversationMessageStatus, errorCode: string | null) {
    super(
      REMOTE_CONV_MESSAGE_MESSAGES[errorCode ?? ""] ??
        REMOTE_CONV_MESSAGE_STATUS_MESSAGES[status] ??
        errorCode ??
        `Message ${status}`,
    );
    this.name = "RemoteConversationMessageError";
    this.status = status;
    this.errorCode = errorCode;
  }
}

/**
 * Human copy for the host's continuation refusals, whether they arrive as a synchronous
 * command error or as a terminal `errorCode` on the polled intent. Unrecognised codes pass
 * through untouched — inventing prose for an error we do not understand would hide it.
 */
const REMOTE_CONV_MESSAGE_MESSAGES: Readonly<Record<string, string>> = {
  REMOTE_CONV_MESSAGE_EMPTY_CONTENT: "Type a message before sending.",
  REMOTE_CONV_MESSAGE_CONVERSATION_NOT_FOUND:
    "That conversation no longer exists on the host.",
  REMOTE_CONV_MESSAGE_CONVERSATION_ARCHIVED:
    "This conversation is archived. Restore it on the host to keep talking.",
  REMOTE_CONV_MESSAGE_PROJECT_MISMATCH:
    "That conversation belongs to a different project on the host.",
  REMOTE_CONV_MESSAGE_PROVIDER_NOT_ENABLED:
    "The provider this conversation uses is turned off on the host.",
  REMOTE_CONV_MESSAGE_MODEL_NOT_ENABLED:
    "That model is not enabled on the host. Pick another one and send again.",
  REMOTE_CONV_MESSAGE_LOOKUP_FAILED:
    "The host couldn't check this conversation, so nothing was sent.",
  REMOTE_CONV_MESSAGE_ENQUEUE_FAILED:
    "The host couldn't accept that message. Try again.",
  REMOTE_CONV_MESSAGE_RUN_WENT_LIVE:
    "The agent started working before your message went out. Send it again to reach the live run.",
  REMOTE_CONV_MESSAGE_HOST_SEND_FAILED:
    "The host couldn't start the agent for that message, so it was never delivered. Try again.",
};

/**
 * Fallback copy per terminal status, for a failure the host settled without an `errorCode`.
 * Nothing here may read as success: the whole point of surfacing terminal states is that a
 * message no agent ever saw must not look sent.
 */
const REMOTE_CONV_MESSAGE_STATUS_MESSAGES: Readonly<
  Partial<Record<RemoteConversationMessageStatus, string>>
> = {
  failed: "The host couldn't deliver that message. Try again.",
  cancelled: "That message was cancelled on the host and never delivered.",
  failedStale:
    "The host restarted before delivering that message, so it was never sent. Send it again.",
};

const REMOTE_CONVERSATION_MESSAGE_POLL_INTERVAL_MS = 750;
/** ~3 minutes at the poll interval — a host that has not settled by then is surfaced as a timeout. */
const REMOTE_CONVERSATION_MESSAGE_MAX_POLLS = 240;

function isTerminalRemoteConversationMessageStatus(
  status: RemoteConversationMessageStatus,
): boolean {
  return (
    REMOTE_CONVERSATION_MESSAGE_TERMINAL_STATUSES as readonly string[]
  ).includes(status);
}

function remoteConversationMessagePollDelay(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

/**
 * Polls a continuation intent to a terminal state. The first read is immediate; subsequent
 * reads back off by a fixed interval. Fail-closed: a read error propagates rather than reading
 * as "not dispatched yet" forever.
 */
async function pollRemoteConversationMessage(
  messageRequestId: string,
): Promise<RemoteConversationMessageRequest> {
  for (
    let attempt = 0;
    attempt < REMOTE_CONVERSATION_MESSAGE_MAX_POLLS;
    attempt += 1
  ) {
    // Literal command name (P-11): the status read is a pure host-side repository read.
    const request = await typedInvoke(
      "get_remote_conversation_message_request",
      { messageRequestId },
      GetRemoteConversationMessageRequestResponseSchema,
    );
    if (isTerminalRemoteConversationMessageStatus(request.status)) {
      return request;
    }
    await remoteConversationMessagePollDelay(
      REMOTE_CONVERSATION_MESSAGE_POLL_INTERVAL_MS,
    );
  }
  throw new Error(
    "Timed out waiting for the host to deliver that message. Check the conversation before sending again.",
  );
}

/** The readable refusal for a known continuation code, or `null` to leave the error alone. */
function remoteConversationMessageRefusal(error: unknown): Error | null {
  if (error instanceof RemoteTransportError) return null;
  const raw = error instanceof Error ? error.message : String(error);
  const copy = REMOTE_CONV_MESSAGE_MESSAGES[raw.trim()];
  return copy === undefined ? null : new Error(copy);
}

/**
 * The idle half of the remote send: persist a continuation intent, then poll it to a terminal
 * state before reporting anything to the composer.
 *
 * The poll is not optional politeness. Returning as soon as the intent is persisted would show
 * the user a sent message whose delivery is still unproven — the exact false-success the
 * chat-send design (§7) names as this queue's main hazard.
 */
async function continueRemoteConversation(
  content: string,
  conversationId: string,
  projectId: string,
  options?: SendAgentMessageOptions,
): Promise<SendAgentMessageResult> {
  // Literal command name (P-11): persists a continuation intent — no client-side spawn sink.
  // UX-5: the composer's model/effort selections TRAVEL here instead of being dropped.
  const requested = await typedInvoke(
    "request_remote_agent_conversation_message",
    {
      input: {
        conversationId,
        projectId,
        content,
        ...(options?.modelId ? { modelOverride: options.modelId } : {}),
        ...(options?.logicalEffort
          ? { logicalEffort: options.logicalEffort }
          : {}),
      },
    },
    RequestRemoteAgentConversationMessageResponseSchema,
  ).catch((error: unknown) => {
    throw remoteConversationMessageRefusal(error) ?? error;
  });

  const request = await pollRemoteConversationMessage(requested.messageRequestId);
  if (request.status !== "dispatched") {
    throw new RemoteConversationMessageError(
      request.status,
      request.errorCode ?? null,
    );
  }

  return {
    conversationId: request.conversationId,
    agentRunId: request.agentRunId ?? "",
    isNewConversation: false,
    wasQueued: false,
    queuedAsPending: false,
  };
}

/**
 * The remote half of {@link sendAgentMessage}.
 *
 * `send_agent_message` is not, and will not be, registered on the remote facade — it
 * reaches a process-launch sink, and the facade's detector-(c) floor admits no
 * exceptions. A paired device therefore uses two spawn-free host commands, chosen by the
 * HOST rather than by client-side liveness inference:
 *
 * - a run is live ⇒ `send_remote_chat_message` queues the turn into the queue that run drains;
 * - the conversation is idle ⇒ `request_remote_agent_conversation_message` persists a
 *   continuation intent a host dispatcher sends through the provider-session resume seam.
 *
 * The live path is attempted FIRST and its `REMOTE_CHAT_SEND_NOT_STEERABLE` refusal is the
 * branch condition. That ordering is deliberate: the host is the only authority on run
 * liveness, and a client that guessed would either double a turn (guessing idle while a run
 * drains the queue) or strand one (guessing live with nothing to drain it). The idle command
 * independently refuses with `REMOTE_CONV_MESSAGE_RUN_ALREADY_LIVE` if a run goes live inside
 * the race window, so both directions fail closed.
 *
 * Starting a conversation stays host-only, so a send with no conversation id is refused here
 * rather than silently creating one.
 *
 * Options that only a host-side send can honour — attachments, team intent, composer
 * references — are still not forwarded, because neither facade command accepts them. Model and
 * effort DO travel on the idle path (UX-5).
 */
async function sendRemoteChatMessage(
  content: string,
  conversationId: string | null | undefined,
  projectId: string,
  options?: SendAgentMessageOptions,
): Promise<SendAgentMessageResult> {
  if (!conversationId) {
    throw new RemoteTransportError({
      code: "REMOTE_COMMAND_UNAVAILABLE",
      message:
        "Starting a conversation runs only on the host — open an existing conversation to send from here.",
      environmentId: getTransportEnvironmentId(),
      cmd: "send_remote_chat_message",
    });
  }

  let raw: z.infer<typeof SendRemoteChatMessageResponseSchema>;
  try {
    raw = await typedInvoke(
      "send_remote_chat_message",
      {
        input: {
          conversationId,
          content,
          // Server-pinned to "user" at dispatch. Sent explicitly so the wire shape is
          // self-describing; the host overwrites whatever arrives here.
          role: "user",
        },
      },
      SendRemoteChatMessageResponseSchema,
    );
  } catch (error: unknown) {
    // A RemoteTransportError must reach the caller INTACT — the composer's unknown-outcome
    // reconcile and the remote error banner both discriminate on its type, and rewrapping it
    // here would silently convert a "did that go through?" into an ordinary failure. It also
    // must NOT fall through to the idle path: an unknown outcome may already have queued the
    // turn, and continuing would risk sending it twice.
    if (error instanceof RemoteTransportError) throw error;
    const code = (error instanceof Error ? error.message : String(error)).trim();
    if (code === "REMOTE_CHAT_SEND_NOT_STEERABLE") {
      return continueRemoteConversation(
        content,
        conversationId,
        projectId,
        options,
      );
    }
    throw remoteChatSendRefusal(error) ?? error;
  }

  return {
    conversationId: raw.conversationId,
    agentRunId: raw.agentRunId,
    isNewConversation: false,
    wasQueued: true,
    queuedAsPending: false,
    queuedMessageId: raw.queuedMessageId,
  };
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
  options?: SendAgentMessageOptions,
): Promise<SendAgentMessageResult> {
  if (isRemoteEnvironmentId(getTransportEnvironmentId())) {
    // `contextId` IS the project id for the Project context, which is the only context the
    // remote Agents surface exposes. It is passed explicitly so the host can prove the named
    // conversation belongs to it rather than trusting the conversation id alone.
    return sendRemoteChatMessage(
      content,
      options?.conversationId,
      contextId,
      options,
    );
  }

  const raw = await typedInvoke(
    "send_agent_message",
    {
      input: {
        contextType,
        contextId,
        content,
        ...(attachmentIds !== undefined &&
          attachmentIds.length > 0 && { attachmentIds }),
        ...(options?.conversationId
          ? { conversationId: options.conversationId }
          : {}),
        ...(options?.providerHarness
          ? { providerHarness: options.providerHarness }
          : {}),
        ...(options?.modelId ? { modelOverride: options.modelId } : {}),
        ...(options?.logicalEffort
          ? { logicalEffort: options.logicalEffort }
          : {}),
        ...(options?.codexFastMode != null
          ? { codexFastMode: options.codexFastMode }
          : {}),
        ...(options?.runtimeOverride
          ? { runtimeOverride: roleRuntimeOverrideInvokeInput(options.runtimeOverride) }
          : {}),
        ...(options?.suppressUserMessage ? { suppressUserMessage: true } : {}),
        ...(options?.requireApprovedLinkedPlan
          ? { requireApprovedLinkedPlan: true }
          : {}),
        ...(options?.expectedLinkedPlanFingerprint
          ? {
              expectedLinkedPlanFingerprint:
                options.expectedLinkedPlanFingerprint,
            }
          : {}),
        ...(options?.capabilityIntent
          ? { capabilityIntent: options.capabilityIntent }
          : options?.teamIntent
            ? { teamIntent: options.teamIntent }
            : {}),
        ...(options?.teamMessageTarget
          ? { teamMessageTarget: options.teamMessageTarget }
          : {}),
        ...(options?.composerProjectReferences?.length
          ? { composerProjectReferences: options.composerProjectReferences }
          : {}),
        ...(options?.composerIntegrationReferences?.length
          ? {
              composerIntegrationReferences:
                options.composerIntegrationReferences,
            }
          : {}),
        ...(options?.composerArtifactReferences?.length
          ? { composerArtifactReferences: options.composerArtifactReferences }
          : {}),
        ...(options?.composerSelectionSnapshot
          ? { composerSelectionSnapshot: options.composerSelectionSnapshot }
          : {}),
        ...(options?.composerExcerptReferences?.length
          ? { composerExcerptReferences: options.composerExcerptReferences }
          : {}),
      },
    },
    SendAgentMessageResponseSchema,
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
  contextId: string,
): Promise<QueuedMessageResponse[]> {
  const raw = await typedInvoke(
    "get_queued_agent_messages",
    { contextType, contextId },
    z.array(QueuedMessageResponseSchema),
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
  messageId: string,
): Promise<boolean> {
  return typedInvoke(
    "delete_queued_agent_message",
    { contextType, contextId, messageId },
    z.boolean(),
  );
}

/**
 * Send a queued message immediately.
 *
 * Interrupts the active provider process for the queue context, then sends the
 * selected queued payload through the normal provider-session resume path.
 */
export async function sendQueuedAgentMessageNow(
  contextType: ContextType,
  contextId: string,
  messageId: string,
): Promise<SendAgentMessageResult> {
  const raw = await typedInvoke(
    "send_queued_agent_message_now",
    { contextType, contextId, messageId },
    SendAgentMessageResponseSchema,
  );
  return transformSendAgentMessageResponse(raw);
}

/**
 * Check if the chat service is available (Claude CLI installed)
 */
export async function isChatServiceAvailable(): Promise<boolean> {
  return typedInvoke("is_chat_service_available", {}, z.boolean());
}

/**
 * The remote spawn-free STOP seam (WP2).
 *
 * `stop_agent` reaches `Command::new(resolve_pkill_cli_path())` on the host and stays
 * unregistered by the absolute process floor, so a paired device never calls it. It persists a
 * STOP INTENT through `request_remote_agent_stop` — registered at `ui:operate`, i.e. reachable
 * from the DEFAULT "viewer with brakes" pairing — and polls `get_remote_agent_stop_request`
 * until the host-owned dispatcher settles it.
 *
 * Status values are the host enum serialized camelCase (`RemoteAgentStopStatus`).
 */
export const RemoteAgentStopStatusSchema = z.enum([
  "pending",
  "stopping",
  "stopped",
  "noLiveRun",
  "failed",
  "cancelled",
  "failedStale",
]);
export type RemoteAgentStopStatus = z.infer<typeof RemoteAgentStopStatusSchema>;

/**
 * Terminal states. `noLiveRun` is BENIGN — the brake was pulled and nothing was running — and
 * is deliberately grouped with `stopped` rather than with the failures: showing "couldn't stop
 * the agent" for the ordinary finished-between-tap-and-drain race would train users to ignore
 * the error.
 */
const REMOTE_AGENT_STOP_SUCCESS_STATUSES = ["stopped", "noLiveRun"] as const;
const REMOTE_AGENT_STOP_TERMINAL_STATUSES = [
  ...REMOTE_AGENT_STOP_SUCCESS_STATUSES,
  "failed",
  "cancelled",
  "failedStale",
] as const;

const RequestRemoteAgentStopResponseSchema = z
  .object({
    stopRequestId: z.string(),
    conversationId: z.string(),
    status: RemoteAgentStopStatusSchema,
    deduplicated: z.boolean(),
    createdAt: z.string(),
  })
  .strict();

const GetRemoteAgentStopRequestResponseSchema = z
  .object({
    id: z.string(),
    conversationId: z.string(),
    status: RemoteAgentStopStatusSchema,
    errorCode: z.string().nullable(),
    createdAt: z.string(),
    updatedAt: z.string(),
  })
  .strict();

/**
 * A remote stop intent that reached a non-success terminal state. Typed (rather than a plain
 * `Error`) so callers can branch on `status` — the brake failing is exactly the class of
 * failure that used to be swallowed at both call sites.
 */
export class RemoteAgentStopError extends Error {
  readonly status: RemoteAgentStopStatus;
  readonly errorCode: string | null;
  constructor(status: RemoteAgentStopStatus, errorCode: string | null) {
    super(errorCode ?? `Stop request ${status}`);
    this.name = "RemoteAgentStopError";
    this.status = status;
    this.errorCode = errorCode;
  }
}

const REMOTE_AGENT_STOP_POLL_INTERVAL_MS = 400;
/** ~24s at the poll interval. A brake that has not bitten by then is surfaced, not hidden. */
const REMOTE_AGENT_STOP_MAX_POLLS = 60;

function isTerminalRemoteAgentStopStatus(status: RemoteAgentStopStatus): boolean {
  return (REMOTE_AGENT_STOP_TERMINAL_STATUSES as readonly string[]).includes(
    status,
  );
}

function remoteAgentStopPollDelay(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

/**
 * The remote half of {@link stopAgent}. Persists a stop-intent and polls it to a terminal
 * state. Fail-closed: a read error propagates rather than reading as "not stopped yet" forever,
 * and a non-success terminal throws {@link RemoteAgentStopError} so the caller can surface it.
 */
async function stopRemoteAgent(conversationId: string): Promise<boolean> {
  // Literal command name (P-11): persists a stop-intent — no client-side process sink.
  const requested = await typedInvoke(
    "request_remote_agent_stop",
    { input: { conversationId } },
    RequestRemoteAgentStopResponseSchema,
  );

  let request = {
    status: requested.status as RemoteAgentStopStatus,
    errorCode: null as string | null,
  };
  for (let attempt = 0; attempt < REMOTE_AGENT_STOP_MAX_POLLS; attempt += 1) {
    if (isTerminalRemoteAgentStopStatus(request.status)) break;
    await remoteAgentStopPollDelay(REMOTE_AGENT_STOP_POLL_INTERVAL_MS);
    // Literal command name (P-11): the status read is a pure host-side repository read.
    const polled = await typedInvoke(
      "get_remote_agent_stop_request",
      { stopRequestId: requested.stopRequestId },
      GetRemoteAgentStopRequestResponseSchema,
    );
    request = { status: polled.status, errorCode: polled.errorCode };
  }

  if (!isTerminalRemoteAgentStopStatus(request.status)) {
    throw new Error(
      "Timed out waiting for the host to stop the agent. It may still be stopping — reopen the conversation to check.",
    );
  }
  if (
    !(REMOTE_AGENT_STOP_SUCCESS_STATUSES as readonly string[]).includes(
      request.status,
    )
  ) {
    throw new RemoteAgentStopError(request.status, request.errorCode);
  }
  // `stopped` terminated a run; `noLiveRun` found none. Same shape the local command returns.
  return request.status === "stopped";
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
  contextId: string,
): Promise<boolean> {
  // Two literal `typedInvoke` sites live in `stopRemoteAgent` (P-11): a remote environment
  // persists a stop-intent and polls the host, which owns the terminating path. Only `project`
  // conversations exist on the remote Agents surface, and for those the backend queue context
  // id IS the conversation id — the intent takes no `contextType`, so any other context must
  // keep the local command rather than be silently re-aimed at a conversation.
  if (isRemoteEnvironmentId(getTransportEnvironmentId()) && contextType === "project") {
    return stopRemoteAgent(contextId);
  }
  return typedInvoke("stop_agent", { contextType, contextId }, z.boolean());
}

/**
 * Check if an agent is currently running for a context
 *
 * @param contextType The context type
 * @param contextId The context ID
 */
export async function isAgentRunning(
  contextType: ContextType,
  contextId: string,
): Promise<boolean> {
  return typedInvoke(
    "is_agent_running",
    { contextType, contextId },
    z.boolean(),
  );
}

/**
 * Bulk-check whether agents are currently running for multiple context IDs.
 */
const AgentRuntimeStatusSchema = z.enum([
  "idle",
  "generating",
  "waiting_for_input",
]);

export type AgentRuntimeStatus = z.infer<typeof AgentRuntimeStatusSchema>;

export interface AgentRunningState {
  isRunning: boolean;
  agentStatus: AgentRuntimeStatus;
}

const AgentRunningStateSchema = z.union([
  z.boolean().transform((isRunning): AgentRunningState => ({
    isRunning,
    agentStatus: isRunning ? "generating" : "idle",
  })),
  z
    .object({
      is_running: z.boolean().optional(),
      isRunning: z.boolean().optional(),
      agent_status: AgentRuntimeStatusSchema.optional(),
      agentStatus: AgentRuntimeStatusSchema.optional(),
    })
    .transform((state): AgentRunningState => {
      const isRunning = state.is_running ?? state.isRunning ?? false;
      const agentStatus =
        state.agent_status ??
        state.agentStatus ??
        (isRunning ? "generating" : "idle");
      return {
        isRunning,
        agentStatus: isRunning
          ? agentStatus === "idle"
            ? "generating"
            : agentStatus
          : "idle",
      };
    }),
]);

export async function getAgentRunningStates(
  contextType: ContextType,
  contextIds: string[],
): Promise<Record<string, AgentRunningState>> {
  return typedInvoke(
    "get_agent_running_states",
    { contextType, contextIds },
    z.record(z.string(), AgentRunningStateSchema),
  );
}

const AgentConversationRuntimeSourceSchema = z.enum([
  "workspace",
  "workspace_review",
  "ideation",
  "verification",
  "task_execution",
  "review",
  "merge",
]);

export type AgentConversationRuntimeSource = z.infer<
  typeof AgentConversationRuntimeSourceSchema
>;

export interface AgentConversationRuntimeItem {
  source: AgentConversationRuntimeSource;
  contextType: ContextType;
  contextId: string;
  label: string;
  title: string;
  agentStatus: AgentRuntimeStatus;
  taskId: string | null;
  internalStatus: string | null;
  runningProcess: RunningProcess | null;
  ideationSession: RunningIdeationSession | null;
  parentSessionId: string | null;
  childSessionId: string | null;
  conversationId: string | null;
}

export interface AgentConversationRuntimeStatus {
  conversationId: string;
  isRunning: boolean;
  agentStatus: AgentRuntimeStatus;
  primarySource: AgentConversationRuntimeSource | null;
  summaryLabel: string | null;
  items: AgentConversationRuntimeItem[];
}

const AgentConversationRuntimeItemSchema = z
  .object({
    source: AgentConversationRuntimeSourceSchema,
    contextType: ContextTypeSchema,
    contextId: z.string(),
    label: z.string(),
    title: z.string(),
    agentStatus: AgentRuntimeStatusSchema,
    taskId: z.string().nullable(),
    internalStatus: z.string().nullable(),
    runningProcess: RunningProcessSchema.nullable(),
    ideationSession: RunningIdeationSessionSchema.nullable(),
    parentSessionId: z.string().nullable(),
    childSessionId: z.string().nullable(),
    conversationId: z.string().nullable(),
  })
  .transform((item): AgentConversationRuntimeItem => ({
    source: item.source,
    contextType: item.contextType,
    contextId: item.contextId,
    label: item.label,
    title: item.title,
    agentStatus: item.agentStatus,
    taskId: item.taskId,
    internalStatus: item.internalStatus,
    runningProcess: item.runningProcess
      ? transformRunningProcess(item.runningProcess)
      : null,
    ideationSession: item.ideationSession
      ? transformRunningIdeationSession(item.ideationSession)
      : null,
    parentSessionId: item.parentSessionId,
    childSessionId: item.childSessionId,
    conversationId: item.conversationId,
  }));

const AgentConversationRuntimeStatusSchema = z
  .object({
    conversationId: z.string(),
    isRunning: z.boolean(),
    agentStatus: AgentRuntimeStatusSchema,
    primarySource: AgentConversationRuntimeSourceSchema.nullable(),
    summaryLabel: z.string().nullable(),
    items: z.array(AgentConversationRuntimeItemSchema),
  })
  .transform((status): AgentConversationRuntimeStatus => ({
    conversationId: status.conversationId,
    isRunning: status.isRunning,
    agentStatus: status.agentStatus,
    primarySource: status.primarySource,
    summaryLabel: status.summaryLabel,
    items: status.items,
  }));

export async function getAgentConversationRuntimeStatuses(
  conversationIds: string[],
): Promise<Record<string, AgentConversationRuntimeStatus>> {
  return typedInvoke(
    "get_agent_conversation_runtime_statuses",
    { conversationIds },
    z.record(z.string(), AgentConversationRuntimeStatusSchema),
  );
}

const AgentConversationRuntimeIndexGroupSchema = z.enum([
  "main",
  "ideation_verification",
  "pipeline",
]);

export type AgentConversationRuntimeIndexGroup = z.infer<
  typeof AgentConversationRuntimeIndexGroupSchema
>;

const AgentConversationRuntimeIndexKindSchema = z.enum([
  "workspace",
  "workspace_review",
  "ideation",
  "verification",
  "delegation",
  "task",
]);

export type AgentConversationRuntimeIndexKind = z.infer<
  typeof AgentConversationRuntimeIndexKindSchema
>;

const AgentConversationRuntimeLifecycleSchema = z.enum([
  "planned",
  "queued",
  "running",
  "waiting",
  "completed",
  "failed",
  "cancelled",
  "blocked",
  "dropped",
]);

export type AgentConversationRuntimeLifecycle = z.infer<
  typeof AgentConversationRuntimeLifecycleSchema
>;

const AgentConversationRuntimeIndexModeSchema = z.enum([
  "chat",
  "agent",
  "plan",
  "pr_review",
  "ideation",
  "automation",
]);

export type AgentConversationRuntimeIndexMode = z.infer<
  typeof AgentConversationRuntimeIndexModeSchema
>;

export interface AgentConversationRuntimeIndexRow {
  id: string;
  group: AgentConversationRuntimeIndexGroup;
  kind: AgentConversationRuntimeIndexKind;
  lifecycle: AgentConversationRuntimeLifecycle;
  statusLabel: string;
  title: string;
  mode: AgentConversationRuntimeIndexMode | null;
  orderIndex: number;
  orderStartedAt: string | null;
  completedAt: string | null;
  conversationId: string | null;
  contextType: ContextType | null;
  contextId: string | null;
  taskId: string | null;
  agentRunId: string | null;
  parentSessionId: string | null;
  childSessionId: string | null;
  providerHarness: string | null;
  providerSessionId: string | null;
  errorMessage: string | null;
}

export interface AgentConversationRuntimeIndexResponse {
  conversationId: string;
  rows: AgentConversationRuntimeIndexRow[];
}

const AgentConversationRuntimeIndexRowSchema = z.object({
  id: z.string(),
  group: AgentConversationRuntimeIndexGroupSchema,
  kind: AgentConversationRuntimeIndexKindSchema,
  lifecycle: AgentConversationRuntimeLifecycleSchema,
  statusLabel: z.string(),
  title: z.string(),
  mode: AgentConversationRuntimeIndexModeSchema.nullable(),
  orderIndex: z.number(),
  orderStartedAt: z.string().nullable(),
  completedAt: z.string().nullable(),
  conversationId: z.string().nullable(),
  contextType: ContextTypeSchema.nullable(),
  contextId: z.string().nullable(),
  taskId: z.string().nullable(),
  agentRunId: z.string().nullable(),
  parentSessionId: z.string().nullable(),
  childSessionId: z.string().nullable(),
  providerHarness: z.string().nullable(),
  providerSessionId: z.string().nullable(),
  errorMessage: z.string().nullable(),
}) satisfies z.ZodType<AgentConversationRuntimeIndexRow>;

const AgentConversationRuntimeIndexResponseSchema = z.object({
  conversationId: z.string(),
  rows: z.array(AgentConversationRuntimeIndexRowSchema),
}) satisfies z.ZodType<AgentConversationRuntimeIndexResponse>;

export async function getAgentConversationRuntimeIndex(
  conversationId: string,
): Promise<AgentConversationRuntimeIndexResponse> {
  return typedInvoke(
    "get_agent_conversation_runtime_index",
    { conversationId },
    AgentConversationRuntimeIndexResponseSchema,
  );
}

export interface BulkPublicationStateResponse {
  publication_state: string;
  publication_label: string | null;
}

export async function getBulkWorkspacePublicationStates(
  conversationIds: string[],
): Promise<Record<string, BulkPublicationStateResponse>> {
  return typedInvoke(
    "get_bulk_workspace_publication_states",
    { conversationIds },
    z.record(
      z.string(),
      z.object({
        publication_state: z.string(),
        publication_label: z.string().nullable(),
      }),
    ),
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
  messageId: string,
): Promise<ChatAttachmentResponse[]> {
  return typedInvoke(
    "list_message_attachments",
    { messageId },
    z.array(ChatAttachmentResponseSchema),
  );
}
