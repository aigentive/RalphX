/**
 * Mock Tasks API
 *
 * Mirrors the interface of src/api/tasks.ts with mock implementations.
 * List/get operations return mock data; create/update/delete are no-ops that return success.
 */

import type { Task, TaskListResponse, CreateTask, UpdateTask, InternalStatus } from "@/types/task";
import {
  getStepProgressDisplay,
  type TaskStep,
  type StepProgressSummary,
} from "@/types/task-step";
import type { CleanupReport, InjectTaskResponse, InjectTaskInput, StateTransition, UnblockTaskResponse } from "@/api/tasks";
import { AGENT_WORKER } from "@/constants/agents";
import { createMockTask, generateTestUuid } from "@/test/mock-data";
import { getStore } from "./store";

// ============================================================================
// Helper to create a valid TaskStep
// ============================================================================

function createMockStep(overrides: Partial<TaskStep> & { id: string; taskId: string }): TaskStep {
  return {
    title: "Mock Step",
    description: null,
    status: "pending",
    sortOrder: 0,
    dependsOn: null,
    createdBy: AGENT_WORKER,
    completionNote: null,
    createdAt: new Date().toISOString(),
    updatedAt: new Date().toISOString(),
    startedAt: null,
    completedAt: null,
    ...overrides,
  };
}

// ============================================================================
// Mock Tasks API
// ============================================================================

export const mockTasksApi = {
  getSessionHistoryAvailability: async (
    projectId: string,
    ideationSessionId: string,
  ): Promise<{ hasHistory: boolean; taskCount: number }> => {
    const taskCount = Array.from(getStore().tasks.values()).filter(
      (task) =>
        task.projectId === projectId && task.ideationSessionId === ideationSessionId,
    ).length;
    return { hasHistory: taskCount > 0, taskCount };
  },

  list: async (params: {
    projectId: string;
    statuses?: string[];
    offset?: number;
    limit?: number;
    includeArchived?: boolean;
    ideationSessionId?: string | null;
    executionPlanId?: string | null;
  }): Promise<TaskListResponse> => {
    const store = getStore();
    let tasks = Array.from(store.tasks.values()).filter(
      (t) => t.projectId === params.projectId
    );

    // Filter by ideationSessionId (matches planArtifactId) if provided
    if (params.ideationSessionId !== undefined) {
      tasks = tasks.filter((t) => t.planArtifactId === params.ideationSessionId);
    } else if (params.executionPlanId !== undefined && params.executionPlanId !== null) {
      // In web-mode mocks, execution-plan-scoped tasks reuse planArtifactId as the stable filter key.
      tasks = tasks.filter((t) => t.planArtifactId === params.executionPlanId);
    }

    // Filter by statuses if provided
    if (params.statuses && params.statuses.length > 0) {
      tasks = tasks.filter((t) => params.statuses!.includes(t.internalStatus));
    }

    // Filter archived if not included
    if (!params.includeArchived) {
      tasks = tasks.filter((t) => !t.archivedAt);
    }

    // Apply pagination
    const offset = params.offset ?? 0;
    const limit = params.limit ?? 20;
    const paginatedTasks = tasks.slice(offset, offset + limit);

    return {
      tasks: paginatedTasks,
      total: tasks.length,
      offset,
      hasMore: offset + limit < tasks.length,
    };
  },

  search: async (
    projectId: string,
    query: string,
    _includeArchived?: boolean,
    ideationSessionId?: string | null,
    executionPlanId?: string | null
  ): Promise<Task[]> => {
    const store = getStore();
    const lowerQuery = query.toLowerCase();
    let tasks = Array.from(store.tasks.values()).filter(
      (t) =>
        t.projectId === projectId &&
        (t.title.toLowerCase().includes(lowerQuery) ||
          (t.description && t.description.toLowerCase().includes(lowerQuery)))
    );

    // Filter by ideationSessionId (matches planArtifactId) if provided
    if (ideationSessionId !== undefined) {
      tasks = tasks.filter((t) => t.planArtifactId === ideationSessionId);
    } else if (executionPlanId !== undefined && executionPlanId !== null) {
      tasks = tasks.filter((t) => t.planArtifactId === executionPlanId);
    }

    return tasks;
  },

  get: async (taskId: string): Promise<Task> => {
    const store = getStore();
    const task = store.tasks.get(taskId);
    if (!task) {
      throw new Error(`Task not found: ${taskId}`);
    }
    return task;
  },

  create: async (input: CreateTask): Promise<Task> => {
    // Read-only mock: return a new task with generated ID
    const task = createMockTask({
      id: generateTestUuid(),
      projectId: input.projectId,
      title: input.title,
      description: input.description ?? null,
      category: input.category ?? "feature",
      priority: input.priority ?? 0,
      blockedReason: null,
    });
    // Don't persist in read-only mode
    return task;
  },

  update: async (taskId: string, input: UpdateTask): Promise<Task> => {
    const store = getStore();
    const existing = store.tasks.get(taskId);
    if (!existing) {
      throw new Error(`Task not found: ${taskId}`);
    }
    // Read-only mock: return merged task without persisting
    return {
      ...existing,
      ...(input.category !== undefined && { category: input.category }),
      ...(input.title !== undefined && { title: input.title }),
      ...(input.description !== undefined && { description: input.description }),
      ...(input.priority !== undefined && { priority: input.priority }),
      updatedAt: new Date().toISOString(),
    };
  },

  delete: async (_taskId: string): Promise<boolean> => {
    // Read-only mock: return success
    return true;
  },

  archive: async (taskId: string): Promise<Task> => {
    const store = getStore();
    const task = store.tasks.get(taskId);
    if (!task) {
      throw new Error(`Task not found: ${taskId}`);
    }
    return { ...task, archivedAt: new Date().toISOString() };
  },

  restore: async (taskId: string): Promise<Task> => {
    const store = getStore();
    const task = store.tasks.get(taskId);
    if (!task) {
      throw new Error(`Task not found: ${taskId}`);
    }
    return { ...task, archivedAt: null };
  },

  pause: async (taskId: string): Promise<Task> => {
    const store = getStore();
    const task = store.tasks.get(taskId);
    if (!task) {
      throw new Error(`Task not found: ${taskId}`);
    }
    return { ...task, internalStatus: "paused" as InternalStatus };
  },

  resume: async (taskId: string): Promise<Task> => {
    const store = getStore();
    const task = store.tasks.get(taskId);
    if (!task) {
      throw new Error(`Task not found: ${taskId}`);
    }
    return { ...task, internalStatus: "executing" as InternalStatus };
  },

  stop: async (taskId: string): Promise<Task> => {
    const store = getStore();
    const task = store.tasks.get(taskId);
    if (!task) {
      throw new Error(`Task not found: ${taskId}`);
    }
    return { ...task, internalStatus: "stopped" as InternalStatus };
  },

  retryBranchUpdate: async (taskId: string): Promise<Task> => {
    const store = getStore();
    const task = store.tasks.get(taskId);
    if (!task) {
      throw new Error(`Task not found: ${taskId}`);
    }
    const direction = (() => {
      try {
        const metadata = task.metadata ? JSON.parse(task.metadata) : {};
        return metadata?.branch_update?.direction;
      } catch {
        return null;
      }
    })();
    return {
      ...task,
      internalStatus:
        direction === "task_branch" ? "updating_task_branch" : "updating_plan_branch",
    };
  },

  getArchivedCount: async (
    projectId: string,
    ideationSessionId?: string | null,
    executionPlanId?: string | null
  ): Promise<number> => {
    const store = getStore();
    let tasks = Array.from(store.tasks.values()).filter(
      (t) => t.projectId === projectId && t.archivedAt !== null
    );

    // Filter by ideationSessionId (matches planArtifactId) if provided
    if (ideationSessionId !== undefined) {
      tasks = tasks.filter((t) => t.planArtifactId === ideationSessionId);
    } else if (executionPlanId !== undefined && executionPlanId !== null) {
      tasks = tasks.filter((t) => t.planArtifactId === executionPlanId);
    }

    return tasks.length;
  },

  getValidTransitions: async (taskId: string): Promise<{ status: string; label: string }[]> => {
    // Return valid transitions based on current task status
    const store = getStore();
    const task = store.tasks.get(taskId);
    if (!task) {
      return [];
    }

    // Common transition mappings
    const transitionMap: Record<string, { status: string; label: string }[]> = {
      backlog: [{ status: "ready", label: "Ready" }],
      ready: [
        { status: "executing", label: "Executing" },
        { status: "blocked", label: "Blocked" },
      ],
      blocked: [{ status: "ready", label: "Ready" }],
      executing: [{ status: "pending_review", label: "Pending Review" }],
      pending_review: [{ status: "reviewing", label: "AI Review in Progress" }],
      reviewing: [
        { status: "review_passed", label: "AI Review Passed" },
        { status: "revision_needed", label: "Needs Revision" },
        { status: "escalated", label: "Escalated" },
      ],
      review_passed: [{ status: "approved", label: "Approved" }],
      escalated: [
        { status: "approved", label: "Approved" },
        { status: "revision_needed", label: "Needs Revision" },
      ],
    };

    return transitionMap[task.internalStatus] ?? [];
  },

  move: async (taskId: string, toStatus: string): Promise<Task> => {
    const store = getStore();
    const task = store.tasks.get(taskId);
    if (!task) {
      throw new Error(`Task not found: ${taskId}`);
    }
    return {
      ...task,
      internalStatus: toStatus as InternalStatus,
      updatedAt: new Date().toISOString(),
    };
  },

  retryMerge: async (_taskId: string, _skipValidation?: boolean): Promise<null> => {
    return null;
  },

  inject: async (input: InjectTaskInput): Promise<InjectTaskResponse> => {
    const task = createMockTask({
      id: generateTestUuid(),
      projectId: input.projectId,
      title: input.title,
      description: input.description ?? null,
      category: (input.category ?? "feature") as Task["category"],
      internalStatus: input.target === "planned" ? "ready" : "backlog",
      blockedReason: null,
    });
    return {
      task,
      target: input.target ?? "backlog",
      priority: task.priority,
      makeNextApplied: input.makeNext ?? false,
    };
  },

  getTasksAwaitingReview: async (projectId: string): Promise<Task[]> => {
    const store = getStore();
    const reviewStatuses = ["pending_review", "reviewing", "review_passed", "escalated"];
    return Array.from(store.tasks.values()).filter(
      (t) => t.projectId === projectId && reviewStatuses.includes(t.internalStatus)
    );
  },

  block: async (taskId: string, reason?: string): Promise<Task> => {
    const store = getStore();
    const task = store.tasks.get(taskId);
    if (!task) {
      throw new Error(`Task not found: ${taskId}`);
    }
    return {
      ...task,
      internalStatus: "blocked",
      blockedReason: reason ?? null,
      updatedAt: new Date().toISOString(),
    };
  },

  unblock: async (taskId: string): Promise<UnblockTaskResponse> => {
    const store = getStore();
    const task = store.tasks.get(taskId);
    if (!task) {
      throw new Error(`Task not found: ${taskId}`);
    }
    return {
      task: {
        ...task,
        internalStatus: "ready",
        blockedReason: null,
        updatedAt: new Date().toISOString(),
      },
      warning: null,
    };
  },

  getStateTransitions: async (taskId: string): Promise<StateTransition[]> => {
    const store = getStore();
    const task = store.tasks.get(taskId);
    if (!task) {
      return [];
    }

    // Generate mock state transitions based on current task status
    // This simulates a realistic task history for visual testing
    const transitions: StateTransition[] = [];
    const baseTime = new Date(task.createdAt);

    // Always start from backlog
    transitions.push({
      fromStatus: null,
      toStatus: "backlog",
      trigger: "user",
      timestamp: baseTime.toISOString(),
    });

    // Common progression based on current status
    // Uses all InternalStatus values from status.ts
    const statusProgression: Record<InternalStatus, InternalStatus[]> = {
      backlog: [],
      ready: ["ready"],
      blocked: ["ready", "blocked"],
      executing: ["ready", "executing"],
      qa_refining: ["ready", "executing", "qa_refining"],
      qa_testing: ["ready", "executing", "qa_refining", "qa_testing"],
      qa_passed: ["ready", "executing", "qa_refining", "qa_testing", "qa_passed"],
      qa_failed: ["ready", "executing", "qa_refining", "qa_testing", "qa_failed"],
      pending_review: ["ready", "executing", "pending_review"],
      reviewing: ["ready", "executing", "pending_review", "reviewing"],
      review_passed: ["ready", "executing", "pending_review", "reviewing", "review_passed"],
      revision_needed: ["ready", "executing", "pending_review", "reviewing", "revision_needed"],
      re_executing: ["ready", "executing", "pending_review", "reviewing", "revision_needed", "re_executing"],
      escalated: ["ready", "executing", "pending_review", "reviewing", "escalated"],
      approved: ["ready", "executing", "pending_review", "reviewing", "review_passed", "approved"],
      pending_merge: ["ready", "executing", "pending_review", "reviewing", "review_passed", "approved", "pending_merge"],
      merging: ["ready", "executing", "pending_review", "reviewing", "review_passed", "approved", "pending_merge", "merging"],
      waiting_on_pr: ["ready", "executing", "pending_review", "reviewing", "review_passed", "approved", "pending_merge", "waiting_on_pr"],
      merge_incomplete: ["ready", "executing", "pending_review", "reviewing", "review_passed", "approved", "pending_merge", "merging", "merge_incomplete"],
      merge_conflict: ["ready", "executing", "pending_review", "reviewing", "review_passed", "approved", "pending_merge", "merging", "merge_conflict"],
      updating_plan_branch: ["ready", "updating_plan_branch"],
      updating_task_branch: ["ready", "updating_task_branch"],
      branch_update_blocked: ["ready", "branch_update_blocked"],
      merged: ["ready", "executing", "pending_review", "reviewing", "review_passed", "approved", "pending_merge", "merged"],
      cancelled: ["ready", "cancelled"],
      failed: ["ready", "executing", "failed"],
      paused: ["ready", "executing", "paused"],
      stopped: ["ready", "executing", "stopped"],
    };

    const progression = statusProgression[task.internalStatus] ?? [];
    let prevStatus: InternalStatus = "backlog";
    let timeOffset = 1;

    for (const status of progression) {
      const transitionTime = new Date(baseTime.getTime() + timeOffset * 60 * 60 * 1000);
      transitions.push({
        fromStatus: prevStatus,
        toStatus: status,
        trigger: status === "ready" ? "user" : status === "executing" ? "agent" : "system",
        timestamp: transitionTime.toISOString(),
      });
      prevStatus = status;
      timeOffset++;
    }

    return transitions;
  },

  cleanupTask: async (_taskId: string): Promise<void> => {
    // Read-only mock: no-op
  },

  cleanupTasksInGroup: async (
    _groupKind: string,
    _groupId: string,
    _projectId: string
  ): Promise<CleanupReport> => {
    return {
      deletedCount: 0,
      failedCount: 0,
      stoppedAgents: 0,
    };
  },
} as const;

// ============================================================================
// Mock Steps API
// ============================================================================

export const mockStepsApi = {
  getByTask: async (taskId: string): Promise<TaskStep[]> => {
    const store = getStore();
    return store.taskSteps.get(taskId) ?? [];
  },

  create: async (
    taskId: string,
    data: { title: string; description?: string; sortOrder?: number }
  ): Promise<TaskStep> => {
    return createMockStep({
      id: generateTestUuid(),
      taskId,
      title: data.title,
      description: data.description ?? null,
      sortOrder: data.sortOrder ?? 0,
    });
  },

  update: async (
    stepId: string,
    data: { title?: string; description?: string; sortOrder?: number }
  ): Promise<TaskStep> => {
    return createMockStep({
      id: stepId,
      taskId: "mock-task-id",
      title: data.title ?? "Updated Step",
      description: data.description ?? null,
      sortOrder: data.sortOrder ?? 0,
    });
  },

  delete: async (_stepId: string): Promise<void> => {
    // Read-only mock: no-op
  },

  reorder: async (taskId: string, _stepIds: string[]): Promise<TaskStep[]> => {
    const store = getStore();
    return store.taskSteps.get(taskId) ?? [];
  },

  getProgress: async (taskId: string): Promise<StepProgressSummary> => {
    const store = getStore();
    const steps = store.taskSteps.get(taskId) ?? [];
    const total = steps.length;
    const completed = steps.filter((s) => s.status === "completed").length;
    const inProgress = steps.filter((s) => s.status === "in_progress").length;
    const pending = steps.filter((s) => s.status === "pending").length;
    const skipped = steps.filter((s) => s.status === "skipped").length;
    const failed = steps.filter((s) => s.status === "failed").length;

    const currentStep = steps.find((s) => s.status === "in_progress") ?? null;
    const nextStep = steps.find((s) => s.status === "pending") ?? null;

    const display = getStepProgressDisplay({
      total,
      completed,
      inProgress,
      skipped,
    });

    return {
      taskId,
      total,
      completed,
      inProgress,
      pending,
      skipped,
      failed,
      currentStep,
      nextStep,
      percentComplete: display.completedPercent,
    };
  },

  start: async (stepId: string): Promise<TaskStep> => {
    return createMockStep({
      id: stepId,
      taskId: "mock-task-id",
      title: "Started Step",
      status: "in_progress",
      startedAt: new Date().toISOString(),
    });
  },

  complete: async (stepId: string, note?: string): Promise<TaskStep> => {
    return createMockStep({
      id: stepId,
      taskId: "mock-task-id",
      title: "Completed Step",
      status: "completed",
      completionNote: note ?? null,
      completedAt: new Date().toISOString(),
    });
  },

  skip: async (stepId: string, reason: string): Promise<TaskStep> => {
    return createMockStep({
      id: stepId,
      taskId: "mock-task-id",
      title: "Skipped Step",
      status: "skipped",
      completionNote: reason,
    });
  },

  fail: async (stepId: string, error: string): Promise<TaskStep> => {
    return createMockStep({
      id: stepId,
      taskId: "mock-task-id",
      title: "Failed Step",
      status: "failed",
      completionNote: error,
    });
  },
} as const;
