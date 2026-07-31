/**
 * Event type definitions for Tauri IPC communication
 *
 * These types define the structure of events emitted from the Rust backend
 * to the React frontend via Tauri's event system.
 */

import { z } from "zod";
import { InternalStatusSchema } from "./status";
import { TaskSchema } from "./task";
import type { ConversationProviderMetadata } from "./chat-conversation";

// ============================================================================
// Agent Message Events (high frequency)
// ============================================================================

/**
 * Schema for agent activity events
 * Emitted during agent execution for real-time activity streaming
 */
export const AgentMessageEventSchema = z.object({
  taskId: z.string(),
  type: z.enum(["thinking", "tool_call", "tool_result", "text", "error"]),
  content: z.string(),
  timestamp: z.number(),
  metadata: z.record(z.string(), z.unknown()).optional(),
});

export type AgentMessageEvent = z.infer<typeof AgentMessageEventSchema>;

// ============================================================================
// Task Status Events
// ============================================================================

/**
 * Schema for task status changes
 * Emitted when a task transitions between states
 */
export const TaskStatusEventSchema = z.object({
  taskId: z.string(),
  fromStatus: z.string().nullable(),
  toStatus: z.string(),
  changedBy: z.enum(["user", "system", "ai_worker", "ai_reviewer", "ai_supervisor"]),
  reason: z.string().optional(),
});

export type TaskStatusEvent = z.infer<typeof TaskStatusEventSchema>;

/**
 * Schema for legacy task:status_changed events (snake_case payload)
 */
export const TaskStatusChangedEventSchema = z.object({
  task_id: z.string().uuid(),
  old_status: InternalStatusSchema,
  new_status: InternalStatusSchema,
});

export type TaskStatusChangedEvent = z.infer<typeof TaskStatusChangedEventSchema>;

// ============================================================================
// Recovery Prompt Events
// ============================================================================

/**
 * Schema for recovery prompt events
 * Emitted when backend needs user input to resolve ambiguous state
 */
export const RecoveryPromptEventSchema = z.object({
  taskId: z.string().uuid(),
  status: InternalStatusSchema,
  contextType: z.enum(["execution", "review", "merge", "qa_refining", "qa_testing"]),
  reason: z.string(),
  primaryAction: z.object({
    id: z.enum(["restart", "cancel"]),
    label: z.string(),
  }),
  secondaryAction: z.object({
    id: z.enum(["restart", "cancel"]),
    label: z.string(),
  }),
});

export type RecoveryPromptEvent = z.infer<typeof RecoveryPromptEventSchema>;

// ============================================================================
// Supervisor Alert Events
// ============================================================================

/**
 * Schema for supervisor alerts
 * Emitted when the supervisor detects anomalies or issues
 */
export const SupervisorAlertEventSchema = z.object({
  taskId: z.string(),
  severity: z.enum(["low", "medium", "high", "critical"]),
  type: z.enum(["loop_detected", "stuck", "error", "escalation"]),
  message: z.string(),
  suggestedAction: z.string().optional(),
});

export type SupervisorAlertEvent = z.infer<typeof SupervisorAlertEventSchema>;

// ============================================================================
// Review Events
// ============================================================================

/**
 * Schema for review events
 * Emitted during the code review process
 */
export const ReviewEventSchema = z.object({
  taskId: z.string(),
  reviewId: z.string(),
  type: z.enum(["started", "completed", "needs_human", "fix_proposed"]),
  outcome: z.enum(["approved", "changes_requested", "escalated"]).optional(),
});

export type ReviewEvent = z.infer<typeof ReviewEventSchema>;

// ============================================================================
// File Change Events
// ============================================================================

/**
 * Schema for file change events (for diff viewer)
 * Emitted when files are modified by agents
 */
export const FileChangeEventSchema = z.object({
  projectId: z.string(),
  filePath: z.string(),
  changeType: z.enum(["created", "modified", "deleted"]),
});

export type FileChangeEvent = z.infer<typeof FileChangeEventSchema>;

// ============================================================================
// Progress Events
// ============================================================================

/**
 * Schema for progress events
 * Emitted to show task execution progress
 */
export const ProgressEventSchema = z.object({
  taskId: z.string(),
  progress: z.number().min(0).max(100),
  stage: z.string(),
});

export type ProgressEvent = z.infer<typeof ProgressEventSchema>;

// ============================================================================
// TaskEvent Discriminated Union
// ============================================================================

/**
 * Discriminated union for all task-related events
 * Used for Tauri event listening with runtime validation
 */
export const TaskEventSchema = z.discriminatedUnion("type", [
  z.object({
    type: z.literal("created"),
    task: TaskSchema,
  }),
  z.object({
    type: z.literal("updated"),
    taskId: z.string().uuid(),
    changes: TaskSchema.partial(),
  }),
  z.object({
    type: z.literal("deleted"),
    taskId: z.string().uuid(),
  }),
  z.object({
    type: z.literal("status_changed"),
    taskId: z.string().uuid(),
    from: InternalStatusSchema,
    to: InternalStatusSchema,
    changedBy: z.enum(["user", "system", "agent", "auto"]),
  }),
]);

export type TaskEvent = z.infer<typeof TaskEventSchema>;

// ============================================================================
// QA Events
// ============================================================================

/**
 * Schema for QA prep events
 * Emitted when QA preparation phase changes
 */
export const QAPrepEventSchema = z.object({
  taskId: z.string(),
  type: z.enum(["started", "completed", "failed"]),
  agentId: z.string().optional(),
  acceptanceCriteriaCount: z.number().int().nonnegative().optional(),
  testStepsCount: z.number().int().nonnegative().optional(),
  error: z.string().optional(),
});

export type QAPrepEvent = z.infer<typeof QAPrepEventSchema>;

/**
 * Schema for QA test events
 * Emitted during and after QA test execution
 */
export const QATestEventSchema = z.object({
  taskId: z.string(),
  type: z.enum(["started", "passed", "failed"]),
  agentId: z.string().optional(),
  totalSteps: z.number().int().nonnegative().optional(),
  passedSteps: z.number().int().nonnegative().optional(),
  failedSteps: z.number().int().nonnegative().optional(),
  error: z.string().optional(),
});

export type QATestEvent = z.infer<typeof QATestEventSchema>;

// ============================================================================
// Proposal Events
// ============================================================================

/**
 * Schema for task proposal events
 * Emitted when proposals are created, updated, or deleted in ideation sessions
 */
/**
 * Raw proposal schema for events (snake_case from backend)
 */
const ProposalEventPayloadSchema = z.object({
  id: z.string(),
  session_id: z.string(),
  title: z.string(),
  description: z.string().nullable(),
  category: z.string(),
  steps: z.array(z.string()),
  acceptance_criteria: z.array(z.string()),
  suggested_priority: z.string(),
  priority_score: z.number(),
  priority_reason: z.string().nullable(),
  estimated_complexity: z.string(),
  user_priority: z.string().nullable(),
  user_modified: z.boolean(),
  status: z.string(),
  created_task_id: z.string().nullable(),
  plan_artifact_id: z.string().nullable(),
  plan_version_at_creation: z.number().nullable(),
  sort_order: z.number(),
  created_at: z.string(),
  updated_at: z.string(),
});

export const ProposalEventSchema = z.discriminatedUnion("type", [
  z.object({
    type: z.literal("created"),
    proposal: ProposalEventPayloadSchema,
  }),
  z.object({
    type: z.literal("updated"),
    proposal: ProposalEventPayloadSchema,
  }),
  z.object({
    type: z.literal("deleted"),
    proposalId: z.string(),
  }),
]);

export type ProposalEvent = z.infer<typeof ProposalEventSchema>;

/**
 * Schema for proposals:reordered event
 * Emitted when proposals are reordered within a session
 */
export const ProposalsReorderedEventSchema = z.object({
  session_id: z.string(),
  proposals: z.array(ProposalEventPayloadSchema),
});

export type ProposalsReorderedEvent = z.infer<typeof ProposalsReorderedEventSchema>;

// ============================================================================
// Plan Artifact Events
// ============================================================================

/**
 * Schema for plan artifact events
 * Emitted when plan artifacts are created or updated in ideation sessions
 */
export const PlanArtifactEventSchema = z.discriminatedUnion("type", [
  z.object({
    type: z.literal("created"),
    sessionId: z.string(),
    artifact: z.object({
      id: z.string(),
      name: z.string(),
      content: z.string(),
      version: z.number(),
    }),
  }),
  z.object({
    type: z.literal("updated"),
    sessionId: z.string().nullable().optional(),
    artifactId: z.string(),
    previousArtifactId: z.string(),
    artifact: z.object({
      id: z.string(),
      name: z.string(),
      content: z.string(),
      version: z.number(),
    }),
  }),
]);

export type PlanArtifactEvent = z.infer<typeof PlanArtifactEventSchema>;

// ============================================================================
// Plan Verification Events
// ============================================================================

/**
 * Gap entry in plan_verification:status_changed event payload (snake_case from Rust serde)
 */
export const EventVerificationGapSchema = z.object({
  severity: z.enum(["critical", "high", "medium", "low"]),
  category: z.string(),
  description: z.string(),
  why_it_matters: z.string().nullable().optional(),
});

export type EventVerificationGap = z.infer<typeof EventVerificationGapSchema>;

/**
 * Round entry in plan_verification:status_changed event payload (snake_case from Rust serde)
 */
export const EventRoundSummarySchema = z.object({
  fingerprints: z.array(z.string()),
  gap_score: z.number(),
});

export type EventRoundSummary = z.infer<typeof EventRoundSummarySchema>;

/**
 * Schema for plan_verification:status_changed events (snake_case — backend emits via serde_json)
 */
export const PlanVerificationStatusChangedSchema = z.object({
  session_id: z.string(),
  // "imported_verified" is included defensively — backend currently skips event emission for ImportedVerified,
  // but the schema must accept it if that changes.
  status: z.enum(["unverified", "reviewing", "verified", "needs_revision", "skipped", "imported_verified"]),
  in_progress: z.boolean(),
  generation: z.number().int().nullable().optional(),
  round: z.number().int().nullable().optional(),
  max_rounds: z.number().int().nullable().optional(),
  gap_score: z.number().int().nullable().optional(),
  convergence_reason: z.string().nullable().optional(),
  // Extended payload (B1): fast-path data for setQueryData cache update
  current_gaps: z.array(EventVerificationGapSchema).optional(),
  rounds: z.array(EventRoundSummarySchema).optional(),
});

export type PlanVerificationStatusChangedPayload = z.infer<typeof PlanVerificationStatusChangedSchema>;

/** Mapped camelCase view of the payload for consumers */
export type PlanVerificationStatusChangedEvent = {
  sessionId: string;
  status: PlanVerificationStatusChangedPayload["status"];
  inProgress: boolean;
  generation?: number;
  round?: number;
  maxRounds?: number;
  gapScore?: number;
  convergenceReason?: string;
  currentGaps?: EventVerificationGap[];
  rounds?: EventRoundSummary[];
};

// ============================================================================
// Merge Validation Events
// ============================================================================

/**
 * Schema for merge validation step events
 * Emitted during post-merge validation for real-time progress streaming
 * Also used for pre-execution setup/install progress
 */
export const MergeValidationStepEventSchema = z.object({
  task_id: z.string(),
  phase: z.enum(["setup", "validate", "install"]),
  command: z.string(),
  path: z.string(),
  label: z.string(),
  status: z.enum(["running", "success", "failed", "cached", "skipped"]),
  exit_code: z.number().nullable().optional(),
  stdout: z.string().optional(),
  stderr: z.string().optional(),
  duration_ms: z.number().optional(),
  context: z.enum(["merge", "execution", "review"]).optional(),
});

export type MergeValidationStepEvent = z.infer<typeof MergeValidationStepEventSchema>;

// ============================================================================
// Task Validation Events
// ============================================================================

export const TaskValidationEventSchema = z.object({
  type: z.enum([
    "run_started",
    "command_started",
    "command_output",
    "command_completed",
    "run_completed",
  ]),
  task_id: z.string(),
  project_id: z.string(),
  run_id: z.string(),
  status: z.enum([
    "running",
    "passed",
    "failed",
    "error",
    "cancelled",
    "skipped",
    "cached",
  ]),
  purpose: z.string(),
  context_type: z.string(),
  mode: z.string(),
  policy_enabled: z.boolean(),
  run_started_at: z.string(),
  run_completed_at: z.string().optional(),
  requested_by_agent: z.string().optional(),
  head_sha: z.string().optional(),
  head_short_sha: z.string().optional(),
  base_ref: z.string().optional(),
  current_for_head: z.boolean().optional(),
  current_for_execution_episode: z.boolean().optional(),
  review_evidence_eligible: z.boolean().optional(),
  ineligible_reason: z.string().nullable().optional(),
  command_id: z.string().optional(),
  command_source: z.string().optional(),
  command_ref: z.string().optional(),
  command: z.string().optional(),
  cwd: z.string().optional(),
  label: z.string().optional(),
  category: z.string().optional(),
  reason: z.string().optional(),
  cache_decision: z.enum(["ran", "cached", "stale", "forced", "skipped"]).optional(),
  exit_code: z.number().nullable().optional(),
  duration_ms: z.number().nullable().optional(),
  stream: z.enum(["stdout", "stderr"]).optional(),
  stdout_delta: z.string().optional(),
  stderr_delta: z.string().optional(),
  stdout_snippet: z.string().optional(),
  stderr_snippet: z.string().optional(),
  stdout_log_path: z.string().optional(),
  stderr_log_path: z.string().optional(),
  command_started_at: z.string().optional(),
  command_completed_at: z.string().optional(),
  emitted_at: z.string(),
});

export type TaskValidationEvent = z.infer<typeof TaskValidationEventSchema>;

// ============================================================================
// Merge Progress Events (high-level phases)
// ============================================================================

/**
 * Schema for high-level merge progress events
 * Emitted during merge/validation to provide user-friendly phase-level updates.
 * Phase is a dynamic string ID (not a fixed enum) — derived from project analysis.
 */
export const MergeProgressEventSchema = z.object({
  task_id: z.string(),
  phase: z.string().min(1),
  status: z.enum(["started", "passed", "failed", "skipped"]),
  message: z.string(),
  timestamp: z.string(),
});

export type MergeProgressEvent = z.infer<typeof MergeProgressEventSchema>;

/** Single phase definition in the dynamic phase list */
export const MergePhaseInfoSchema = z.object({
  id: z.string().min(1),
  label: z.string().min(1),
  /** Actual shell command (only set for dynamic/validation phases) */
  command: z.string().optional(),
  /** Static description (only set for structural phases) */
  description: z.string().optional(),
});

export type MergePhaseInfo = z.infer<typeof MergePhaseInfoSchema>;

/** Schema for the task:merge_phases event — emitted at start of validation */
export const MergePhaseListEventSchema = z.object({
  task_id: z.string(),
  phases: z.array(MergePhaseInfoSchema),
});

export type MergePhaseListEvent = z.infer<typeof MergePhaseListEventSchema>;

// ============================================================================
// Agent Run Lifecycle Payloads
// ============================================================================

/**
 * Payload emitted with the agent:run_started Tauri event.
 * Canonical wire fields are snake_case to match the backend event serializer.
 * CamelCase aliases stay optional here only as a temporary compatibility read path.
 * effective_model_id and effective_model_label are optional — present only when
 * the backend captured the resolved model at spawn time.
 */
export interface AgentRunStartedPayload {
  run_id: string;
  context_type: string;
  context_id: string;
  conversation_id: string;
  started_at?: string | null;
  agent_name?: string | null;
  launch_role?: string | null;
  effective_model_id?: string;
  effective_model_label?: string;
  provider_harness?: string | null;
  provider_session_id?: string | null;
  service_tier?: string | null;
  startedAt?: string | null;
  agentName?: string | null;
  launchRole?: string | null;
  effectiveModelId?: string;
  effectiveModelLabel?: string;
  providerHarness?: string | null;
  providerSessionId?: string | null;
  serviceTier?: string | null;
}

/**
 * Payload emitted with the agent:run_completed and agent:turn_completed Tauri events.
 * Fields remain snake_case to match the backend serializer for these payloads.
 */
export interface AgentRunCompletedPayload {
  run_id?: string | null;
  agent_run_id?: string | null;
  conversation_id: string;
  context_type: string;
  context_id: string;
  claude_session_id?: string | null;
  provider_harness?: string | null;
  provider_session_id?: string | null;
  service_tier?: string | null;
  run_chain_id?: string | null;
}

export function extractConversationProviderMetadataFromRunPayload(
  payload: AgentRunStartedPayload | AgentRunCompletedPayload
): ConversationProviderMetadata {
  if ("provider_harness" in payload || "provider_session_id" in payload) {
    const snakePayload = payload as AgentRunStartedPayload | AgentRunCompletedPayload;
    return {
      providerHarness:
        snakePayload.provider_harness ??
        ("providerHarness" in snakePayload ? snakePayload.providerHarness : undefined) ??
        undefined,
      providerSessionId:
        snakePayload.provider_session_id ??
        ("providerSessionId" in snakePayload ? snakePayload.providerSessionId : undefined) ??
        undefined,
      serviceTier:
        snakePayload.service_tier ??
        ("serviceTier" in snakePayload ? snakePayload.serviceTier : undefined) ??
        undefined,
    };
  }

  if ("providerHarness" in payload || "providerSessionId" in payload) {
    const startedPayload = payload as AgentRunStartedPayload;
    return {
      providerHarness: startedPayload.providerHarness ?? undefined,
      providerSessionId: startedPayload.providerSessionId ?? undefined,
      serviceTier: startedPayload.serviceTier ?? undefined,
    };
  }

  const completedPayload = payload as AgentRunCompletedPayload;

  return {
    claudeSessionId: completedPayload.claude_session_id ?? undefined,
    providerHarness: completedPayload.provider_harness ?? undefined,
    providerSessionId: completedPayload.provider_session_id ?? undefined,
    serviceTier: completedPayload.service_tier ?? undefined,
  };
}
