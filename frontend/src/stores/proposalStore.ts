/**
 * Proposal store using Zustand with immer middleware
 *
 * Manages task proposal state for the frontend. Proposals are stored in a
 * Record keyed by proposal ID for O(1) lookup.
 */

import { create } from "zustand";
import { immer } from "zustand/middleware/immer";

import { registerEnvIsolatedStore } from "@/lib/remote/env-state-isolation";
import type { TaskProposal, Priority } from "@/types/ideation";

// ============================================================================
// State Interface
// ============================================================================

interface ProposalState {
  /** Proposals indexed by ID for O(1) lookup */
  proposals: Record<string, TaskProposal>;
  /** Loading state for async operations */
  isLoading: boolean;
  /** Error message, or null if no error */
  error: string | null;
  /** Timestamp when last proposal was added, keyed by sessionId */
  lastProposalAddedAt: Record<string, number>;
  /** Timestamp when proposal content changed (dependency refresh hint), keyed by sessionId */
  lastDependencyRefreshRequestedAt: Record<string, number>;
  /** Timestamp when a proposal was last updated, keyed by sessionId */
  lastProposalUpdatedAt: Record<string, number>;
  /** ID of the last updated proposal, keyed by sessionId */
  lastUpdatedProposalId: Record<string, string>;
}

// ============================================================================
// Actions Interface
// ============================================================================

interface ProposalActions {
  /** Replace all proposals with new array (converts to Record) */
  setProposals: (proposals: TaskProposal[]) => void;
  /** Add a single proposal to the store */
  addProposal: (proposal: TaskProposal) => void;
  /** Update a specific proposal with partial changes */
  updateProposal: (proposalId: string, changes: Partial<TaskProposal>) => void;
  /** Remove a proposal from the store */
  removeProposal: (proposalId: string) => void;
  /** Update sort order based on position in array */
  reorder: (proposalIds: string[]) => void;
  /** Set loading state */
  setLoading: (isLoading: boolean) => void;
  /** Set error message */
  setError: (error: string | null) => void;
  /** Clear error message */
  clearError: () => void;
}

// ============================================================================
// Store Implementation
// ============================================================================

export const useProposalStore = create<ProposalState & ProposalActions>()(
  immer((set) => ({
    // Initial state
    proposals: {},
    isLoading: false,
    error: null,
    lastProposalAddedAt: {},
    lastDependencyRefreshRequestedAt: {},
    lastProposalUpdatedAt: {},
    lastUpdatedProposalId: {},

    // Actions
    setProposals: (proposals) =>
      set((state) => {
        state.proposals = Object.fromEntries(proposals.map((p) => [p.id, p]));
      }),

    addProposal: (proposal) =>
      set((state) => {
        state.proposals[proposal.id] = proposal;
        state.lastProposalAddedAt[proposal.sessionId] = Date.now();
        state.lastDependencyRefreshRequestedAt[proposal.sessionId] = Date.now();
      }),

    updateProposal: (proposalId, changes) =>
      set((state) => {
        const proposal = state.proposals[proposalId];
        if (proposal) {
          const sessionId = proposal.sessionId;
          const contentFieldsChanged = [
            "title",
            "description",
            "steps",
            "acceptanceCriteria",
            "category",
          ].some((field) => Object.prototype.hasOwnProperty.call(changes, field));
          Object.assign(proposal, changes);
          state.lastProposalUpdatedAt[sessionId] = Date.now();
          state.lastUpdatedProposalId[sessionId] = proposalId;
          if (contentFieldsChanged) {
            state.lastDependencyRefreshRequestedAt[sessionId] = Date.now();
          }
        }
      }),

    removeProposal: (proposalId) =>
      set((state) => {
        const sessionId = state.proposals[proposalId]?.sessionId;
        delete state.proposals[proposalId];
        if (!sessionId) return;
        state.lastDependencyRefreshRequestedAt[sessionId] = Date.now();
      }),

    reorder: (proposalIds) =>
      set((state) => {
        proposalIds.forEach((id, index) => {
          const proposal = state.proposals[id];
          if (proposal) {
            proposal.sortOrder = index;
          }
        });
      }),

    setLoading: (isLoading) =>
      set((state) => {
        state.isLoading = isLoading;
      }),

    setError: (error) =>
      set((state) => {
        state.error = error;
      }),

    clearError: () =>
      set((state) => {
        state.error = null;
      }),
  }))
);

registerEnvIsolatedStore({ name: "useProposalStore", reset: () => useProposalStore.setState(useProposalStore.getInitialState(), true) });

if (typeof window !== "undefined" && !window.__TAURI_INTERNALS__) {
  window.__proposalStore = useProposalStore;
}

// ============================================================================
// Selectors (defined outside store for memoization)
// ============================================================================

/**
 * Select all proposals for a specific session
 * @param sessionId - The session ID to filter by
 * @returns Selector function returning matching proposals
 */
export const selectProposalsBySession =
  (sessionId: string) =>
  (state: ProposalState): TaskProposal[] =>
    Object.values(state.proposals).filter((p) => p.sessionId === sessionId);

/**
 * Select all proposals with a specific priority
 * @param priority - The priority to filter by
 * @returns Selector function returning matching proposals
 */
export const selectProposalsByPriority =
  (priority: Priority) =>
  (state: ProposalState): TaskProposal[] =>
    Object.values(state.proposals).filter((p) => p.suggestedPriority === priority);

/**
 * Select proposals for a session sorted by sortOrder
 * @param sessionId - The session ID to filter by
 * @returns Selector function returning sorted proposals
 */
export const selectSortedProposals =
  (sessionId: string) =>
  (state: ProposalState): TaskProposal[] =>
    Object.values(state.proposals)
      .filter((p) => p.sessionId === sessionId)
      .sort((a, b) => a.sortOrder - b.sortOrder);
