/**
 * StreamingTask - Represents a subagent Task tool call during streaming.
 *
 * Links child tool calls (via parentToolUseId) to a parent Task invocation.
 * Used to group subagent work in the streaming chat UI.
 *
 * StreamingContentBlock - Represents a discrete chunk of content during streaming.
 * Used to preserve the natural interleaving of text and tool calls — when the
 * assistant writes text → calls a tool → writes more text, each segment renders
 * as a separate content block in the correct order.
 */

import type { ToolCall } from "@/components/Chat/ToolCallIndicator";

export type StreamingTaskStatus = "running" | "completed" | "failed";
export type StreamingTaskProviderStatus = StreamingTaskStatus | "cancelled";

/**
 * StreamingContentBlock - Discriminated union representing chunks of streaming content.
 * Allows text and tool calls to be interleaved in the order they arrive from the agent.
 *
 * The `task` variant is a position marker only — it records WHERE in the stream a Task
 * tool call appeared. Actual task metadata is read from `streamingTasks` Map via
 * toolUseId lookup. This preserves all existing StreamingTask behavior (status updates,
 * child tool calls) while rendering the card at its chronological position.
 *
 * `seq` preserves provider stream order; `receivedAt` is UI-local wall-clock
 * order for interleaving live rows with user messages sent during streaming.
 */
export type StreamingContentBlock =
  | { type: "text"; text: string; blockIndex?: number; seq?: number; receivedAt?: number }
  | { type: "thinking"; text: string; blockIndex?: number; durationMs?: number; isSettled?: boolean; seq?: number; receivedAt?: number }
  | { type: "tool_use"; toolCall: ToolCall; seq?: number; receivedAt?: number }
  | { type: "task"; toolUseId: string; seq?: number; receivedAt?: number };

export interface StreamingTask {
  /** The Task tool_use.id — links child tool calls via parentToolUseId */
  toolUseId: string;
  /** Tool name that triggered this: "Task" or "Agent" */
  toolName: string;
  /** From Task input.description */
  description: string;
  /** Subagent type: "Explore", "Plan", "Bash", etc. */
  subagentType: string;
  /** Model used: "sonnet", "opus", "haiku", "fable" */
  model: string;
  /** Current status */
  status: StreamingTaskProviderStatus;
  /** Backend-owned start timestamp when available; otherwise an explicit local fallback. */
  startedAt: number;
  /** Timestamp when the task completed */
  completedAt?: number;
  /** Source of the timestamps, so recovered cards never imply a local mount clock is backend time. */
  clockSource?: "delegated-run" | "delegation-job" | "local-fallback";
  /** Total duration in milliseconds (from task result) */
  totalDurationMs?: number;
  /** Total tokens used (from task result) */
  totalTokens?: number;
  /** Total tool use count (from task result) */
  totalToolUseCount?: number;
  /** Agent ID (from task result) */
  agentId?: string;
  /** RalphX native delegation job id */
  delegatedJobId?: string;
  /** Delegated session id backing the child runtime */
  delegatedSessionId?: string;
  /** Delegated conversation id for child transcript expansion */
  delegatedConversationId?: string;
  /** Delegated agent run id for latest child run attribution */
  delegatedAgentRunId?: string;
  /** Delegated harness/provider */
  providerHarness?: string;
  /** Delegated provider session continuity id */
  providerSessionId?: string;
  /** Upstream provider captured by the delegated run */
  upstreamProvider?: string;
  /** Provider profile captured by the delegated run */
  providerProfile?: string;
  /** Logical model requested */
  logicalModel?: string;
  /** Effective model used by the harness */
  effectiveModelId?: string;
  /** Logical effort requested */
  logicalEffort?: string;
  /** Effective effort used by the harness */
  effectiveEffort?: string;
  /** Approval policy used by the delegated run */
  approvalPolicy?: string;
  /** Sandbox mode used by the delegated run */
  sandboxMode?: string;
  /** Estimated USD cost for the latest run */
  estimatedUsd?: number;
  /** Input tokens used by the latest run */
  inputTokens?: number;
  /** Output tokens used by the latest run */
  outputTokens?: number;
  /** Cache creation tokens used by the latest run */
  cacheCreationTokens?: number;
  /** Cache read tokens used by the latest run */
  cacheReadTokens?: number;
  /** Final delegated output when available */
  textOutput?: string;
  /** Tool calls made by this subagent (matched by parentToolUseId) */
  childToolCalls: ToolCall[];
  /** Monotonically increasing sequence number for cross-event-type ordering */
  seq?: number;
  /** Authority that supplied the current terminal delegation state. */
  delegationTerminalSource?: "provider" | "lifecycle-complete" | "active-state";
}
