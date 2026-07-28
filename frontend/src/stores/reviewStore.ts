/**
 * Review store using Zustand with immer middleware
 *
 * Manages pending reviews and review-related state for the frontend.
 * Under 100 lines as per PRD requirements.
 */

import { create } from "zustand";
import { immer } from "zustand/middleware/immer";

import { registerEnvIsolatedStore } from "@/lib/remote/env-state-isolation";
import type { ReviewResponse } from "@/lib/tauri";

// ============================================================================
// State Interface
// ============================================================================

interface ReviewState {
  /** Pending reviews indexed by review ID */
  pendingReviews: Record<string, ReviewResponse>;
  /** Currently selected review ID */
  selectedReviewId: string | null;
  /** Whether reviews are being loaded */
  isLoading: boolean;
  /** Error message if any */
  error: string | null;
}

// ============================================================================
// Actions Interface
// ============================================================================

interface ReviewActions {
  /** Set all pending reviews (replaces existing) */
  setPendingReviews: (reviews: ReviewResponse[]) => void;
  /** Add or update a single review */
  setReview: (review: ReviewResponse) => void;
  /** Remove a review by ID */
  removeReview: (reviewId: string) => void;
  /** Select a review */
  selectReview: (reviewId: string | null) => void;
  /** Set loading state */
  setLoading: (loading: boolean) => void;
  /** Set error message */
  setError: (error: string | null) => void;
  /** Clear all reviews */
  clearReviews: () => void;
}

// ============================================================================
// Store Implementation
// ============================================================================

export const useReviewStore = create<ReviewState & ReviewActions>()(
  immer((set) => ({
    // Initial state
    pendingReviews: {},
    selectedReviewId: null,
    isLoading: false,
    error: null,

    // Actions
    setPendingReviews: (reviews) =>
      set((state) => {
        state.pendingReviews = {};
        for (const review of reviews) {
          state.pendingReviews[review.id] = review;
        }
        state.isLoading = false;
      }),

    setReview: (review) =>
      set((state) => {
        state.pendingReviews[review.id] = review;
      }),

    removeReview: (reviewId) =>
      set((state) => {
        delete state.pendingReviews[reviewId];
        if (state.selectedReviewId === reviewId) {
          state.selectedReviewId = null;
        }
      }),

    selectReview: (reviewId) =>
      set((state) => {
        state.selectedReviewId = reviewId;
      }),

    setLoading: (loading) =>
      set((state) => {
        state.isLoading = loading;
      }),

    setError: (error) =>
      set((state) => {
        state.error = error;
        state.isLoading = false;
      }),

    clearReviews: () =>
      set((state) => {
        state.pendingReviews = {};
        state.selectedReviewId = null;
      }),
  }))
);

registerEnvIsolatedStore({ name: "useReviewStore", reset: () => useReviewStore.setState(useReviewStore.getInitialState(), true) });

// ============================================================================
// Selectors
// ============================================================================

/** Select all pending reviews as an array */
export const selectPendingReviewsList = (state: ReviewState): ReviewResponse[] =>
  Object.values(state.pendingReviews);

/** Select a review by ID */
export const selectReviewById =
  (reviewId: string) =>
  (state: ReviewState): ReviewResponse | null =>
    state.pendingReviews[reviewId] ?? null;

/** Select the currently selected review */
export const selectSelectedReview = (state: ReviewState): ReviewResponse | null =>
  state.selectedReviewId ? state.pendingReviews[state.selectedReviewId] ?? null : null;

/** Get count of pending reviews */
export const selectPendingReviewCount = (state: ReviewState): number =>
  Object.keys(state.pendingReviews).length;

/** Check if a review is selected */
export const selectIsReviewSelected =
  (reviewId: string) =>
  (state: ReviewState): boolean =>
    state.selectedReviewId === reviewId;
