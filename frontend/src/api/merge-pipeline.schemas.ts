// Zod schemas for merge pipeline API responses (snake_case from Rust backend)

import { z } from "zod";
import { ExecutionTaskAgentWorkspaceSchema } from "./execution-task-agent-workspace";

/**
 * Merge pipeline task schema from Rust (snake_case)
 */
export const MergePipelineTaskSchema = z.object({
  task_id: z.string(),
  title: z.string(),
  display_title: z.string().optional(),
  internal_status: z.string(),
  source_branch: z.string(),
  target_branch: z.string(),
  is_deferred: z.boolean(),
  is_main_merge_deferred: z.boolean(),
  blocking_branch: z.string().nullable(),
  conflict_files: z.array(z.string()).nullable(),
  error_context: z.string().nullable(),
  agent_workspace: ExecutionTaskAgentWorkspaceSchema.nullable().optional(),
});

/**
 * Merge pipeline response schema from Rust (snake_case)
 */
export const MergePipelineResponseSchema = z.object({
  active: z.array(MergePipelineTaskSchema),
  waiting: z.array(MergePipelineTaskSchema),
  needs_attention: z.array(MergePipelineTaskSchema),
});
