// Context-aware chat conversation types and Zod schemas
// Types for ChatConversation, AgentRun, and related structures

import { z } from "zod";

// ============================================================================
// Model Display
// ============================================================================

/** Effective model display info (id + human-readable label). Transient — not persisted. */
export type ModelDisplay = { id: string; label: string };

// ============================================================================
// Context Type
// ============================================================================

/**
 * Context type for chat conversations
 */
export const CONTEXT_TYPE_VALUES = [
  "ideation",
  "task",
  "project",
  "task_execution",
  "review",
  "merge",
  "delegation",
] as const;

export const ContextTypeSchema = z.enum(CONTEXT_TYPE_VALUES);
export type ContextType = z.infer<typeof ContextTypeSchema>;

// ============================================================================
// Agent Run Status
// ============================================================================

/**
 * Status values for agent runs
 */
export const AGENT_RUN_STATUS_VALUES = [
  "running",
  "completed",
  "failed",
  "cancelled",
] as const;

export const AgentRunStatusSchema = z.enum(AGENT_RUN_STATUS_VALUES);
export type AgentRunStatus = z.infer<typeof AgentRunStatusSchema>;
export const ProviderHarnessSchema = z.string().min(1);
export type ProviderHarness = z.infer<typeof ProviderHarnessSchema>;

export type ConversationProviderMetadata = {
  claudeSessionId?: string | null | undefined;
  providerSessionId?: string | null | undefined;
  providerHarness?: ProviderHarness | null | undefined;
};

export const AGENT_CONVERSATION_MODE_VALUES = [
  "chat",
  "edit",
  "ideation",
] as const;
export const AgentConversationModeSchema = z.enum(
  AGENT_CONVERSATION_MODE_VALUES
);
export type AgentConversationMode = z.infer<typeof AgentConversationModeSchema>;

// ============================================================================
// Chat Conversation
// ============================================================================

/**
 * Chat conversation schema matching Rust backend serialization
 */
export const ChatConversationSchema = z.object({
  id: z.string().min(1),
  contextType: ContextTypeSchema,
  contextId: z.string().min(1),
  claudeSessionId: z.string().nullable().optional(),
  providerSessionId: z.string().nullable(),
  providerHarness: ProviderHarnessSchema.nullable(),
  upstreamProvider: z.string().nullable().optional(),
  providerProfile: z.string().nullable().optional(),
  agentMode: AgentConversationModeSchema.nullable().optional(),
  title: z.string().nullable(),
  messageCount: z.number().int().min(0),
  lastMessageAt: z.string().datetime().nullable(),
  createdAt: z.string().datetime(),
  updatedAt: z.string().datetime(),
  archivedAt: z.string().datetime().nullable().optional(),
});

export type ChatConversation = z.infer<typeof ChatConversationSchema>;

export function normalizeConversationProviderMetadata(
  metadata: ConversationProviderMetadata
): Pick<
  ChatConversation,
  "claudeSessionId" | "providerSessionId" | "providerHarness"
> {
  const providerSessionId =
    metadata.providerSessionId ?? metadata.claudeSessionId ?? null;
  const providerHarness =
    metadata.providerHarness ?? (metadata.claudeSessionId ? "claude" : null);
  const claudeSessionId =
    metadata.claudeSessionId ??
    (providerHarness === "claude" ? providerSessionId : null);

  return {
    claudeSessionId,
    providerSessionId,
    providerHarness,
  };
}

export function mergeConversationProviderMetadata(
  conversation: ChatConversation,
  metadata: ConversationProviderMetadata
): ChatConversation {
  const providerHarness =
    metadata.providerHarness !== undefined
      ? metadata.providerHarness
      : conversation.providerHarness;
  const providerSessionId =
    metadata.providerSessionId !== undefined
      ? metadata.providerSessionId
      : metadata.claudeSessionId !== undefined
        ? metadata.claudeSessionId
        : conversation.providerSessionId;

  const claudeSessionId =
    metadata.claudeSessionId !== undefined
      ? metadata.claudeSessionId
      : metadata.providerHarness !== undefined
        ? metadata.providerHarness === "claude"
          ? (providerSessionId ?? conversation.claudeSessionId ?? null)
          : metadata.providerHarness === null
            ? (conversation.claudeSessionId ?? null)
            : null
        : metadata.providerSessionId !== undefined && providerHarness === "claude"
          ? metadata.providerSessionId
          : conversation.claudeSessionId;

  return {
    ...conversation,
    ...normalizeConversationProviderMetadata({
      claudeSessionId,
      providerSessionId,
      providerHarness,
    }),
  };
}

// ============================================================================
// Agent Run
// ============================================================================

/**
 * Agent run schema matching Rust backend serialization
 */
export const AgentRunSchema = z.object({
  id: z.string().min(1),
  conversationId: z.string().min(1),
  status: AgentRunStatusSchema,
  startedAt: z.string().datetime(),
  completedAt: z.string().datetime().nullable(),
  errorMessage: z.string().nullable(),
  modelId: z.string().nullable(),
  modelLabel: z.string().nullable(),
});

export type AgentRun = z.infer<typeof AgentRunSchema>;

// ============================================================================
// Tool Call (for display in chat UI)
// ============================================================================

/**
 * Tool call schema (parsed from JSON in message.toolCalls)
 * Fields support lifecycle tracking: started → completed → result
 */
export const ToolCallSchema = z.object({
  id: z.string().min(1),
  name: z.string().min(1),
  arguments: z.unknown(),
  result: z.unknown().nullable(),
  error: z.string().nullable(),
});

export type ToolCall = z.infer<typeof ToolCallSchema>;

// ============================================================================
// Queued Message (frontend-only state)
// ============================================================================

/**
 * Queued message schema (not persisted, frontend state only)
 */
export const QueuedMessageSchema = z.object({
  id: z.string().min(1),
  content: z.string().min(1),
  createdAt: z.string().datetime(),
  isEditing: z.boolean(),
});

export type QueuedMessage = z.infer<typeof QueuedMessageSchema>;

// ============================================================================
// Input Schemas (for API calls)
// ============================================================================

/**
 * Input for creating a new conversation
 */
export const CreateConversationInputSchema = z.object({
  contextType: ContextTypeSchema,
  contextId: z.string().min(1, "Context ID is required"),
  title: z.string().optional(),
});

export type CreateConversationInput = z.infer<
  typeof CreateConversationInputSchema
>;

/**
 * Input for sending a context-aware message
 */
export const SendContextMessageInputSchema = z.object({
  contextType: ContextTypeSchema,
  contextId: z.string().min(1, "Context ID is required"),
  content: z.string().min(1, "Message content is required"),
  conversationId: z.string().optional(),
});

export type SendContextMessageInput = z.infer<
  typeof SendContextMessageInputSchema
>;

// ============================================================================
// List Schemas
// ============================================================================

export const ChatConversationListSchema = z.array(ChatConversationSchema);
export type ChatConversationList = z.infer<typeof ChatConversationListSchema>;

export const AgentRunListSchema = z.array(AgentRunSchema);
export type AgentRunList = z.infer<typeof AgentRunListSchema>;

export const ToolCallListSchema = z.array(ToolCallSchema);
export type ToolCallList = z.infer<typeof ToolCallListSchema>;

// ============================================================================
// SendContextMessage Response
// ============================================================================

/**
 * Tool call in the response from send_context_message
 */
export const ToolCallResponseSchema = z.object({
  id: z.string().nullable(),
  name: z.string().min(1),
  arguments: z.unknown(),
  result: z.unknown().nullable(),
});

export type ToolCallResponse = z.infer<typeof ToolCallResponseSchema>;

/**
 * Response from send_context_message Tauri command
 */
export const SendContextMessageResponseSchema = z.object({
  response_text: z.string(),
  tool_calls: z.array(ToolCallResponseSchema),
  claude_session_id: z.string().nullable().optional(),
  provider_session_id: z.string().nullable(),
  provider_harness: ProviderHarnessSchema.nullable(),
  conversation_id: z.string().nullable(),
});

export type SendContextMessageResponse = z.infer<
  typeof SendContextMessageResponseSchema
>;
