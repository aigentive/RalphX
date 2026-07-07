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
  provider_harness: z.string(),
  model_id: z.string(),
  logical_effort: z.string().nullable(),
  run_mode: AutomationRunModeSchema,
  base_ref_kind: AutomationBaseRefKindSchema,
  base_ref: z.string(),
  base_display_name: z.string().nullable(),
  base_source_pull_request_json: z.string().nullable(),
  goal_items_json: z.string().nullable(),
  chain_mode: AutomationChainModeSchema,
  completion_signal: AutomationCompletionSignalSchema,
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
  conversation_id: z.string().nullable(),
  run_prompt: z.string(),
  prompt_author: AutomationPromptAuthorSchema,
  base_ref_kind: AutomationBaseRefKindSchema,
  base_ref_used: z.string(),
  base_from_run_id: z.string().nullable(),
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

export const AutomationDetailSchema = z.object({
  automation: AutomationSchema,
  runs: z.array(AutomationRunSchema),
  usage: AutomationUsageSchema,
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
