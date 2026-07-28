/**
 * Methodology store using Zustand with immer middleware
 *
 * Manages methodology state for the frontend. Methodologies are configuration
 * packages that bring workflow, agents, skills, phases, and templates together.
 */

import { create } from "zustand";
import { immer } from "zustand/middleware/immer";

import { registerEnvIsolatedStore } from "@/lib/remote/env-state-isolation";

// ============================================================================
// Types (matching API response structure)
// ============================================================================

export interface MethodologyPhase {
  id: string;
  name: string;
  order: number;
  description: string | null;
  agentProfiles: string[];
  columnIds: string[];
}

export interface MethodologyTemplate {
  artifactType: string;
  templatePath: string;
  name: string | null;
  description: string | null;
}

export interface Methodology {
  id: string;
  name: string;
  description: string | null;
  agentProfiles: string[];
  skills: string[];
  workflowId: string;
  workflowName: string;
  phases: MethodologyPhase[];
  templates: MethodologyTemplate[];
  isActive: boolean;
  phaseCount: number;
  agentCount: number;
  createdAt: string;
}

// ============================================================================
// State Interface
// ============================================================================

interface MethodologyState {
  /** Methodologies indexed by ID for O(1) lookup */
  methodologies: Record<string, Methodology>;
  /** Currently active methodology ID, or null if none */
  activeMethodologyId: string | null;
  /** Loading state for async operations */
  isLoading: boolean;
  /** Whether methodology activation is in progress */
  isActivating: boolean;
  /** Error message if last operation failed */
  error: string | null;
}

// ============================================================================
// Actions Interface
// ============================================================================

interface MethodologyActions {
  /** Replace all methodologies with new array (converts to Record) */
  setMethodologies: (methodologies: Methodology[]) => void;
  /** Set the active methodology by ID */
  setActiveMethodology: (methodologyId: string | null) => void;
  /** Activate a methodology, deactivating any previously active one */
  activateMethodology: (methodologyId: string) => void;
  /** Deactivate a methodology */
  deactivateMethodology: (methodologyId: string) => void;
  /** Update a specific methodology with partial changes */
  updateMethodology: (methodologyId: string, changes: Partial<Methodology>) => void;
  /** Set loading state */
  setLoading: (isLoading: boolean) => void;
  /** Set activating state */
  setActivating: (isActivating: boolean) => void;
  /** Set error message */
  setError: (error: string | null) => void;
}

// ============================================================================
// Store Implementation
// ============================================================================

export const useMethodologyStore = create<MethodologyState & MethodologyActions>()(
  immer((set) => ({
    // Initial state
    methodologies: {},
    activeMethodologyId: null,
    isLoading: false,
    isActivating: false,
    error: null,

    // Actions
    setMethodologies: (methodologies) =>
      set((state) => {
        state.methodologies = Object.fromEntries(methodologies.map((m) => [m.id, m]));
        // Set active methodology ID if one is marked as active
        const activeMethodology = methodologies.find((m) => m.isActive);
        state.activeMethodologyId = activeMethodology?.id ?? null;
      }),

    setActiveMethodology: (methodologyId) =>
      set((state) => {
        state.activeMethodologyId = methodologyId;
      }),

    activateMethodology: (methodologyId) =>
      set((state) => {
        // Check if methodology exists
        const methodology = state.methodologies[methodologyId];
        if (!methodology) return;

        // Deactivate any currently active methodology
        if (state.activeMethodologyId && state.activeMethodologyId !== methodologyId) {
          const current = state.methodologies[state.activeMethodologyId];
          if (current) {
            current.isActive = false;
          }
        }

        // Activate the new methodology
        methodology.isActive = true;
        state.activeMethodologyId = methodologyId;
      }),

    deactivateMethodology: (methodologyId) =>
      set((state) => {
        const methodology = state.methodologies[methodologyId];
        if (!methodology) return;

        methodology.isActive = false;

        // Clear active methodology ID if it matches
        if (state.activeMethodologyId === methodologyId) {
          state.activeMethodologyId = null;
        }
      }),

    updateMethodology: (methodologyId, changes) =>
      set((state) => {
        const methodology = state.methodologies[methodologyId];
        if (methodology) {
          Object.assign(methodology, changes);
        }
      }),

    setLoading: (isLoading) =>
      set((state) => {
        state.isLoading = isLoading;
      }),

    setActivating: (isActivating) =>
      set((state) => {
        state.isActivating = isActivating;
      }),

    setError: (error) =>
      set((state) => {
        state.error = error;
      }),
  }))
);

registerEnvIsolatedStore({ name: "useMethodologyStore", reset: () => useMethodologyStore.setState(useMethodologyStore.getInitialState(), true) });

// ============================================================================
// Selectors (defined outside store for memoization)
// ============================================================================

/**
 * Select the currently active methodology
 * @returns The active methodology, or null if none
 */
export const selectActiveMethodology = (
  state: MethodologyState & MethodologyActions
): Methodology | null =>
  state.activeMethodologyId ? state.methodologies[state.activeMethodologyId] ?? null : null;

/**
 * Select a methodology by ID
 * @param methodologyId - The methodology ID to find
 * @returns Selector function returning the methodology or undefined
 */
export const selectMethodologyById =
  (methodologyId: string) =>
  (state: MethodologyState): Methodology | undefined =>
    state.methodologies[methodologyId];

/**
 * Select phases for the active methodology
 * @returns Array of methodology phases, or empty array if no active methodology
 */
export const selectMethodologyPhases = (
  state: MethodologyState & MethodologyActions
): MethodologyPhase[] => {
  const methodology = selectActiveMethodology(state);
  return methodology?.phases ?? [];
};

/**
 * Select all methodologies as an array
 * @returns Array of all methodologies
 */
export const selectAllMethodologies = (state: MethodologyState): Methodology[] =>
  Object.values(state.methodologies);

/**
 * Select agent profiles for the active methodology
 * @returns Array of agent profile IDs, or empty array if no active methodology
 */
export const selectMethodologyAgentProfiles = (
  state: MethodologyState & MethodologyActions
): string[] => {
  const methodology = selectActiveMethodology(state);
  return methodology?.agentProfiles ?? [];
};

/**
 * Select skills for the active methodology
 * @returns Array of skill paths, or empty array if no active methodology
 */
export const selectMethodologySkills = (
  state: MethodologyState & MethodologyActions
): string[] => {
  const methodology = selectActiveMethodology(state);
  return methodology?.skills ?? [];
};
