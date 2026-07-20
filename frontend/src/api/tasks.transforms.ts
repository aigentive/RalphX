// Transform functions for converting snake_case tasks API responses to camelCase frontend types

import { z } from "zod";
import {
  BulkArchiveResponseSchemaRaw,
  BulkCancelResponseSchemaRaw,
  BulkPauseResponseSchemaRaw,
  BulkResumeResponseSchemaRaw,
  CleanupReportResponseSchemaRaw,
  ExecutionPlanControlResponseSchemaRaw,
  InjectTaskResponseSchemaRaw,
  StateTransitionResponseSchemaRaw,
  TaskHistoryAvailabilityResponseSchemaRaw,
  UnblockTaskResponseSchemaRaw,
} from "./tasks.schemas";
import { transformTask, type Task, type InternalStatus } from "@/types/task";
import type { TaskRuntimeHistoryContextType } from "@/types/task-history";

export interface TaskHistoryAvailability {
  hasHistory: boolean;
  taskCount: number;
}

export function transformTaskHistoryAvailability(
  raw: z.infer<typeof TaskHistoryAvailabilityResponseSchemaRaw>
): TaskHistoryAvailability {
  return { hasHistory: raw.has_history, taskCount: raw.task_count };
}

/**
 * Frontend BulkCancelResponse type (camelCase)
 */
export interface BulkCancelResponse {
  cancelledCount: number;
}

/**
 * Transform BulkCancelResponseSchemaRaw to BulkCancelResponse
 */
export function transformBulkCancelResponse(
  raw: z.infer<typeof BulkCancelResponseSchemaRaw>
): BulkCancelResponse {
  return {
    cancelledCount: raw.cancelled_count,
  };
}

export interface BulkPauseResponse {
  pausedCount: number;
}

export function transformBulkPauseResponse(raw: z.infer<typeof BulkPauseResponseSchemaRaw>): BulkPauseResponse {
  return { pausedCount: raw.paused_count };
}

export interface BulkResumeResponse {
  resumedCount: number;
}

export function transformBulkResumeResponse(raw: z.infer<typeof BulkResumeResponseSchemaRaw>): BulkResumeResponse {
  return { resumedCount: raw.resumed_count };
}

export interface ExecutionPlanControlResponse {
  executionPlanId: string;
  affectedCount: number;
}

export function transformExecutionPlanControlResponse(
  raw: z.infer<typeof ExecutionPlanControlResponseSchemaRaw>
): ExecutionPlanControlResponse {
  return {
    executionPlanId: raw.execution_plan_id,
    affectedCount: raw.affected_count,
  };
}

export interface BulkArchiveResponse {
  archivedCount: number;
}

export function transformBulkArchiveResponse(raw: z.infer<typeof BulkArchiveResponseSchemaRaw>): BulkArchiveResponse {
  return { archivedCount: raw.archived_count };
}

/**
 * Frontend CleanupReport type (camelCase)
 */
export interface CleanupReport {
  deletedCount: number;
  failedCount: number;
  stoppedAgents: number;
}

/**
 * Transform CleanupReportResponseSchemaRaw to CleanupReport
 */
export function transformCleanupReport(
  raw: z.infer<typeof CleanupReportResponseSchemaRaw>
): CleanupReport {
  return {
    deletedCount: raw.deleted_count,
    failedCount: raw.failed_count,
    stoppedAgents: raw.stopped_agents,
  };
}

/**
 * Frontend InjectTaskResponse type (camelCase)
 */
export interface InjectTaskResponse {
  task: Task;
  target: "backlog" | "planned";
  priority: number;
  makeNextApplied: boolean;
}

/**
 * Transform InjectTaskResponseSchemaRaw to InjectTaskResponse
 */
export function transformInjectTaskResponse(
  raw: z.infer<typeof InjectTaskResponseSchemaRaw>
): InjectTaskResponse {
  return {
    task: transformTask(raw.task),
    target: raw.target,
    priority: raw.priority,
    makeNextApplied: raw.make_next_applied,
  };
}

/**
 * Frontend UnblockTaskResponse type (camelCase)
 */
export interface UnblockTaskResponse {
  task: Task;
  /** Set when the task was unblocked despite having failed dependencies. */
  warning: string | null;
}

/**
 * Transform UnblockTaskResponseSchemaRaw to UnblockTaskResponse
 */
export function transformUnblockTaskResponse(
  raw: z.infer<typeof UnblockTaskResponseSchemaRaw>
): UnblockTaskResponse {
  return {
    task: transformTask(raw.task),
    warning: raw.warning,
  };
}

/**
 * Frontend StateTransition type (camelCase)
 * Represents a single state transition in a task's history.
 */
export interface StateTransition {
  /** Status transitioned from (null for initial state) */
  fromStatus: InternalStatus | null;
  /** Status transitioned to */
  toStatus: InternalStatus;
  /** What triggered this transition (e.g., "user", "agent", "system") */
  trigger: string;
  /** When the transition occurred (RFC3339 format) */
  timestamp: string;
  /** Conversation ID for states that spawn conversations (executing, re_executing, reviewing) */
  conversationId?: string;
  /** Agent run ID for the specific execution within the conversation */
  agentRunId?: string;
  /** Runtime context type for the associated transcript, when provided by the backend */
  contextType?: TaskRuntimeHistoryContextType;
  /** Stable transition identity, when provided by the backend */
  transitionId?: string;
}

function isTaskRuntimeHistoryContextType(
  contextType: string | null | undefined
): contextType is TaskRuntimeHistoryContextType {
  return (
    contextType === "task_execution" ||
    contextType === "review" ||
    contextType === "merge" ||
    contextType === "branch_update"
  );
}

/**
 * Transform StateTransitionResponseSchemaRaw to StateTransition
 */
export function transformStateTransition(
  raw: z.infer<typeof StateTransitionResponseSchemaRaw>
): StateTransition {
  return {
    fromStatus: raw.from_status as InternalStatus | null,
    toStatus: raw.to_status as InternalStatus,
    trigger: raw.trigger,
    timestamp: raw.timestamp,
    // Only include conversationId/agentRunId if they have actual string values (not null or undefined)
    ...(raw.conversation_id != null && { conversationId: raw.conversation_id }),
    ...(raw.agent_run_id != null && { agentRunId: raw.agent_run_id }),
    ...(isTaskRuntimeHistoryContextType(raw.context_type) && { contextType: raw.context_type }),
    ...(raw.transition_id != null && { transitionId: raw.transition_id }),
  };
}
