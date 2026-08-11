// Frontend types for running processes API (camelCase)

import type { TaskStep } from "@/types/task-step";
import type { ExecutionTaskAgentWorkspace } from "./execution-task-agent-workspace";

/**
 * Step progress summary - frontend representation (camelCase)
 */
export interface StepProgressSummary {
  taskId: string;
  total: number;
  completed: number;
  inProgress: number;
  pending: number;
  skipped: number;
  failed: number;
  currentStep: TaskStep | null;
  nextStep: TaskStep | null;
  percentComplete: number;
}

/**
 * Running process - frontend representation (camelCase)
 */
export interface RunningProcess {
  taskId: string;
  title: string;
  internalStatus: string;
  stepProgress: StepProgressSummary | null;
  elapsedSeconds: number | null;
  triggerOrigin: string | null;
  taskBranch: string | null;
  agentWorkspace?: ExecutionTaskAgentWorkspace | null;
}

/**
 * Running ideation session - frontend representation (camelCase)
 */
export interface RunningIdeationSession {
  sessionId: string;
  title: string;
  elapsedSeconds: number | null;
  isGenerating: boolean;
}

/**
 * Running workspace conversation - frontend representation (camelCase)
 */
export interface RunningWorkspaceSession {
  conversationId: string;
  projectId: string;
  automationId: string | null;
  automationRunId: string | null;
  title: string;
  elapsedSeconds: number | null;
  model: string | null;
}

export type ExecutionLaneName = "workspaces" | "tasks" | "ideation";

/**
 * Execution lane usage - frontend representation (camelCase)
 */
export interface ExecutionLaneUsage {
  lane: ExecutionLaneName;
  active: number;
  idle: number;
  waiting: number;
  max: number;
  borrowed: number;
  priorityRank: number;
}

/**
 * Execution capacity summary - frontend representation (camelCase)
 */
export interface ExecutionCapacitySummary {
  totalActive: number;
  globalMaxConcurrent: number;
  borrowingEnabled: boolean;
  priority: ExecutionLaneName[];
}

/**
 * Running processes response - frontend representation (camelCase)
 */
export interface RunningProcessesResponse {
  processes: RunningProcess[];
  ideationSessions: RunningIdeationSession[];
  workspaceSessions: RunningWorkspaceSession[];
  lanes: ExecutionLaneUsage[];
  capacity: ExecutionCapacitySummary;
}
