/**
 * Plan store using Zustand with immer middleware
 *
 * Manages active plan state per project and cached plan candidates.
 * The active plan determines which tasks are displayed in Graph and Kanban views.
 * Uses Record<projectId, sessionId | null> for O(1) lookup.
 */

import { create } from "zustand";
import { immer } from "zustand/middleware/immer";

import { registerEnvIsolatedStore } from "@/lib/remote/env-state-isolation";
import {
  planApi,
  type PlanCandidateResponse,
  type SelectionSource,
} from "@/api/plan";
import { executionPlanApi } from "@/api/executionPlan";

// ============================================================================
// State Interface
// ============================================================================

interface PlanState {
  /** Active plan session ID by project ID (projectId → sessionId | null) */
  activePlanByProject: Record<string, string | null>;
  /** Active execution plan ID by project ID (projectId → executionPlanId | null) */
  activeExecutionPlanIdByProject: Record<string, string | null>;
  /** Tracks whether active plan has been loaded at least once for a project */
  activePlanLoadedByProject: Record<string, boolean>;
  /** Cached plan candidates (from last loadCandidates call) */
  planCandidates: PlanCandidate[];
  /** Loading state for async operations */
  isLoading: boolean;
  /** Error message from last failed operation */
  error: string | null;
}

// ============================================================================
// Actions Interface
// ============================================================================

interface PlanActions {
  /** Load active plan for a project from backend */
  loadActivePlan: (projectId: string) => Promise<void>;
  /** Set active plan for a project (with source tracking).
   *  Pass executionPlanId to set both atomically and skip the async fetch. */
  setActivePlan: (
    projectId: string,
    sessionId: string,
    source: SelectionSource,
    executionPlanId?: string | null
  ) => Promise<void>;
  /** Clear active plan for a project */
  clearActivePlan: (projectId: string) => Promise<void>;
  /** Load plan candidates for selector (not implemented yet - will call list_plan_selector_candidates) */
  loadCandidates: (projectId: string, query?: string) => Promise<void>;
}

// ============================================================================
// Types
// ============================================================================


export interface PlanCandidate {
  sessionId: string;
  title: string | null;
  acceptedAt: string;
  taskStats: TaskStats;
  interactionStats: InteractionStats;
  score: number;
}

export interface TaskStats {
  total: number;
  incomplete: number;
  activeNow: number;
}

export interface InteractionStats {
  selectedCount: number;
  lastSelectedAt: string | null;
}

// ============================================================================
// Store Implementation
// ============================================================================

export const usePlanStore = create<PlanState & PlanActions>()(
  immer((set) => ({
    // Initial state
    activePlanByProject: {},
    activeExecutionPlanIdByProject: {},
    activePlanLoadedByProject: {},
    planCandidates: [],
    isLoading: false,
    error: null,

    // Actions
    loadActivePlan: async (projectId) => {
      try {
        set({ isLoading: true, error: null });
        // Use allSettled so a failed executionPlan fetch doesn't block the activePlan fetch.
        const [planResult, execPlanResult] = await Promise.allSettled([
          planApi.getActivePlan(projectId),
          executionPlanApi.getActiveExecutionPlan(projectId),
        ]);
        if (planResult.status === "rejected") {
          throw planResult.reason;
        }
        set((state) => {
          state.activePlanByProject[projectId] = planResult.value;
          state.activeExecutionPlanIdByProject[projectId] =
            execPlanResult.status === "fulfilled" ? execPlanResult.value : null;
          state.activePlanLoadedByProject[projectId] = true;
          state.isLoading = false;
        });
      } catch (error) {
        set((state) => {
          state.error = error instanceof Error ? error.message : "Failed to load active plan";
          state.isLoading = false;
          // Mark as loaded even on error to avoid indefinite placeholder limbo.
          state.activePlanLoadedByProject[projectId] = true;
        });
      }
    },

    setActivePlan: async (projectId, sessionId, source, executionPlanId?) => {
      const previousSessionId = usePlanStore.getState().activePlanByProject[projectId] ?? null;
      const previousExecutionPlanId =
        usePlanStore.getState().activeExecutionPlanIdByProject[projectId] ?? null;
      try {
        // Optimistic UI update — set both plan and executionPlanId atomically when provided.
        // This eliminates the gap where activePlanByProject is set but activeExecutionPlanIdByProject is null.
        set((state) => {
          state.isLoading = true;
          state.error = null;
          state.activePlanByProject[projectId] = sessionId;
          state.activePlanLoadedByProject[projectId] = true;
          if (executionPlanId !== undefined) {
            state.activeExecutionPlanIdByProject[projectId] = executionPlanId;
          }
        });
        await planApi.setActivePlan(projectId, sessionId, source);
        if (executionPlanId === undefined) {
          // executionPlanId not provided upfront — fetch async (e.g. "View Work" on accepted session)
          const fetchedId = await executionPlanApi.getActiveExecutionPlan(projectId);
          set((state) => {
            state.activeExecutionPlanIdByProject[projectId] = fetchedId;
            state.isLoading = false;
          });
        } else {
          set({ isLoading: false });
        }
      } catch (error) {
        set((state) => {
          state.error = error instanceof Error ? error.message : "Failed to set active plan";
          state.isLoading = false;
          // Roll back optimistic updates on failure.
          state.activePlanByProject[projectId] = previousSessionId;
          if (executionPlanId !== undefined) {
            state.activeExecutionPlanIdByProject[projectId] = previousExecutionPlanId;
          }
        });
        throw error; // Re-throw so callers can handle
      }
    },

    clearActivePlan: async (projectId) => {
      try {
        set({ isLoading: true, error: null });
        await planApi.clearActivePlan(projectId);
        set((state) => {
          state.activePlanByProject[projectId] = null;
          state.activeExecutionPlanIdByProject[projectId] = null;
          state.activePlanLoadedByProject[projectId] = true;
          state.isLoading = false;
        });
      } catch (error) {
        set({
          error: error instanceof Error ? error.message : "Failed to clear active plan",
          isLoading: false,
        });
        throw error;
      }
    },

    loadCandidates: async (projectId, query) => {
      set({ isLoading: true, error: null });
      try {
        const candidates: PlanCandidateResponse[] = await planApi.listCandidates(
          projectId,
          query
        );
        set({ planCandidates: candidates, isLoading: false });
      } catch (error) {
        set({
          error: error instanceof Error ? error.message : "Failed to load candidates",
          isLoading: false,
        });
      }
    },
  }))
);

registerEnvIsolatedStore({ name: "usePlanStore", reset: () => usePlanStore.setState(usePlanStore.getInitialState(), true) });

if (typeof window !== "undefined" && !window.__TAURI_INTERNALS__) {
  window.__planStore = usePlanStore;
}

// ============================================================================
// Selectors (defined outside store for memoization)
// ============================================================================

/**
 * Select the active plan ID for a specific project
 * @param projectId - The project ID to look up
 * @returns Selector function returning the session ID or null
 */
export const selectActivePlanId =
  (projectId: string) =>
  (state: PlanState): string | null =>
    state.activePlanByProject[projectId] ?? null;

/**
 * Select the active execution plan ID for a specific project
 * @param projectId - The project ID to look up
 * @returns Selector function returning the execution plan ID or null
 */
export const selectActiveExecutionPlanId =
  (projectId: string) =>
  (state: PlanState): string | null =>
    state.activeExecutionPlanIdByProject[projectId] ?? null;

/**
 * Select the active plan for the current active project
 * Requires the state to include activeProjectId (from project store)
 * @returns The active plan session ID or null
 */
export const selectCurrentActivePlan = (
  state: PlanState & { activeProjectId: string | null }
): string | null =>
  state.activeProjectId
    ? state.activePlanByProject[state.activeProjectId] ?? null
    : null;
