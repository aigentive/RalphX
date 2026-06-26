/**
 * Mock Execution API
 *
 * Mirrors the interface of src/api/execution.ts with mock implementations.
 * Phase 82: Added globalMaxConcurrent, projectId parameters, and global settings API
 */

import type {
  ExecutionStatusResponse,
  ExecutionCommandResponse,
  ExecutionSettingsResponse,
  UpdateExecutionSettingsInput,
  GlobalExecutionSettingsResponse,
  UpdateGlobalExecutionSettingsInput,
} from "@/api/execution";

// ============================================================================
// Mock State
// ============================================================================

let mockExecutionState: ExecutionStatusResponse = {
  isPaused: false,
  haltMode: "running",
  runningCount: 0,
  maxConcurrent: 3,
  globalMaxConcurrent: 20,
  queuedCount: 0,
  canStartTask: true,
  ideationActive: 0,
  ideationIdle: 0,
  ideationWaiting: 0,
  ideationMaxProject: 5,
  ideationMaxGlobal: 10,
};

let mockGlobalSettings: GlobalExecutionSettingsResponse = {
  globalMaxConcurrent: 20,
  workspaceMaxConcurrent: 10,
  globalIdeationMax: 10,
  allowIdeationBorrowIdleExecution: false,
};

let mockExecutionSettings: ExecutionSettingsResponse = {
  maxConcurrentTasks: 3,
  projectIdeationMax: 5,
  autoCommit: false,
  pauseOnFailure: true,
  agentWorkspacePrAutofixDefault: false,
  agentWorkspacePrAutoMergeDefault: false,
};

let _mockActiveProjectId: string | undefined = undefined;

// ============================================================================
// Mock Execution API
// ============================================================================

export const mockExecutionApi = {
  // Phase 82: Added optional projectId parameter
  getStatus: async (_projectId?: string): Promise<ExecutionStatusResponse> => {
    return { ...mockExecutionState };
  },

  // Phase 82: Added optional projectId parameter
  pause: async (_projectId?: string): Promise<ExecutionCommandResponse> => {
    mockExecutionState = {
      ...mockExecutionState,
      isPaused: true,
      haltMode: "paused",
      canStartTask: false,
    };
    return {
      success: true,
      status: { ...mockExecutionState },
    };
  },

  // Phase 82: Added optional projectId parameter
  resume: async (_projectId?: string): Promise<ExecutionCommandResponse> => {
    mockExecutionState = {
      ...mockExecutionState,
      isPaused: false,
      haltMode: "running",
      canStartTask: true,
    };
    return {
      success: true,
      status: { ...mockExecutionState },
    };
  },

  // Phase 82: Added optional projectId parameter
  stop: async (_projectId?: string): Promise<ExecutionCommandResponse> => {
    mockExecutionState = {
      isPaused: true,
      haltMode: "stopped",
      runningCount: 0,
      maxConcurrent: 3,
      globalMaxConcurrent: 20,
      queuedCount: 0,
      canStartTask: false,
      ideationActive: 0,
      ideationIdle: 0,
      ideationWaiting: 0,
      ideationMaxProject: 5,
      ideationMaxGlobal: 10,
    };
    return {
      success: true,
      status: { ...mockExecutionState },
    };
  },

  // Phase 82: Added optional projectId parameter
  getSettings: async (_projectId?: string): Promise<ExecutionSettingsResponse> => {
    return { ...mockExecutionSettings };
  },

  // Phase 82: Added optional projectId parameter
  updateSettings: async (
    input: UpdateExecutionSettingsInput,
    _projectId?: string
  ): Promise<ExecutionSettingsResponse> => {
    mockExecutionSettings = { ...input };
    mockExecutionState = {
      ...mockExecutionState,
      maxConcurrent: input.maxConcurrentTasks,
    };
    return { ...mockExecutionSettings };
  },

  // Phase 82: Set active project (stores for potential future use in mock scoping)
  setActiveProject: async (projectId?: string): Promise<void> => {
    _mockActiveProjectId = projectId;
    // No-op in mock - just stores the value for potential future use
    void _mockActiveProjectId;
  },

  // Phase 82: Get global execution settings
  getGlobalSettings: async (): Promise<GlobalExecutionSettingsResponse> => {
    return { ...mockGlobalSettings };
  },

  // Phase 82: Update global execution settings
  updateGlobalSettings: async (
    input: UpdateGlobalExecutionSettingsInput
  ): Promise<GlobalExecutionSettingsResponse> => {
    mockGlobalSettings = { ...input };
    mockExecutionState = {
      ...mockExecutionState,
      globalMaxConcurrent: input.globalMaxConcurrent,
    };
    return { ...mockGlobalSettings };
  },
} as const;
