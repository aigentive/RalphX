/**
 * Environment store — multi-environment identity slice (PR 2.1, §6.4).
 *
 * `{ activeEnvironmentId, environments, connectionStates }` with `"local"` always
 * present. Local has NO supervisor and no connection lifecycle — its connection
 * state is pinned to "connected".
 *
 * Authority model: this store is the UI mirror; the Rust backend holds the
 * authoritative active-environment id that the proxy commands enforce (P-26).
 * `setActiveEnvironment` paints synchronously FIRST (rule 24: first paint wins),
 * then mirrors the switch to Rust; if Rust refuses the switch the store reverts,
 * so the UI can never sit on an environment the proxy will not serve.
 *
 * Supervisors, per-environment QueryClients, and connection lifecycles land in
 * PR 2.2+; `connectionStates` already carries the canonical FSM vocabulary so
 * those PRs extend this store instead of introducing a second owner.
 */

import { create } from "zustand";
import {
  remoteEnvironmentsApi,
  type RemoteEnvironmentSummary,
} from "@/api/remote-environments";

export const LOCAL_ENVIRONMENT_ID = "local";

/** Canonical supervisor FSM vocabulary (§6.5); "connected" is all local ever is. */
export type EnvironmentConnectionState =
  | "idle"
  | "connecting"
  | "connected"
  | "backoff"
  | "offline"
  | "blocked"
  | "suspended";

export interface EnvironmentEntry {
  id: string;
  name: string;
  kind: "local" | "remote";
  /** Registry summary for remote entries; absent for local. */
  remote?: RemoteEnvironmentSummary;
}

const LOCAL_ENTRY: EnvironmentEntry = {
  id: LOCAL_ENVIRONMENT_ID,
  name: "This Mac",
  kind: "local",
};

interface EnvironmentState {
  activeEnvironmentId: string;
  environments: EnvironmentEntry[];
  connectionStates: Record<string, EnvironmentConnectionState>;
  /**
   * Switches the active environment. Synchronous state update first (first
   * paint), then mirrors to the Rust authority; reverts on rejection.
   */
  setActiveEnvironment: (id: string) => Promise<void>;
  /** Replaces the remote entries from registry summaries; local always stays. */
  setEnvironments: (summaries: RemoteEnvironmentSummary[]) => void;
  /** Loads the registry from the backend. */
  loadEnvironments: () => Promise<void>;
  /** Adopts the Rust-side authoritative active id (startup hydration). */
  hydrateActiveEnvironment: () => Promise<void>;
  setConnectionState: (id: string, state: EnvironmentConnectionState) => void;
}

function toEntry(summary: RemoteEnvironmentSummary): EnvironmentEntry {
  return {
    id: summary.id,
    name: summary.name,
    kind: "remote",
    remote: summary,
  };
}

export const useEnvironmentStore = create<EnvironmentState>((set, get) => ({
  activeEnvironmentId: LOCAL_ENVIRONMENT_ID,
  environments: [LOCAL_ENTRY],
  connectionStates: { [LOCAL_ENVIRONMENT_ID]: "connected" },

  setActiveEnvironment: async (id) => {
    const previous = get().activeEnvironmentId;
    if (previous === id) return;
    const known = get().environments.some((entry) => entry.id === id);
    if (!known) return;

    // First paint wins: the switch is visible before any backend round-trip.
    set({ activeEnvironmentId: id });
    try {
      await remoteEnvironmentsApi.setActiveEnvironment(id);
    } catch (error) {
      // Rust is authoritative — a refused switch must not leave the UI on an
      // environment the proxy will reject.
      if (get().activeEnvironmentId === id) {
        set({ activeEnvironmentId: previous });
      }
      throw error;
    }
  },

  setEnvironments: (summaries) => {
    set((state) => {
      const environments = [LOCAL_ENTRY, ...summaries.map(toEntry)];
      const knownIds = new Set(environments.map((entry) => entry.id));
      const connectionStates: Record<string, EnvironmentConnectionState> = {
        [LOCAL_ENVIRONMENT_ID]: "connected",
      };
      for (const [id, connection] of Object.entries(state.connectionStates)) {
        if (knownIds.has(id) && id !== LOCAL_ENVIRONMENT_ID) {
          connectionStates[id] = connection;
        }
      }
      return {
        environments,
        connectionStates,
        // A removed environment cannot stay active; Rust already fell back to
        // local when the row died, so the mirror follows.
        activeEnvironmentId: knownIds.has(state.activeEnvironmentId)
          ? state.activeEnvironmentId
          : LOCAL_ENVIRONMENT_ID,
      };
    });
  },

  loadEnvironments: async () => {
    const summaries = await remoteEnvironmentsApi.list();
    get().setEnvironments(summaries);
  },

  hydrateActiveEnvironment: async () => {
    const id = await remoteEnvironmentsApi.getActiveEnvironment();
    set((state) => ({
      activeEnvironmentId: state.environments.some((entry) => entry.id === id)
        ? id
        : LOCAL_ENVIRONMENT_ID,
    }));
  },

  setConnectionState: (id, connection) => {
    if (id === LOCAL_ENVIRONMENT_ID) return; // local has no supervisor
    set((state) => ({
      connectionStates: { ...state.connectionStates, [id]: connection },
    }));
  },
}));
