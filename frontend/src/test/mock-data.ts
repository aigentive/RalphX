/**
 * Mock data factories for testing
 *
 * Provides factory functions to create consistent test data
 * for tasks, projects, and other entities.
 */

import type { Task, InternalStatus, TaskCategory } from "@/types/task";
import type { Project, GitMode } from "@/types/project";
import type { AgentMessageEvent, SupervisorAlertEvent } from "@/types/events";

// ============================================================================
// UUID Generators
// ============================================================================

let uuidCounter = 0;

/**
 * Generate a test-safe UUID
 *
 * Creates UUIDs with a consistent format for testing.
 * Call resetUuidCounter() in beforeEach to ensure consistent IDs across tests.
 *
 * @returns A valid UUID string
 */
export function generateTestUuid(): string {
  uuidCounter++;
  const hex = uuidCounter.toString(16).padStart(12, "0");
  return `00000000-0000-4000-8000-${hex}`;
}

/**
 * Reset the UUID counter
 *
 * Call this in beforeEach to ensure predictable UUIDs across tests.
 */
export function resetUuidCounter(): void {
  uuidCounter = 0;
}

// ============================================================================
// Task Factories
// ============================================================================

/**
 * Create a mock task with sensible defaults
 *
 * @param overrides - Partial task data to override defaults
 * @returns A complete Task object
 *
 * @example
 * ```ts
 * const task = createMockTask({ title: "My Task" });
 * const completedTask = createMockTask({ internalStatus: "approved" });
 * ```
 */
export function createMockTask(overrides: Partial<Task> = {}): Task {
  const id = overrides.id ?? generateTestUuid();
  return {
    id,
    projectId: overrides.projectId ?? generateTestUuid(),
    category: "feature" as TaskCategory,
    title: "Test Task",
    description: null,
    priority: 0,
    internalStatus: "backlog" as InternalStatus,
    needsReviewPoint: false,
    createdAt: new Date().toISOString(),
    updatedAt: new Date().toISOString(),
    startedAt: null,
    completedAt: null,
    archivedAt: null,
    blockedReason: null,
    ...overrides,
  };
}

/**
 * Create multiple mock tasks
 *
 * @param count - Number of tasks to create
 * @param overrides - Partial task data to apply to all tasks
 * @returns Array of Task objects
 *
 * @example
 * ```ts
 * const tasks = createMockTasks(5, { projectId: "project-1" });
 * ```
 */
export function createMockTasks(
  count: number,
  overrides: Partial<Task> = {}
): Task[] {
  return Array.from({ length: count }, (_, i) =>
    createMockTask({
      title: `Task ${i + 1}`,
      ...overrides,
    })
  );
}

/**
 * Create a task in a specific status
 *
 * @param status - The internal status for the task
 * @param overrides - Additional overrides
 * @returns A Task in the specified status
 */
export function createTaskInStatus(
  status: InternalStatus,
  overrides: Partial<Task> = {}
): Task {
  const now = new Date().toISOString();
  // Statuses that indicate work has started
  const startedStatuses: InternalStatus[] = [
    "executing",
    "re_executing",
    "qa_refining",
    "qa_testing",
    "qa_passed",
    "qa_failed",
    "pending_review",
    "reviewing",
    "review_passed",
    "escalated",
    "revision_needed",
    "approved",
    "failed",
  ];
  // Terminal statuses
  const terminalStatuses: InternalStatus[] = ["approved", "merged", "failed", "cancelled", "stopped"];

  return createMockTask({
    internalStatus: status,
    startedAt: startedStatuses.includes(status) ? now : null,
    completedAt: terminalStatuses.includes(status) ? now : null,
    ...overrides,
  });
}

// ============================================================================
// Project Factories
// ============================================================================

/**
 * Create a mock project with sensible defaults
 *
 * @param overrides - Partial project data to override defaults
 * @returns A complete Project object
 *
 * @example
 * ```ts
 * const project = createMockProject({ name: "My Project" });
 * ```
 */
export function createMockProject(overrides: Partial<Project> = {}): Project {
  const id = overrides.id ?? generateTestUuid();
  return {
    id,
    name: "Test Project",
    workingDirectory: "/path/to/project",
    gitMode: "worktree" as GitMode,
    baseBranch: null,
    worktreeParentDirectory: null,
    useFeatureBranches: true,
    mergeValidationMode: "block",
    detectedAnalysis: null,
    customAnalysis: null,
    analyzedAt: null,
    githubPrEnabled: false,
    repositoryCapability: { kind: "localOnly" },
    createdAt: new Date().toISOString(),
    updatedAt: new Date().toISOString(),
    ...overrides,
  };
}

/**
 * Create multiple mock projects
 *
 * @param count - Number of projects to create
 * @param overrides - Partial project data to apply to all projects
 * @returns Array of Project objects
 */
export function createMockProjects(
  count: number,
  overrides: Partial<Project> = {}
): Project[] {
  return Array.from({ length: count }, (_, i) =>
    createMockProject({
      name: `Project ${i + 1}`,
      workingDirectory: `/path/to/project-${i + 1}`,
      ...overrides,
    })
  );
}

// ============================================================================
// Event Factories
// ============================================================================

/**
 * Create a mock agent message event
 *
 * @param overrides - Partial event data to override defaults
 * @returns An AgentMessageEvent object
 */
export function createMockAgentMessage(
  overrides: Partial<AgentMessageEvent> = {}
): AgentMessageEvent {
  return {
    taskId: generateTestUuid(),
    type: "thinking",
    content: "Processing...",
    timestamp: Date.now(),
    ...overrides,
  };
}

/**
 * Create a mock supervisor alert
 *
 * @param overrides - Partial event data to override defaults
 * @returns A SupervisorAlertEvent object
 */
export function createMockAlert(
  overrides: Partial<SupervisorAlertEvent> = {}
): SupervisorAlertEvent {
  return {
    taskId: generateTestUuid(),
    severity: "medium",
    type: "error",
    message: "Something went wrong",
    ...overrides,
  };
}

// ============================================================================
// Timestamp Helpers
// ============================================================================

/**
 * Create an ISO timestamp offset from now
 *
 * @param offsetMs - Milliseconds to offset (negative for past)
 * @returns ISO timestamp string
 *
 * @example
 * ```ts
 * const oneHourAgo = createTimestamp(-3600000);
 * const tomorrow = createTimestamp(86400000);
 * ```
 */
export function createTimestamp(offsetMs: number = 0): string {
  return new Date(Date.now() + offsetMs).toISOString();
}

/**
 * Create timestamps for common time offsets
 */
export const timestamps = {
  now: () => createTimestamp(0),
  minutesAgo: (n: number) => createTimestamp(-n * 60 * 1000),
  hoursAgo: (n: number) => createTimestamp(-n * 60 * 60 * 1000),
  daysAgo: (n: number) => createTimestamp(-n * 24 * 60 * 60 * 1000),
};
