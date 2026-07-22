// Transform functions for converting snake_case running processes API responses to camelCase frontend types

import { z } from "zod";
import {
  StepProgressSummarySchema,
  RunningProcessSchema,
  RunningProcessesResponseSchema,
  RunningIdeationSessionSchema,
  RunningWorkspaceSessionSchema,
  ExecutionLaneUsageSchema,
  ExecutionCapacitySummarySchema,
} from "./running-processes.schemas";
import type {
  StepProgressSummary,
  RunningProcess,
  RunningProcessesResponse,
  RunningIdeationSession,
  RunningWorkspaceSession,
  ExecutionLaneUsage,
  ExecutionCapacitySummary,
} from "./running-processes.types";
import { transformTaskStep } from "@/types/task-step";
import { transformExecutionTaskAgentWorkspace } from "./execution-task-agent-workspace";

/**
 * Transform StepProgressSummarySchema (snake_case) → StepProgressSummary (camelCase)
 */
export function transformStepProgressSummary(
  raw: z.infer<typeof StepProgressSummarySchema>
): StepProgressSummary {
  return {
    taskId: raw.task_id,
    total: raw.total,
    completed: raw.completed,
    inProgress: raw.in_progress,
    pending: raw.pending,
    skipped: raw.skipped,
    failed: raw.failed,
    currentStep: raw.current_step ? transformTaskStep(raw.current_step) : null,
    nextStep: raw.next_step ? transformTaskStep(raw.next_step) : null,
    percentComplete: raw.percent_complete,
  };
}

/**
 * Transform RunningProcessSchema (snake_case) → RunningProcess (camelCase)
 */
export function transformRunningProcess(
  raw: z.infer<typeof RunningProcessSchema>
): RunningProcess {
  return {
    taskId: raw.task_id,
    title: raw.title,
    internalStatus: raw.internal_status,
    stepProgress: raw.step_progress
      ? transformStepProgressSummary(raw.step_progress)
      : null,
    elapsedSeconds: raw.elapsed_seconds,
    triggerOrigin: raw.trigger_origin,
    taskBranch: raw.task_branch,
    ...(raw.agent_workspace !== undefined && {
      agentWorkspace: raw.agent_workspace
        ? transformExecutionTaskAgentWorkspace(raw.agent_workspace)
        : null,
    }),
  };
}

/**
 * Transform RunningIdeationSessionSchema (snake_case) → RunningIdeationSession (camelCase)
 */
export function transformRunningIdeationSession(
  raw: z.infer<typeof RunningIdeationSessionSchema>
): RunningIdeationSession {
  return {
    sessionId: raw.session_id,
    title: raw.title,
    elapsedSeconds: raw.elapsed_seconds,
    isGenerating: raw.is_generating,
  };
}

/**
 * Transform RunningWorkspaceSessionSchema (snake_case) → RunningWorkspaceSession (camelCase)
 */
export function transformRunningWorkspaceSession(
  raw: z.infer<typeof RunningWorkspaceSessionSchema>
): RunningWorkspaceSession {
  return {
    conversationId: raw.conversation_id,
    projectId: raw.project_id,
    automationId: raw.automation_id,
    automationRunId: raw.automation_run_id,
    title: raw.title,
    elapsedSeconds: raw.elapsed_seconds,
    model: raw.model,
  };
}

/**
 * Transform ExecutionLaneUsageSchema (snake_case) → ExecutionLaneUsage (camelCase)
 */
export function transformExecutionLaneUsage(
  raw: z.infer<typeof ExecutionLaneUsageSchema>
): ExecutionLaneUsage {
  return {
    lane: raw.lane,
    active: raw.active,
    idle: raw.idle,
    waiting: raw.waiting,
    max: raw.max,
    borrowed: raw.borrowed,
    priorityRank: raw.priority_rank,
  };
}

/**
 * Transform ExecutionCapacitySummarySchema (snake_case) → ExecutionCapacitySummary (camelCase)
 */
export function transformExecutionCapacitySummary(
  raw: z.infer<typeof ExecutionCapacitySummarySchema>
): ExecutionCapacitySummary {
  return {
    totalActive: raw.total_active,
    globalMaxConcurrent: raw.global_max_concurrent,
    borrowingEnabled: raw.borrowing_enabled,
    priority: raw.priority,
  };
}

/**
 * Transform RunningProcessesResponseSchema (snake_case) → RunningProcessesResponse (camelCase)
 */
export function transformRunningProcessesResponse(
  raw: z.infer<typeof RunningProcessesResponseSchema>
): RunningProcessesResponse {
  return {
    processes: raw.processes.map(transformRunningProcess),
    ideationSessions: raw.ideation_sessions.map(transformRunningIdeationSession),
    workspaceSessions: raw.workspace_sessions.map(transformRunningWorkspaceSession),
    lanes: raw.lanes.map(transformExecutionLaneUsage),
    capacity: transformExecutionCapacitySummary(raw.capacity),
  };
}
