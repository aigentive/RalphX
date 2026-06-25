/**
 * Mock Running Processes API
 *
 * Mirrors the interface of src/api/running-processes.ts with mock implementations.
 */

import type { RunningProcessesResponse } from "@/api/running-processes";

// ============================================================================
// Mock State
// ============================================================================

const mockRunningProcesses: RunningProcessesResponse = {
  ideationSessions: [],
  workspaceSessions: [],
  lanes: [
    {
      lane: "workspaces",
      active: 0,
      idle: 0,
      waiting: 0,
      max: 10,
      borrowed: 0,
      priorityRank: 1,
    },
    {
      lane: "tasks",
      active: 2,
      idle: 0,
      waiting: 0,
      max: 10,
      borrowed: 0,
      priorityRank: 2,
    },
    {
      lane: "ideation",
      active: 0,
      idle: 0,
      waiting: 0,
      max: 5,
      borrowed: 0,
      priorityRank: 3,
    },
  ],
  capacity: {
    totalActive: 2,
    globalMaxConcurrent: 20,
    borrowingEnabled: false,
    priority: ["workspaces", "tasks", "ideation"],
  },
  processes: [
    {
      taskId: "task-1",
      title: "Add JWT authentication",
      internalStatus: "executing",
      stepProgress: {
        taskId: "task-1",
        total: 7,
        completed: 2,
        inProgress: 1,
        pending: 4,
        skipped: 0,
        failed: 0,
        currentStep: null,
        nextStep: null,
        percentComplete: 28.6,
      },
      elapsedSeconds: 134,
      triggerOrigin: "scheduler",
      taskBranch: "ralphx/app/task-a1b2c3",
    },
    {
      taskId: "task-2",
      title: "Fix login validation",
      internalStatus: "re_executing",
      stepProgress: {
        taskId: "task-2",
        total: 5,
        completed: 0,
        inProgress: 1,
        pending: 4,
        skipped: 0,
        failed: 0,
        currentStep: null,
        nextStep: null,
        percentComplete: 0,
      },
      elapsedSeconds: 45,
      triggerOrigin: "revision",
      taskBranch: "ralphx/app/task-d4e5f6",
    },
  ],
};

// ============================================================================
// Mock Running Processes API
// ============================================================================

export const mockRunningProcessesApi = {
  getRunningProcesses: async (): Promise<RunningProcessesResponse> => {
    return { ...mockRunningProcesses };
  },
} as const;
