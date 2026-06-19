// Zod schemas for plan-branch API - matches Rust response format (snake_case)

import { z } from "zod";

export const PlanBranchStatusSchema = z.enum(["active", "merged", "abandoned"]);

export const PlanBranchSchema = z.object({
  id: z.string(),
  plan_artifact_id: z.string(),
  session_id: z.string(),
  project_id: z.string(),
  branch_name: z.string(),
  source_branch: z.string(),
  status: PlanBranchStatusSchema,
  merge_task_id: z.string().nullable(),
  created_at: z.string(),
  merged_at: z.string().nullable(),
  pr_number: z.number().int().nullable(),
  pr_url: z.string().nullable(),
  pr_draft: z.boolean().nullable(),
  pr_push_status: z.enum(["pending", "pushed", "failed"]).nullable(),
  pr_status: z.enum(["Draft", "Open", "Merged", "Closed"]).nullable(),
  pr_polling_active: z.boolean().default(false),
  pr_eligible: z.boolean().default(false),
  merge_commit_sha: z.string().nullable().optional(),
  base_branch_override: z.string().nullable(),
  execution_plan_id: z.string().nullable().optional(),
});

export const PlanBranchListSchema = z.array(PlanBranchSchema);

export const PlanBranchNullableSchema = PlanBranchSchema.nullable();
