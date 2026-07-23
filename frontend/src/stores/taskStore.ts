/**
 * Task store using Zustand with immer middleware
 *
 * Manages task state for the frontend. Tasks are stored in a Record
 * keyed by task ID for O(1) lookup. The store is synchronized with
 * backend state via Tauri events.
 */

import { create } from "zustand";
import { immer } from "zustand/middleware/immer";
import type { Task, InternalStatus } from "@/types/task";

// ============================================================================
// State Interface
// ============================================================================

interface TaskState {
  /** Tasks indexed by ID for O(1) lookup */
  tasks: Record<string, Task>;
}

// ============================================================================
// Actions Interface
// ============================================================================

interface TaskActions {
  /** Replace all tasks with new array (converts to Record) */
  setTasks: (tasks: Task[]) => void;
  /** Update a specific task with partial changes */
  updateTask: (taskId: string, changes: Partial<Task>) => void;
  /** Add a single task to the store */
  addTask: (task: Task) => void;
  /** Remove a task from the store */
  removeTask: (taskId: string) => void;
}

// ============================================================================
// Store Implementation
// ============================================================================

export const useTaskStore = create<TaskState & TaskActions>()(
  immer((set) => ({
    // Initial state
    tasks: {},

    // Actions
    setTasks: (tasks) =>
      set((state) => {
        state.tasks = Object.fromEntries(tasks.map((t) => [t.id, t]));
      }),

    updateTask: (taskId, changes) =>
      set((state) => {
        const task = state.tasks[taskId];
        if (task) {
          Object.assign(task, changes);
        }
      }),

    addTask: (task) =>
      set((state) => {
        state.tasks[task.id] = task;
      }),

    removeTask: (taskId) =>
      set((state) => {
        delete state.tasks[taskId];
      }),
  }))
);

// ============================================================================
// Selectors (defined outside store for memoization)
// ============================================================================

/**
 * Select all tasks with a specific status
 * @param status - The internal status to filter by
 * @returns Selector function returning matching tasks
 */
export const selectTasksByStatus =
  (status: InternalStatus) =>
  (state: TaskState): Task[] =>
    Object.values(state.tasks).filter((t) => t.internalStatus === status);
