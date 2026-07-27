// Zod schemas for ideation API responses (snake_case from Rust backend)

import { z } from "zod";

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
  session_flow: z.enum(["ideation", "planning"]).optional(),
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
 * Verification status response schema (snake_case from HTTP server)
 */
export const VerificationResponseSchema = z.object({
  session_id: z.string(),
  status: z.enum(["unverified", "queued", "verifying", "verified", "failed", "cancelled"]),
  in_progress: z.boolean(),
  plan_artifact_id: z.string().nullable(),
  verified_plan_artifact_id: z.string().nullable(),
  agent_run_id: z.string().nullable(),
  started_at: z.string().nullable(),
  completed_at: z.string().nullable(),
  error: z.string().nullable(),
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
  blueprint_artifact_id: z.string().nullable().optional(),
  blueprint_version_at_creation: z.number().nullable().optional(),
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

export const RestartImplementationResultResponseSchema = z.object({
  session_id: z.string(),
  old_execution_plan_id: z.string(),
  execution_plan_id: z.string(),
  archived_task_count: z.number(),
  created_task_ids: z.array(z.string()),
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
