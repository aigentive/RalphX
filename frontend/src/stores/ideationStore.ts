/**
 * Ideation store using Zustand with immer middleware
 *
 * Manages ideation session state for the frontend. Sessions are stored in a
 * Record keyed by session ID for O(1) lookup.
 */

import { create } from "zustand";
import { immer } from "zustand/middleware/immer";

import { registerEnvIsolatedStore } from "@/lib/remote/env-state-isolation";
import type { IdeationSession, IdeationSessionStatus } from "@/types/ideation";
import type { Artifact } from "@/types/artifact";
import type { IdeationSettings } from "@/types/ideation-config";

// ============================================================================
// Constants
// ============================================================================

/** Maximum number of sessions to keep in memory before LRU eviction */
const MAX_CACHED_SESSIONS = 20;

// ============================================================================
// State Interface
// ============================================================================

export interface ProactiveSyncNotification {
  /** Artifact ID that was updated */
  artifactId: string;
  /** Proposal IDs that may need updating */
  proposalIds: string[];
  /** Previous proposal states for undo functionality */
  previousStates: Record<string, unknown>;
  /** Timestamp when notification was created */
  timestamp: number;
}

interface IdeationState {
  /** Sessions indexed by ID for O(1) lookup */
  sessions: Record<string, IdeationSession>;
  /** Currently active session ID, or null if none */
  activeSessionId: string | null;
  /** Plan artifact for the active session, or null if none */
  planArtifact: Artifact | null;
  /** Ideation settings */
  ideationSettings: IdeationSettings | null;
  /** Proactive sync notification for stale proposals */
  syncNotification: ProactiveSyncNotification | null;
  /** Loading state for async operations */
  isLoading: boolean;
  /** Error message, or null if no error */
  error: string | null;
  /** Active tab per session (keyed by sessionId) */
  activeIdeationTab: Record<string, 'plan' | 'verification' | 'proposals' | 'research'>;
  /** Active verification child session ID per parent session (keyed by parent sessionId) */
  activeVerificationChildId: Record<string, string | null>;
  /** Last known verification child session ID per parent session — display-only reference, persists after agent terminates (keyed by parent sessionId) */
  lastVerificationChildId: Record<string, string | null>;
  /** Pending verification notifications: parentSessionId → childSessionId */
  verificationNotifications: Record<string, string>;
}

// ============================================================================
// Actions Interface
// ============================================================================

interface IdeationActions {
  /** Set the active session by ID, or null to deselect */
  setActiveSession: (sessionId: string | null) => void;
  /** Add a single session to the store */
  addSession: (session: IdeationSession) => void;
  /** Select a session atomically (adds to store + sets active in one update) */
  selectSession: (session: IdeationSession) => void;
  /** Replace all sessions with new array (converts to Record) */
  setSessions: (sessions: IdeationSession[]) => void;
  /** Update a specific session with partial changes */
  updateSession: (sessionId: string, changes: Partial<IdeationSession>) => void;
  /** Archive a session in the store (soft-update: sets status to archived) */
  archiveSession: (sessionId: string) => void;
  /** Set the plan artifact for the active session */
  setPlanArtifact: (artifact: Artifact | null) => void;
  /** Fetch the plan artifact for a given artifact ID */
  fetchPlanArtifact: (artifactId: string) => Promise<void>;
  /** Set ideation settings */
  setIdeationSettings: (settings: IdeationSettings) => void;
  /** Show proactive sync notification */
  showSyncNotification: (notification: ProactiveSyncNotification) => void;
  /** Dismiss sync notification */
  dismissSyncNotification: () => void;
  /** Set loading state */
  setLoading: (isLoading: boolean) => void;
  /** Set error message */
  setError: (error: string | null) => void;
  /** Clear error message */
  clearError: () => void;
  /** Set the active tab for a session */
  setActiveIdeationTab: (sessionId: string, tab: 'plan' | 'verification' | 'proposals' | 'research') => void;
  /** Set the active verification child session ID for a parent session */
  setActiveVerificationChildId: (sessionId: string, childId: string | null) => void;
  /** Set the last known verification child session ID for display purposes (persists after agent terminates) */
  setLastVerificationChildId: (sessionId: string, childId: string | null) => void;
  /** Clear all tab-related state for a session (call on session unmount/archive) */
  clearSessionTabState: (sessionId: string) => void;
  /** Set a pending verification notification (parentSessionId → childSessionId) */
  setVerificationNotification: (parentId: string, childId: string) => void;
  /** Clear a pending verification notification (on terminal verification state or dismiss) */
  clearVerificationNotification: (parentId: string) => void;
}

// ============================================================================
// Store Implementation
// ============================================================================

export const useIdeationStore = create<IdeationState & IdeationActions>()(
  immer((set) => ({
    // Initial state
    sessions: {},
    activeSessionId: null,
    planArtifact: null,
    ideationSettings: null,
    syncNotification: null,
    isLoading: false,
    error: null,
    activeIdeationTab: {},
    activeVerificationChildId: {},
    lastVerificationChildId: {},
    verificationNotifications: {},

    // Actions
    setActiveSession: (sessionId) =>
      set((state) => {
        const isSameSession = state.activeSessionId === sessionId;
        state.activeSessionId = sessionId;
        // Clear session-specific state only when switching to a different session
        if (!isSameSession) {
          state.planArtifact = null;
          state.syncNotification = null;
          state.error = null;
        }
      }),

    addSession: (session) =>
      set((state) => {
        state.sessions[session.id] = session;
        // LRU eviction: remove oldest session if over limit
        const sessionIds = Object.keys(state.sessions);
        if (sessionIds.length > MAX_CACHED_SESSIONS) {
          // Find the oldest session (by updatedAt) that's not the active session
          const oldest = sessionIds
            .filter((id) => id !== state.activeSessionId)
            .sort((a, b) => {
              const aTime = new Date(state.sessions[a]?.updatedAt ?? 0).getTime();
              const bTime = new Date(state.sessions[b]?.updatedAt ?? 0).getTime();
              return aTime - bTime;
            })[0];
          if (oldest) {
            delete state.sessions[oldest];
          }
        }
      }),

    selectSession: (session) =>
      set((state) => {
        const isSameSession = state.activeSessionId === session.id;
        // Add session to store
        state.sessions[session.id] = session;

        // Set as active
        state.activeSessionId = session.id;

        // Clear session-specific state only when switching sessions
        if (!isSameSession) {
          state.planArtifact = null;
          state.syncNotification = null;
          state.error = null;
        }

        // LRU eviction if needed
        const sessionIds = Object.keys(state.sessions);
        if (sessionIds.length > MAX_CACHED_SESSIONS) {
          const oldest = sessionIds
            .filter((id) => id !== session.id)
            .sort((a, b) => {
              const aTime = new Date(state.sessions[a]?.updatedAt ?? 0).getTime();
              const bTime = new Date(state.sessions[b]?.updatedAt ?? 0).getTime();
              return aTime - bTime;
            })[0];
          if (oldest) {
            delete state.sessions[oldest];
          }
        }
      }),

    setSessions: (sessions) =>
      set((state) => {
        state.sessions = Object.fromEntries(sessions.map((s) => [s.id, s]));
      }),

    updateSession: (sessionId, changes) =>
      set((state) => {
        const session = state.sessions[sessionId];
        if (session) {
          Object.assign(session, changes);
        }
      }),

    archiveSession: (sessionId) =>
      set((state) => {
        const session = state.sessions[sessionId];
        if (session) {
          session.status = "archived";
          session.archivedAt = new Date().toISOString();
        }
        // Clear active session if archiving active session
        if (state.activeSessionId === sessionId) {
          state.activeSessionId = null;
        }
      }),

    setPlanArtifact: (artifact) =>
      set((state) => {
        state.planArtifact = artifact;
      }),

    fetchPlanArtifact: async (artifactId) => {
      const { artifactApi } = await import("@/api/artifact");
      try {
        set((state) => {
          state.isLoading = true;
          state.error = null;
        });
        const artifact = await artifactApi.get(artifactId);
        set((state) => {
          state.planArtifact = artifact;
          state.isLoading = false;
        });
      } catch (error) {
        set((state) => {
          state.error =
            error instanceof Error ? error.message : "Failed to fetch plan artifact";
          state.isLoading = false;
        });
      }
    },

    setIdeationSettings: (settings) =>
      set((state) => {
        state.ideationSettings = settings;
      }),

    showSyncNotification: (notification) =>
      set((state) => {
        state.syncNotification = notification;
      }),

    dismissSyncNotification: () =>
      set((state) => {
        state.syncNotification = null;
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

    setActiveIdeationTab: (sessionId, tab) =>
      set((state) => {
        state.activeIdeationTab[sessionId] = tab;
        // LRU eviction: cap at MAX_CACHED_SESSIONS entries
        const keys = Object.keys(state.activeIdeationTab);
        if (keys.length > MAX_CACHED_SESSIONS) {
          // Remove the oldest entry (first key, since we can't track access time easily)
          const firstKey = keys[0];
          if (firstKey && firstKey !== sessionId) {
            delete state.activeIdeationTab[firstKey];
            delete state.activeVerificationChildId[firstKey];
            delete state.lastVerificationChildId[firstKey];
          }
        }
      }),

    setActiveVerificationChildId: (sessionId, childId) =>
      set((state) => {
        state.activeVerificationChildId[sessionId] = childId;
      }),

    setLastVerificationChildId: (sessionId, childId) =>
      set((state) => {
        state.lastVerificationChildId[sessionId] = childId;
      }),

    clearSessionTabState: (sessionId) =>
      set((state) => {
        delete state.activeIdeationTab[sessionId];
        delete state.activeVerificationChildId[sessionId];
        delete state.lastVerificationChildId[sessionId];
        delete state.verificationNotifications[sessionId];
      }),

    setVerificationNotification: (parentId, childId) =>
      set((state) => {
        state.verificationNotifications[parentId] = childId;
      }),

    clearVerificationNotification: (parentId) =>
      set((state) => {
        delete state.verificationNotifications[parentId];
      }),
  }))
);

registerEnvIsolatedStore({ name: "useIdeationStore", reset: () => useIdeationStore.setState(useIdeationStore.getInitialState(), true) });

if (typeof window !== "undefined" && !window.__TAURI_INTERNALS__) {
  window.__ideationStore = useIdeationStore;
}

// ============================================================================
// Selectors (defined outside store for memoization)
// ============================================================================

/**
 * Select the currently active session
 * @returns The active session, or null if none active
 */
export const selectActiveSession = (
  state: IdeationState & IdeationActions
): IdeationSession | null =>
  state.activeSessionId ? state.sessions[state.activeSessionId] ?? null : null;

/**
 * Select all sessions for a specific project
 * @param projectId - The project ID to filter by
 * @returns Selector function returning matching sessions
 */
export const selectSessionsByProject =
  (projectId: string) =>
  (state: IdeationState): IdeationSession[] =>
    Object.values(state.sessions).filter((s) => s.projectId === projectId);

/**
 * Select all sessions with a specific status
 * @param status - The status to filter by
 * @returns Selector function returning matching sessions
 */
export const selectSessionsByStatus =
  (status: IdeationSessionStatus) =>
  (state: IdeationState): IdeationSession[] =>
    Object.values(state.sessions).filter((s) => s.status === status);

/**
 * Select the last known verification child session ID for a given parent session.
 * Display-only reference — persists after agent terminates. Never used for guard/watchdog decisions.
 * @param sessionId - The parent session ID
 * @returns Selector function returning the last verification child ID, or null
 */
export const selectLastVerificationChildId =
  (sessionId: string) =>
  (state: IdeationState & IdeationActions): string | null =>
    state.lastVerificationChildId[sessionId] ?? null;

/**
 * Select the effective chat session ID for a given parent session.
 * Returns the verification child ID when verification tab is active, otherwise returns the parent session ID.
 * @param sessionId - The parent session ID
 * @returns Selector function returning the effective session ID for chat
 * @deprecated unused. Use lastVerificationChildId directly for verification chat routing.
 */
export const selectChatSessionId =
  (sessionId: string) =>
  (state: IdeationState & IdeationActions): string => {
    const tab = state.activeIdeationTab[sessionId] ?? 'plan';
    if (tab === 'verification') {
      return state.activeVerificationChildId[sessionId] ?? sessionId;
    }
    return sessionId;
  };
