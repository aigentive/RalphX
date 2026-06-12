// Frontend types for execution API (camelCase)

export type ExecutionHaltMode = "running" | "paused" | "stopped";

/**
 * Execution status response - frontend representation (camelCase)
 * Phase 82: Added globalMaxConcurrent for cross-project cap
 */
export interface ExecutionStatusResponse {
  isPaused: boolean;
  haltMode: ExecutionHaltMode;
  runningCount: number;
  maxConcurrent: number;
  globalMaxConcurrent: number;
  queuedCount: number;
  queuedMessageCount?: number;
  canStartTask: boolean;
  /** Currently generating ideation sessions (consuming slots) */
  ideationActive: number;
  /** Ideation sessions in waiting_for_input state (NOT consuming slots) */
  ideationIdle: number;
  /** Sessions with pending_initial_prompt set (queued, waiting to start) */
  ideationWaiting: number;
  /** Per-project maximum concurrent ideation sessions */
  ideationMaxProject: number;
  /** Global maximum concurrent ideation sessions across all projects */
  ideationMaxGlobal: number;
}

/**
 * Execution command response - frontend representation (camelCase)
 */
export interface ExecutionCommandResponse {
  success: boolean;
  status: ExecutionStatusResponse;
}

/**
 * Execution settings response - frontend representation (camelCase)
 * Contains persistence settings: max concurrent tasks, auto-commit, pause on failure
 */
export interface ExecutionSettingsResponse {
  maxConcurrentTasks: number;
  projectIdeationMax: number;
  autoCommit: boolean;
  pauseOnFailure: boolean;
  agentWorkspacePrAutofixDefault: boolean;
  agentWorkspacePrAutoMergeDefault: boolean;
}

/**
 * Input for updating execution settings - frontend representation (camelCase)
 */
export interface UpdateExecutionSettingsInput {
  maxConcurrentTasks: number;
  projectIdeationMax: number;
  autoCommit: boolean;
  pauseOnFailure: boolean;
  agentWorkspacePrAutofixDefault: boolean;
  agentWorkspacePrAutoMergeDefault: boolean;
}

/**
 * Global execution settings response - frontend representation (Phase 82)
 */
export interface GlobalExecutionSettingsResponse {
  globalMaxConcurrent: number;
  globalIdeationMax: number;
  allowIdeationBorrowIdleExecution: boolean;
}

/**
 * Input for updating global execution settings (Phase 82)
 */
export interface UpdateGlobalExecutionSettingsInput {
  globalMaxConcurrent: number;
  globalIdeationMax: number;
  allowIdeationBorrowIdleExecution: boolean;
}
