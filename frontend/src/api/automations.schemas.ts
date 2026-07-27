import { z } from "zod";

export const AutomationStatusSchema = z.enum([
  "draft",
  "active",
  "paused",
  "completed",
  "stopped",
]);

export const AutomationRunStatusSchema = z.enum([
  "pending",
  "provisioning",
  "running",
  "awaiting_plan_approval",
  "published",
  "completed",
  "merged",
  "pr_closed",
  "agent_failed",
  "cancelled",
]);

export const AutomationJudgeStateSchema = z.enum([
  "none",
  "in_progress",
  "done",
  "failed",
  "skipped",
]);

export const AutomationPlanApprovalModeSchema = z.enum(["manual", "automatic"]);

export const AutomationPrMergeModeSchema = z.enum(["manual", "automatic"]);

export const AutomationPlanJudgeStateSchema = z.enum([
  "none",
  "in_progress",
  "done",
  "failed",
]);

export const AutomationPromptAuthorSchema = z.enum([
  "setup_agent",
  "judge",
  "skip_judge_template",
]);

export const AutomationRunModeSchema = z.enum(["edit", "plan", "ideation"]);

export const AutomationBaseRefKindSchema = z.enum([
  "project_default",
  "current_branch",
  "local_branch",
  "pull_request",
]);

export const AutomationChainModeSchema = z.enum([
  "merged_base",
  "pr_head_stacked",
]);

export const AutomationCompletionSignalSchema = z.enum([
  "pr_merged",
  "agent_completed",
  "ideation_finalized",
]);

export const AutomationAuthoringModeSchema = z.enum([
  "reviewed",
  "trusted_auto_finalize",
]);

export const AutomationDecompositionVerificationStatusSchema = z.enum([
  "unverified",
  "verified",
  "needs_revision",
  "failed",
]);

export const AutomationSchema = z.object({
  id: z.string(),
  project_id: z.string(),
  name: z.string(),
  status: AutomationStatusSchema,
  paused_reason_code: z.string().nullable(),
  paused_reason_detail: z.string().nullable(),
  goal_prompt: z.string(),
  setup_conversation_id: z.string().nullable(),
  spec_artifact_id: z.string().nullable(),
  authoring_mode: AutomationAuthoringModeSchema.default("reviewed"),
  decomposition_verification_status:
    AutomationDecompositionVerificationStatusSchema.default("unverified"),
  decomposition_verification_verdict_json: z.string().nullable().default(null),
  provider_harness: z.string(),
  model_id: z.string(),
  logical_effort: z.string().nullable(),
  run_mode: AutomationRunModeSchema,
  base_ref_kind: AutomationBaseRefKindSchema,
  base_ref: z.string(),
  base_display_name: z.string().nullable(),
  // Final merge target (fork point, e.g. `main`) when the automation runs on its own
  // integration branch; detail response only, so optional + nullable.
  base_target_ref: z.string().nullable().optional(),
  base_target_display_name: z.string().nullable().optional(),
  base_source_pull_request_json: z.string().nullable(),
  goal_items_json: z.string().nullable(),
  chain_mode: AutomationChainModeSchema,
  completion_signal: AutomationCompletionSignalSchema,
  plan_approval_mode: AutomationPlanApprovalModeSchema,
  pr_merge_mode: AutomationPrMergeModeSchema,
  plan_deep_verification: z.boolean(),
  max_runs: z.number().int().positive(),
  max_consecutive_failures: z.number().int().positive(),
  first_run_prompt: z.string().nullable(),
  setup_analysis_summary: z.string().nullable(),
  created_at: z.string(),
  updated_at: z.string(),
});

export const AutomationRunSchema = z.object({
  id: z.string(),
  automation_id: z.string(),
  run_index: z.number().int().positive(),
  status: AutomationRunStatusSchema,
  judge_state: AutomationJudgeStateSchema,
  judge_lease_expires_at: z.string().nullable(),
  plan_judge_state: AutomationPlanJudgeStateSchema,
  plan_revision_round: z.number().int().nonnegative(),
  plan_revision_pending: z.boolean(),
  plan_phase: z.boolean(),
  plan_artifact_id: z.string().nullable(),
  plan_blueprint_artifact_id: z.string().nullable().optional().default(null),
  parked_plan_artifact_id: z.string().nullable().optional().default(null),
  parked_plan_blueprint_artifact_id: z
    .string()
    .nullable()
    .optional()
    .default(null),
  plan_approved_by: z.string().nullable(),
  plan_approved_artifact_version: z.number().int().positive().nullable(),
  plan_approved_at: z.string().nullable(),
  conversation_id: z.string().nullable(),
  run_prompt: z.string(),
  prompt_author: AutomationPromptAuthorSchema,
  base_ref_kind: AutomationBaseRefKindSchema,
  base_ref_used: z.string(),
  base_from_run_id: z.string().nullable(),
  goal_item_id: z.string().nullable(),
  branch_name: z.string().nullable(),
  pr_number: z.number().int().nullable(),
  pr_url: z.string().nullable(),
  pr_title: z.string().nullable(),
  pr_head_ref_name: z.string().nullable(),
  pr_base_ref_name: z.string().nullable(),
  pr_merged_at: z.string().nullable(),
  merge_commit_sha: z.string().nullable(),
  diff_stats_json: z.string().nullable(),
  agent_summary: z.string().nullable(),
  judge_verdict_json: z.string().nullable(),
  judge_model_id: z.string().nullable(),
  error_code: z.string().nullable(),
  error_detail: z.string().nullable(),
  signal_check_failures: z.number().int().nonnegative(),
  started_at: z.string().nullable(),
  finished_at: z.string().nullable(),
  created_at: z.string(),
  updated_at: z.string(),
});

export const AutomationUsageSchema = z.object({
  input_tokens: z.number().int().nonnegative(),
  output_tokens: z.number().int().nonnegative(),
  cache_creation_tokens: z.number().int().nonnegative(),
  cache_read_tokens: z.number().int().nonnegative(),
  estimated_usd: z.number().nullable(),
});

export const AutomationPipelineTaskSchema = z.object({
  id: z.string(),
  title: z.string(),
  status: z.string(),
  blocked_by: z.array(z.string()),
});

export const AutomationPipelineProgressSchema = z.object({
  deliverable: z.literal("task_graph"),
  status: z.enum(["authoring", "executing", "completed", "attention"]),
  ideation_session_id: z.string(),
  plan_artifact_id: z.string().nullable(),
  proposal_count: z.number().int().nonnegative(),
  task_total: z.number().int().nonnegative(),
  task_merged: z.number().int().nonnegative(),
  task_terminal: z.number().int().nonnegative(),
  tasks: z.array(AutomationPipelineTaskSchema),
});

export const AutomationDetailSchema = z.object({
  automation: AutomationSchema,
  runs: z.array(AutomationRunSchema),
  usage: AutomationUsageSchema,
  pipeline: AutomationPipelineProgressSchema.nullable().optional(),
});

export const CreateAutomationDraftResponseSchema = z.object({
  automation: AutomationSchema,
  setup_conversation_id: z.string().nullable(),
});

export const AutomationScheduleResponseSchema = z.object({
  scheduled: z.boolean(),
  reason: z.string().nullable(),
});

export const AutomationListSchema = z.array(AutomationSchema);
