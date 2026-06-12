// Transform functions for converting snake_case execution API responses to camelCase frontend types

import { z } from "zod";
import {
  ExecutionStatusResponseSchema,
  ExecutionCommandResponseSchema,
  ExecutionSettingsResponseSchema,
  GlobalExecutionSettingsResponseSchema,
} from "./execution.schemas";
import type {
  ExecutionStatusResponse,
  ExecutionCommandResponse,
  ExecutionSettingsResponse,
  UpdateExecutionSettingsInput,
  GlobalExecutionSettingsResponse,
  UpdateGlobalExecutionSettingsInput,
} from "./execution.types";

/**
 * Transform ExecutionStatusResponseSchema (snake_case) → ExecutionStatusResponse (camelCase)
 * Phase 82: Added globalMaxConcurrent transform
 */
export function transformExecutionStatus(
  raw: z.infer<typeof ExecutionStatusResponseSchema>
): ExecutionStatusResponse {
  return {
    isPaused: raw.is_paused,
    haltMode: raw.halt_mode,
    runningCount: raw.running_count,
    maxConcurrent: raw.max_concurrent,
    globalMaxConcurrent: raw.global_max_concurrent,
    queuedCount: raw.queued_count,
    queuedMessageCount: raw.queued_message_count,
    canStartTask: raw.can_start_task,
    ideationActive: raw.ideation_active,
    ideationIdle: raw.ideation_idle,
    ideationWaiting: raw.ideation_waiting,
    ideationMaxProject: raw.ideation_max_project,
    ideationMaxGlobal: raw.ideation_max_global,
  };
}

/**
 * Transform ExecutionCommandResponseSchema → ExecutionCommandResponse
 */
export function transformExecutionCommand(
  raw: z.infer<typeof ExecutionCommandResponseSchema>
): ExecutionCommandResponse {
  return {
    success: raw.success,
    status: transformExecutionStatus(raw.status),
  };
}

/**
 * Transform ExecutionSettingsResponseSchema (snake_case) → ExecutionSettingsResponse (camelCase)
 */
export function transformExecutionSettings(
  raw: z.infer<typeof ExecutionSettingsResponseSchema>
): ExecutionSettingsResponse {
  return {
    maxConcurrentTasks: raw.max_concurrent_tasks,
    projectIdeationMax: raw.project_ideation_max,
    autoCommit: raw.auto_commit,
    pauseOnFailure: raw.pause_on_failure,
    agentWorkspacePrAutofixDefault: raw.agent_workspace_pr_autofix_default,
    agentWorkspacePrAutoMergeDefault: raw.agent_workspace_pr_auto_merge_default,
  };
}

/**
 * Transform UpdateExecutionSettingsInput (camelCase) → snake_case for Tauri command
 */
export function transformExecutionSettingsInput(
  input: UpdateExecutionSettingsInput
): {
  max_concurrent_tasks: number;
  project_ideation_max: number;
  auto_commit: boolean;
  pause_on_failure: boolean;
  agent_workspace_pr_autofix_default: boolean;
  agent_workspace_pr_auto_merge_default: boolean;
} {
  return {
    max_concurrent_tasks: input.maxConcurrentTasks,
    project_ideation_max: input.projectIdeationMax,
    auto_commit: input.autoCommit,
    pause_on_failure: input.pauseOnFailure,
    agent_workspace_pr_autofix_default: input.agentWorkspacePrAutofixDefault,
    agent_workspace_pr_auto_merge_default: input.agentWorkspacePrAutoMergeDefault,
  };
}

/**
 * Transform GlobalExecutionSettingsResponseSchema (snake_case) → GlobalExecutionSettingsResponse (camelCase)
 * Phase 82
 */
export function transformGlobalExecutionSettings(
  raw: z.infer<typeof GlobalExecutionSettingsResponseSchema>
): GlobalExecutionSettingsResponse {
  return {
    globalMaxConcurrent: raw.global_max_concurrent,
    globalIdeationMax: raw.global_ideation_max,
    allowIdeationBorrowIdleExecution: raw.allow_ideation_borrow_idle_execution,
  };
}

/**
 * Transform UpdateGlobalExecutionSettingsInput (camelCase) → snake_case for Tauri command
 * Phase 82
 */
export function transformGlobalExecutionSettingsInput(
  input: UpdateGlobalExecutionSettingsInput
): {
  global_max_concurrent: number;
  global_ideation_max: number;
  allow_ideation_borrow_idle_execution: boolean;
} {
  return {
    global_max_concurrent: input.globalMaxConcurrent,
    global_ideation_max: input.globalIdeationMax,
    allow_ideation_borrow_idle_execution: input.allowIdeationBorrowIdleExecution,
  };
}
