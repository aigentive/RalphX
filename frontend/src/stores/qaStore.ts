/**
 * QA store using Zustand with immer middleware
 *
 * Manages QA settings and per-task QA data for the frontend.
 * Settings are global, while taskQA data is stored per-task in a Record.
 */

import { create } from "zustand";
import { immer } from "zustand/middleware/immer";

import { registerEnvIsolatedStore } from "@/lib/remote/env-state-isolation";
import { enableMapSet } from "immer";
import type { QASettings } from "@/types/qa-config";
import type { TaskQAResponse } from "@/lib/tauri";
import { DEFAULT_QA_SETTINGS } from "@/types/qa-config";

// Enable Immer's MapSet plugin for Set/Map support
enableMapSet();

// ============================================================================
// State Interface
// ============================================================================

interface QAState {
  /** Global QA settings */
  settings: QASettings;
  /** Whether settings have been loaded from backend */
  settingsLoaded: boolean;
  /** TaskQA data indexed by task ID */
  taskQA: Record<string, TaskQAResponse>;
  /** Loading state for settings */
  isLoadingSettings: boolean;
  /** Loading state for task QA (per task) */
  loadingTasks: Set<string>;
  /** Error message if any */
  error: string | null;
}

// ============================================================================
// Actions Interface
// ============================================================================

interface QAActions {
  /** Set global QA settings (after loading from backend) */
  setSettings: (settings: QASettings) => void;
  /** Update specific settings fields */
  updateSettings: (changes: Partial<QASettings>) => void;
  /** Mark settings as loading */
  setLoadingSettings: (loading: boolean) => void;
  /** Set task QA data for a specific task */
  setTaskQA: (taskId: string, data: TaskQAResponse | null) => void;
  /** Update task QA data with partial changes */
  updateTaskQA: (taskId: string, changes: Partial<TaskQAResponse>) => void;
  /** Mark a task as loading */
  setLoadingTask: (taskId: string, loading: boolean) => void;
  /** Set error message */
  setError: (error: string | null) => void;
  /** Clear all task QA data */
  clearTaskQA: () => void;
  /** Remove task QA data for a specific task */
  removeTaskQA: (taskId: string) => void;
}

// ============================================================================
// Store Implementation
// ============================================================================

export const useQAStore = create<QAState & QAActions>()(
  immer((set) => ({
    // Initial state
    settings: DEFAULT_QA_SETTINGS,
    settingsLoaded: false,
    taskQA: {},
    isLoadingSettings: false,
    loadingTasks: new Set(),
    error: null,

    // Actions
    setSettings: (settings) =>
      set((state) => {
        state.settings = settings;
        state.settingsLoaded = true;
        state.isLoadingSettings = false;
      }),

    updateSettings: (changes) =>
      set((state) => {
        Object.assign(state.settings, changes);
      }),

    setLoadingSettings: (loading) =>
      set((state) => {
        state.isLoadingSettings = loading;
      }),

    setTaskQA: (taskId, data) =>
      set((state) => {
        if (data === null) {
          delete state.taskQA[taskId];
        } else {
          state.taskQA[taskId] = data;
        }
        state.loadingTasks.delete(taskId);
      }),

    updateTaskQA: (taskId, changes) =>
      set((state) => {
        const existing = state.taskQA[taskId];
        if (existing) {
          Object.assign(existing, changes);
        }
      }),

    setLoadingTask: (taskId, loading) =>
      set((state) => {
        if (loading) {
          state.loadingTasks.add(taskId);
        } else {
          state.loadingTasks.delete(taskId);
        }
      }),

    setError: (error) =>
      set((state) => {
        state.error = error;
      }),

    clearTaskQA: () =>
      set((state) => {
        state.taskQA = {};
        state.loadingTasks = new Set();
      }),

    removeTaskQA: (taskId) =>
      set((state) => {
        delete state.taskQA[taskId];
        state.loadingTasks.delete(taskId);
      }),
  }))
);

registerEnvIsolatedStore({ name: "useQAStore", reset: () => useQAStore.setState(useQAStore.getInitialState(), true) });

// ============================================================================
// Selectors
// ============================================================================

/**
 * Select QA data for a specific task
 */
export const selectTaskQA =
  (taskId: string) =>
  (state: QAState): TaskQAResponse | null =>
    state.taskQA[taskId] ?? null;

/**
 * Check if QA is enabled globally
 */
export const selectIsQAEnabled = (state: QAState): boolean =>
  state.settings.qa_enabled;

/**
 * Check if a task is being loaded
 */
export const selectIsTaskLoading =
  (taskId: string) =>
  (state: QAState): boolean =>
    state.loadingTasks.has(taskId);

/**
 * Get QA results for a task
 */
export const selectTaskQAResults =
  (taskId: string) =>
  (state: QAState) =>
    state.taskQA[taskId]?.test_results ?? null;

/**
 * Check if a task has QA data
 */
export const selectHasTaskQA =
  (taskId: string) =>
  (state: QAState): boolean =>
    taskId in state.taskQA;
