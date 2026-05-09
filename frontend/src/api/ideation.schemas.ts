// Zod schemas for ideation API responses (snake_case from Rust backend)

import { z } from "zod";
import { VerificationGapSchema } from "../types/ideation";

/**
 * Ideation session response schema (snake_case from Rust)
 */
export const IdeationSessionResponseSchema = z.object({
  id: z.string(),
  project_id: z.string(),
  title: z.string().nullable(),
  title_source: z.enum(["auto", "user"]).nullable().optional(),
  status: z.string(),
  plan_artifact_id: z.string().nullable(),
  seed_task_id: z.string().nullable().optional(),
  parent_session_id: z.string().nullable(),
  team_mode: z.enum(["solo", "research", "debate"]).nullable().optional(),
  team_config: z.object({
    max_teammates: z.number(),
    model_ceiling: z.string(),
    budget_limit: z.number().nullable().optional(),
    composition_mode: z.string().nullable().optional(),
  }).nullable().optional(),
  created_at: z.string(),
  updated_at: z.string(),
  archived_at: z.string().nullable(),
  converted_at: z.string().nullable(),
  verification_status: z.string().optional(),
  verification_in_progress: z.boolean().optional(),
  gap_score: z.number().int().nullable().optional(),
  source_project_id: z.string().nullable().optional(),
  source_session_id: z.string().nullable().optional(),
  source_task_id: z.string().nullable().optional(),
  source_context_type: z.string().nullable().optional(),
  source_context_id: z.string().nullable().optional(),
  spawn_reason: z.string().nullable().optional(),
  blocker_fingerprint: z.string().nullable().optional(),
  inherited_plan_artifact_id: z.string().nullable().optional(),
  session_purpose: z.enum(["general", "verification"]).optional(),
  acceptance_status: z.enum(["pending", "accepted", "rejected"]).nullable().optional(),
  analysis_base_ref_kind: z.enum(["project_default", "current_branch", "local_branch", "pull_request"]).nullable().optional(),
  analysis_base_ref: z.string().nullable().optional(),
  analysis_base_display_name: z.string().nullable().optional(),
  analysis_workspace_kind: z.enum(["project_root", "ideation_worktree"]).optional(),
  analysis_workspace_path: z.string().nullable().optional(),
  analysis_base_commit: z.string().nullable().optional(),
  analysis_base_locked_at: z.string().nullable().optional(),
  last_effective_model: z.string().nullable().optional(),
});

export const SessionProgressResponseSchema = z.object({
  idle: z.number(),
  active: z.number(),
  done: z.number(),
  total: z.number(),
});

export const IdeationSessionWithProgressResponseSchema =
  IdeationSessionResponseSchema.extend({
    progress: SessionProgressResponseSchema.nullable(),
    parentSessionTitle: z.string().nullable(),
    verificationChildCount: z.number(),
    hasPendingPrompt: z.boolean(),
  });

export const SessionListResponseSchema = z.object({
  sessions: z.array(IdeationSessionWithProgressResponseSchema),
  total: z.number(),
  hasMore: z.boolean(),
  offset: z.number(),
});

/**
 * API gap schema (snake_case from HTTP server) — transforms to VerificationGap shape.
 * Reuses VerificationGapSchema as the output type (single source of truth).
 */
export const ApiVerificationGapSchema = z.object({
  severity: z.enum(["critical", "high", "medium", "low"]),
  category: z.string(),
  description: z.string(),
  why_it_matters: z.string().optional(),
}).transform((val): z.infer<typeof VerificationGapSchema> => ({
  severity: val.severity,
  category: val.category,
  description: val.description,
  ...(val.why_it_matters !== undefined && { whyItMatters: val.why_it_matters }),
}));

/**
 * API round summary schema (snake_case from HTTP server) — transforms to RoundSummary shape.
 */
export const ApiRoundSummarySchema = z.object({
  round: z.number(),
  gap_score: z.number(),
  gap_count: z.number(),
}).transform((val) => ({
  round: val.round,
  gapScore: val.gap_score,
  gapCount: val.gap_count,
}));

export const ApiRoundDetailSchema = z.object({
  round: z.number(),
  gap_score: z.number(),
  gap_count: z.number(),
  gaps: z.array(ApiVerificationGapSchema).optional().default([]),
}).transform((val) => ({
  round: val.round,
  gapScore: val.gap_score,
  gapCount: val.gap_count,
  gaps: val.gaps,
}));

export const ApiVerificationRunHistoryEntrySchema = z.object({
  generation: z.number().int(),
  status: z.string(),
  in_progress: z.boolean(),
  current_round: z.number().int().optional(),
  max_rounds: z.number().int().optional(),
  round_count: z.number().int(),
  gap_count: z.number().int(),
  gap_score: z.number().int().optional(),
  convergence_reason: z.string().optional(),
}).transform((val) => ({
  generation: val.generation,
  status: val.status,
  inProgress: val.in_progress,
  ...(val.current_round !== undefined && { currentRound: val.current_round }),
  ...(val.max_rounds !== undefined && { maxRounds: val.max_rounds }),
  roundCount: val.round_count,
  gapCount: val.gap_count,
  ...(val.gap_score !== undefined && { gapScore: val.gap_score }),
  ...(val.convergence_reason !== undefined && { convergenceReason: val.convergence_reason }),
}));

/**
 * Verification status response schema (snake_case from HTTP server)
 */
export const VerificationResponseSchema = z.object({
  session_id: z.string(),
  status: z.string(),
  in_progress: z.boolean(),
  current_round: z.number().int().optional(),
  max_rounds: z.number().int().optional(),
  gap_score: z.number().int().optional(),
  convergence_reason: z.string().optional(),
  best_round_index: z.number().int().optional(),
  current_gaps: z.array(ApiVerificationGapSchema).optional().default([]),
  rounds: z.array(ApiRoundSummarySchema).optional().default([]),
  round_details: z.array(ApiRoundDetailSchema).optional().default([]),
  plan_version: z.number().int().optional(),
  verification_generation: z.number().int(),
  selected_generation: z.number().int(),
  run_history: z.array(ApiVerificationRunHistoryEntrySchema).optional().default([]),
  verification_child: z.object({
    activeChildSessionId: z.string().optional(),
    latestChildSessionId: z.string().optional(),
    latestChildArchived: z.boolean().optional(),
    latestChildUpdatedAt: z.string().optional(),
    agentState: z.string().optional(),
    lastAssistantMessage: z.string().nullable().optional(),
    lastAssistantMessageAt: z.string().nullable().optional(),
  }).optional(),
});

/**
 * Task proposal response schema (snake_case from Rust)
 */
export const TaskProposalResponseSchema = z.object({
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

/**
 * Chat message response schema (snake_case from Rust)
 */
export const ChatMessageResponseSchema = z.object({
  id: z.string(),
  session_id: z.string().nullable(),
  project_id: z.string().nullable(),
  task_id: z.string().nullable(),
  role: z.string(),
  content: z.string(),
  metadata: z.string().nullable(),
  tool_calls: z.string().nullable(),
  parent_message_id: z.string().nullable(),
  created_at: z.string(),
});

/**
 * Session with proposals and messages (snake_case from Rust)
 */
export const SessionWithDataResponseSchema = z.object({
  session: IdeationSessionResponseSchema,
  proposals: z.array(TaskProposalResponseSchema),
  messages: z.array(ChatMessageResponseSchema),
});

/**
 * Priority assessment response (snake_case from Rust)
 */
export const PriorityAssessmentResponseSchema = z.object({
  proposal_id: z.string(),
  priority: z.string(),
  score: z.number(),
  reason: z.string(),
});

/**
 * Dependency graph node response (snake_case from Rust)
 */
export const DependencyGraphNodeResponseSchema = z.object({
  proposal_id: z.string(),
  title: z.string(),
  in_degree: z.number(),
  out_degree: z.number(),
});

/**
 * Dependency graph edge response (snake_case from Rust)
 */
export const DependencyGraphEdgeResponseSchema = z.object({
  from: z.string(),
  to: z.string(),
  reason: z.string().nullable().optional(),
});

/**
 * Dependency analysis summary (snake_case from Rust)
 */
export const DependencyAnalysisSummarySchema = z.object({
  total_proposals: z.number(),
  root_count: z.number(),
  leaf_count: z.number(),
  max_depth: z.number(),
});

/**
 * Dependency graph response (snake_case from Rust)
 */
export const DependencyGraphResponseSchema = z.object({
  nodes: z.array(DependencyGraphNodeResponseSchema),
  edges: z.array(DependencyGraphEdgeResponseSchema),
  critical_path: z.array(z.string()),
  has_cycles: z.boolean(),
  cycles: z.array(z.array(z.string())).nullable(),
  message: z.string().nullable().optional(),
  summary: DependencyAnalysisSummarySchema.nullable().optional(),
});

/**
 * Apply proposals result response (snake_case from Rust)
 */
export const ApplyProposalsResultResponseSchema = z.object({
  created_task_ids: z.array(z.string()),
  dependencies_created: z.number(),
  tasks_created: z.number().optional(),
  warnings: z.array(z.string()),
  session_converted: z.boolean(),
  execution_plan_id: z.string().nullable().optional(),
  message: z.string().nullable().optional(),
});

/**
 * Parent session context response (snake_case from Rust)
 */
export const ParentSessionContextResponseSchema = z.object({
  parent_session: z.object({
    id: z.string(),
    title: z.string().nullable(),
    status: z.string(),
  }),
  plan_content: z.string().nullable(),
  proposals: z.array(
    z.object({
      id: z.string(),
      title: z.string(),
      category: z.string(),
      priority: z.string().nullable(),
      status: z.string(),
      acceptance_criteria: z.array(z.string()),
    })
  ),
});

/**
 * Team mode and config schemas (snake_case from Rust)
 */
export const TeamModeResponseSchema = z.enum(["solo", "research", "debate"]);
export const CompositionModeResponseSchema = z.enum(["dynamic", "constrained"]);
export const TeamConfigResponseSchema = z.object({
  max_teammates: z.number(),
  model_ceiling: z.string(),
  budget_limit: z.number().nullable().optional(),
  composition_mode: z.string().nullable().optional(),
});

/**
 * Create child session response (snake_case from Rust)
 */
export const CreateChildSessionResponseSchema = z.object({
  session_id: z.string(),
  parent_session_id: z.string(),
  title: z.string().nullable(),
  status: z.string(),
  created_at: z.string(),
  generation: z.number().optional(),
  parent_context: ParentSessionContextResponseSchema.optional(),
});

export const LatestChildSessionIdResponseSchema = z.object({
  session_id: z.string(),
  purpose: z.enum(["general", "verification"]).nullable().optional(),
  latest_child_session_id: z.string().nullable(),
});
