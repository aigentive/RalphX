// Zod schemas for execution API responses (snake_case from Rust backend)

import { z } from "zod";

/**
 * Execution status response schema from Rust (snake_case)
 * Backend outputs snake_case by default (no rename_all annotation)
 * Phase 82: Added global_max_concurrent for cross-project cap
 */
export const ExecutionStatusResponseSchema = z.object({
  is_paused: z.boolean(),
  halt_mode: z.enum(["running", "paused", "stopped"]),
  running_count: z.number().int().nonnegative(),
  max_concurrent: z.number().int().nonnegative(),
  global_max_concurrent: z.number().int().nonnegative(),
  queued_count: z.number().int().nonnegative(),
  queued_message_count: z.number().int().nonnegative(),
  can_start_task: z.boolean(),
  ideation_active: z.number().int().nonnegative(),
  ideation_idle: z.number().int().nonnegative(),
  ideation_waiting: z.number().int().nonnegative(),
  ideation_max_project: z.number().int().nonnegative(),
  ideation_max_global: z.number().int().nonnegative(),
});

/**
 * Execution command response schema from Rust (for pause/resume/stop) (snake_case)
 */
export const ExecutionCommandResponseSchema = z.object({
  success: z.boolean(),
  status: ExecutionStatusResponseSchema,
});

/**
 * Execution settings response schema from Rust (snake_case)
 * Contains persistence settings: max concurrent tasks, auto-commit, pause on failure
 */
export const ExecutionSettingsResponseSchema = z.object({
  max_concurrent_tasks: z.number().int().positive(),
  project_ideation_max: z.number().int().nonnegative(),
  auto_commit: z.boolean(),
  pause_on_failure: z.boolean(),
  agent_workspace_pr_autofix_default: z.boolean(),
  agent_workspace_pr_auto_merge_default: z.boolean(),
});

/**
 * Input schema for updating execution settings (snake_case for Tauri command)
 */
export const UpdateExecutionSettingsInputSchema = z.object({
  max_concurrent_tasks: z.number().int().positive(),
  project_ideation_max: z.number().int().nonnegative(),
  auto_commit: z.boolean(),
  pause_on_failure: z.boolean(),
  agent_workspace_pr_autofix_default: z.boolean(),
  agent_workspace_pr_auto_merge_default: z.boolean(),
});

/**
 * Global execution settings response schema (Phase 82)
 */
export const GlobalExecutionSettingsResponseSchema = z.object({
  global_max_concurrent: z.number().int().positive(),
  global_ideation_max: z.number().int().positive(),
  allow_ideation_borrow_idle_execution: z.boolean(),
});

/**
 * Input schema for updating global execution settings (Phase 82)
 */
export const UpdateGlobalExecutionSettingsInputSchema = z.object({
  global_max_concurrent: z.number().int().positive(),
  global_ideation_max: z.number().int().positive(),
  allow_ideation_borrow_idle_execution: z.boolean(),
});
