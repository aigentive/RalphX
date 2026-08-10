/**
 * Workflow store using Zustand with immer middleware
 *
 * Manages workflow state for the frontend. Workflows define the Kanban
 * column layout and map external columns to internal statuses.
 */

import { create } from "zustand";
import { immer } from "zustand/middleware/immer";

import { registerEnvIsolatedStore } from "@/lib/remote/env-state-isolation";
import type { WorkflowSchema, WorkflowColumn } from "@/types/workflow";

// ============================================================================
// State Interface
// ============================================================================

interface WorkflowState {
  /** Workflows indexed by ID for O(1) lookup */
  workflows: Record<string, WorkflowSchema>;
  /** Currently active workflow ID, or null if none */
  activeWorkflowId: string | null;
  /** Loading state for async operations */
  isLoading: boolean;
  /** Error message if last operation failed */
  error: string | null;
}

// ============================================================================
// Actions Interface
// ============================================================================

interface WorkflowActions {
  /** Replace all workflows with new array (converts to Record) */
  setWorkflows: (workflows: WorkflowSchema[]) => void;
  /** Set the active workflow by ID */
  setActiveWorkflow: (workflowId: string | null) => void;
  /** Add a single workflow to the store */
  addWorkflow: (workflow: WorkflowSchema) => void;
  /** Update a specific workflow with partial changes */
  updateWorkflow: (workflowId: string, changes: Partial<WorkflowSchema>) => void;
  /** Set loading state */
  setLoading: (isLoading: boolean) => void;
  /** Set error message */
  setError: (error: string | null) => void;
}

// ============================================================================
// Store Implementation
// ============================================================================

export const useWorkflowStore = create<WorkflowState & WorkflowActions>()(
  immer((set) => ({
    // Initial state
    workflows: {},
    activeWorkflowId: null,
    isLoading: false,
    error: null,

    // Actions
    setWorkflows: (workflows) =>
      set((state) => {
        state.workflows = Object.fromEntries(workflows.map((w) => [w.id, w]));
        // Set active workflow to the default one if present
        const defaultWorkflow = workflows.find((w) => w.isDefault);
        if (defaultWorkflow && !state.activeWorkflowId) {
          state.activeWorkflowId = defaultWorkflow.id;
        }
      }),

    setActiveWorkflow: (workflowId) =>
      set((state) => {
        state.activeWorkflowId = workflowId;
      }),

    addWorkflow: (workflow) =>
      set((state) => {
        state.workflows[workflow.id] = workflow;
        // Set as active if it's default and no active workflow
        if (workflow.isDefault && !state.activeWorkflowId) {
          state.activeWorkflowId = workflow.id;
        }
      }),

    updateWorkflow: (workflowId, changes) =>
      set((state) => {
        const workflow = state.workflows[workflowId];
        if (workflow) {
          Object.assign(workflow, changes);
        }
      }),

    setLoading: (isLoading) =>
      set((state) => {
        state.isLoading = isLoading;
      }),

    setError: (error) =>
      set((state) => {
        state.error = error;
      }),
  }))
);

registerEnvIsolatedStore({ name: "useWorkflowStore", reset: () => useWorkflowStore.setState(useWorkflowStore.getInitialState(), true) });

// ============================================================================
// Selectors (defined outside store for memoization)
// ============================================================================

/**
 * Select the currently active workflow
 * @returns The active workflow, or null if none
 */
export const selectActiveWorkflow = (
  state: WorkflowState & WorkflowActions
): WorkflowSchema | null =>
  state.activeWorkflowId ? state.workflows[state.activeWorkflowId] ?? null : null;

/**
 * Select columns for the active workflow
 * @returns Array of workflow columns, or empty array if no active workflow
 */
export const selectWorkflowColumns = (
  state: WorkflowState & WorkflowActions
): WorkflowColumn[] => {
  const workflow = selectActiveWorkflow(state);
  return workflow?.columns ?? [];
};

/**
 * Select a workflow by ID
 * @param workflowId - The workflow ID to find
 * @returns Selector function returning the workflow or undefined
 */
export const selectWorkflowById =
  (workflowId: string) =>
  (state: WorkflowState): WorkflowSchema | undefined =>
    state.workflows[workflowId];
