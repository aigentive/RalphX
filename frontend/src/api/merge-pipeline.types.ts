// Frontend types for merge pipeline API (camelCase)

import type { ExecutionTaskAgentWorkspace } from "./execution-task-agent-workspace";

/**
 * Merge pipeline task - frontend representation (camelCase)
 */
export interface MergePipelineTask {
  taskId: string;
  title: string;
  displayTitle?: string;
  internalStatus: string;
  sourceBranch: string;
  targetBranch: string;
  isDeferred: boolean;
  isMainMergeDeferred: boolean;
  blockingBranch: string | null;
  conflictFiles: string[] | null;
  errorContext: string | null;
  agentWorkspace?: ExecutionTaskAgentWorkspace | null;
}

/**
 * Merge pipeline response - frontend representation (camelCase)
 */
export interface MergePipelineResponse {
  active: MergePipelineTask[];
  waiting: MergePipelineTask[];
  needsAttention: MergePipelineTask[];
}
