/**
 * Artifact store using Zustand with immer middleware
 *
 * Manages artifact and bucket state for the frontend. Artifacts are
 * typed documents that flow between processes.
 */

import { create } from "zustand";
import { immer } from "zustand/middleware/immer";

import { registerEnvIsolatedStore } from "@/lib/remote/env-state-isolation";
import type { Artifact, ArtifactBucket, ArtifactType } from "@/types/artifact";

// ============================================================================
// State Interface
// ============================================================================

interface ArtifactState {
  /** Artifacts indexed by ID for O(1) lookup */
  artifacts: Record<string, Artifact>;
  /** Buckets indexed by ID for O(1) lookup */
  buckets: Record<string, ArtifactBucket>;
  /** Currently selected bucket ID, or null if none */
  selectedBucketId: string | null;
  /** Currently selected artifact ID, or null if none */
  selectedArtifactId: string | null;
  /** Loading state for async operations */
  isLoading: boolean;
  /** Error message if last operation failed */
  error: string | null;
}

// ============================================================================
// Actions Interface
// ============================================================================

interface ArtifactActions {
  /** Replace all artifacts with new array (converts to Record) */
  setArtifacts: (artifacts: Artifact[]) => void;
  /** Replace all buckets with new array (converts to Record) */
  setBuckets: (buckets: ArtifactBucket[]) => void;
  /** Set the selected bucket by ID (clears artifact selection if bucket changes) */
  setSelectedBucket: (bucketId: string | null) => void;
  /** Set the selected artifact by ID */
  setSelectedArtifact: (artifactId: string | null) => void;
  /** Add a single artifact to the store */
  addArtifact: (artifact: Artifact) => void;
  /** Update a specific artifact with partial changes */
  updateArtifact: (artifactId: string, changes: Partial<Artifact>) => void;
  /** Add a single bucket to the store */
  addBucket: (bucket: ArtifactBucket) => void;
  /** Set loading state */
  setLoading: (isLoading: boolean) => void;
  /** Set error message */
  setError: (error: string | null) => void;
}

// ============================================================================
// Store Implementation
// ============================================================================

export const useArtifactStore = create<ArtifactState & ArtifactActions>()(
  immer((set) => ({
    // Initial state
    artifacts: {},
    buckets: {},
    selectedBucketId: null,
    selectedArtifactId: null,
    isLoading: false,
    error: null,

    // Actions
    setArtifacts: (artifacts) =>
      set((state) => {
        state.artifacts = Object.fromEntries(artifacts.map((a) => [a.id, a]));
      }),

    setBuckets: (buckets) =>
      set((state) => {
        state.buckets = Object.fromEntries(buckets.map((b) => [b.id, b]));
      }),

    setSelectedBucket: (bucketId) =>
      set((state) => {
        // Clear artifact selection when bucket changes
        if (state.selectedBucketId !== bucketId) {
          state.selectedArtifactId = null;
        }
        state.selectedBucketId = bucketId;
      }),

    setSelectedArtifact: (artifactId) =>
      set((state) => {
        state.selectedArtifactId = artifactId;
      }),

    addArtifact: (artifact) =>
      set((state) => {
        state.artifacts[artifact.id] = artifact;
      }),

    updateArtifact: (artifactId, changes) =>
      set((state) => {
        const artifact = state.artifacts[artifactId];
        if (artifact) {
          Object.assign(artifact, changes);
        }
      }),

    addBucket: (bucket) =>
      set((state) => {
        state.buckets[bucket.id] = bucket;
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

registerEnvIsolatedStore({ name: "useArtifactStore", reset: () => useArtifactStore.setState(useArtifactStore.getInitialState(), true) });

// ============================================================================
// Selectors (defined outside store for memoization)
// ============================================================================

/**
 * Select the currently selected bucket
 * @returns The selected bucket, or null if none
 */
export const selectSelectedBucket = (
  state: ArtifactState & ArtifactActions
): ArtifactBucket | null =>
  state.selectedBucketId ? state.buckets[state.selectedBucketId] ?? null : null;

/**
 * Select the currently selected artifact
 * @returns The selected artifact, or null if none
 */
export const selectSelectedArtifact = (
  state: ArtifactState & ArtifactActions
): Artifact | null =>
  state.selectedArtifactId ? state.artifacts[state.selectedArtifactId] ?? null : null;

/**
 * Select artifacts belonging to a specific bucket
 * @param bucketId - The bucket ID to filter by
 * @returns Selector function returning array of artifacts in the bucket
 */
export const selectArtifactsByBucket =
  (bucketId: string) =>
  (state: ArtifactState): Artifact[] =>
    Object.values(state.artifacts).filter((a) => a.bucketId === bucketId);

/**
 * Select artifacts of a specific type
 * @param type - The artifact type to filter by
 * @returns Selector function returning array of artifacts of that type
 */
export const selectArtifactsByType =
  (type: ArtifactType) =>
  (state: ArtifactState): Artifact[] =>
    Object.values(state.artifacts).filter((a) => a.type === type);

/**
 * Select an artifact by ID
 * @param artifactId - The artifact ID to find
 * @returns Selector function returning the artifact or undefined
 */
export const selectArtifactById =
  (artifactId: string) =>
  (state: ArtifactState): Artifact | undefined =>
    state.artifacts[artifactId];
