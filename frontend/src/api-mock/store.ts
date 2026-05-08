/**
 * In-memory mock data store
 *
 * Provides a centralized store for mock data used by the mock API.
 * Data is seeded on initialization and can be queried/modified by mock API functions.
 * Resets to seed data on page reload (no persistence).
 */

import type { Task, InternalStatus } from "@/types/task";
import type { Project } from "@/types/project";
import type { TaskStep } from "@/types/task-step";
import type { ChatConversation } from "@/types/chat-conversation";
import { AGENT_WORKER } from "@/constants/agents";
import {
  createMockTask,
  createMockProject,
  generateTestUuid,
} from "@/test/mock-data";

// ============================================================================
// Store State
// ============================================================================

export interface MockStore {
  projects: Map<string, Project>;
  tasks: Map<string, Task>;
  taskSteps: Map<string, TaskStep[]>;
  conversations: Map<string, ChatConversation>;
  activePlans?: Map<string, string | null>; // projectId -> sessionId | null
  sessions?: Map<string, unknown>; // sessionId -> session data
  initialized: boolean;
}

const store: MockStore = {
  projects: new Map(),
  tasks: new Map(),
  taskSteps: new Map(),
  conversations: new Map(),
  initialized: false,
};

// ============================================================================
// Seed Data
// ============================================================================

function seedMockData(): void {
  if (store.initialized) return;

  // Create a demo project
  const project = createMockProject({
    id: "project-mock-1",
    name: "Demo Project",
    workingDirectory: "/demo/project",
  });
  store.projects.set(project.id, project);

  // Create tasks in various states for visual testing
  // Using actual InternalStatus values
  const statuses: { status: InternalStatus; title: string }[] = [
    { status: "backlog", title: "Backlog Task" },
    { status: "ready", title: "Ready Task" },
    { status: "blocked", title: "Blocked Task" },
    { status: "executing", title: "Executing Task" },
    { status: "pending_review", title: "Pending Review Task" },
    { status: "review_passed", title: "Review Passed Task" },
    { status: "approved", title: "Approved Task" },
  ];

  statuses.forEach(({ status, title }, idx) => {
    const planArtifactId =
      status === "approved"
        ? "plan-mock-2"
        : idx < 4
        ? (idx % 2 === 0 ? "plan-mock-1" : "plan-mock-2")
        : null;
    const task = createMockTask({
      id: `task-mock-${idx + 1}`,
      projectId: project.id,
      title,
      description: `A sample task in ${status} status for visual testing`,
      internalStatus: status,
      priority: idx,
      blockedReason: status === "blocked" ? "Waiting for dependencies" : null,
      planArtifactId,
    });
    store.tasks.set(task.id, task);

    // Add steps for active/completed cards so v29a progress chrome is visible.
    if (["executing", "pending_review", "approved"].includes(status)) {
      const allStepsComplete = status === "approved";
      const secondStepComplete = allStepsComplete || status === "pending_review";
      const steps: TaskStep[] = [
        {
          id: generateTestUuid(),
          taskId: task.id,
          title: "Setup environment",
          description: "Configure dev environment",
          status: "completed",
          sortOrder: 0,
          dependsOn: null,
          createdBy: AGENT_WORKER,
          completionNote: "Environment ready",
          createdAt: new Date().toISOString(),
          updatedAt: new Date().toISOString(),
          startedAt: new Date().toISOString(),
          completedAt: new Date().toISOString(),
        },
        {
          id: generateTestUuid(),
          taskId: task.id,
          title: "Implement feature",
          description: "Write the main feature code",
          status: secondStepComplete ? "completed" : "in_progress",
          sortOrder: 1,
          dependsOn: null,
          createdBy: AGENT_WORKER,
          completionNote: null,
          createdAt: new Date().toISOString(),
          updatedAt: new Date().toISOString(),
          startedAt: new Date().toISOString(),
          completedAt: secondStepComplete ? new Date().toISOString() : null,
        },
        {
          id: generateTestUuid(),
          taskId: task.id,
          title: "Write tests",
          description: "Add unit and integration tests",
          status: allStepsComplete ? "completed" : "pending",
          sortOrder: 2,
          dependsOn: null,
          createdBy: AGENT_WORKER,
          completionNote: null,
          createdAt: new Date().toISOString(),
          updatedAt: new Date().toISOString(),
          startedAt: allStepsComplete ? new Date().toISOString() : null,
          completedAt: allStepsComplete ? new Date().toISOString() : null,
        },
      ];
      store.taskSteps.set(task.id, steps);
    }
  });

  // Add a few extra tasks for better visual representation
  for (let i = 0; i < 3; i++) {
    const task = createMockTask({
      id: `task-mock-extra-${i + 1}`,
      projectId: project.id,
      title: `Additional Task ${i + 1}`,
      description: "Extra task for visual density",
      internalStatus: "backlog",
      priority: 10 + i,
      blockedReason: null,
      planArtifactId: i === 0 ? "plan-mock-2" : null,
    });
    store.tasks.set(task.id, task);
  }

  // Add a merge_incomplete task with metadata for visual testing
  const mergeIncompleteTask = createMockTask({
    id: "task-mock-merge-incomplete",
    projectId: project.id,
    title: "Merge Incomplete Task",
    description: "A task whose merge failed with a git error",
    category: "plan_merge",
    internalStatus: "merge_incomplete",
    priority: 20,
    planArtifactId: "plan-mock-2",
    taskBranch: "ralphx/demo/task-abc123",
    metadata: JSON.stringify({
      error: "fatal: Not possible to fast-forward, aborting.\nerror: could not apply fa1afe1... feat: implement task filtering",
      source_branch: "ralphx/demo/task-abc123",
      target_branch: "main",
    }),
  });
  store.tasks.set(mergeIncompleteTask.id, mergeIncompleteTask);

  store.initialized = true;
}

// ============================================================================
// Store Access Functions
// ============================================================================

export function getStore(): MockStore {
  if (!store.initialized) {
    seedMockData();
  }

  // Expose store to window in web mode for Playwright testing
  if (typeof window !== 'undefined') {
    window.__mockStore = store;
  }

  return store;
}

export function resetStore(): void {
  store.projects.clear();
  store.tasks.clear();
  store.taskSteps.clear();
  store.conversations.clear();
  store.initialized = false;
  seedMockData();
}
