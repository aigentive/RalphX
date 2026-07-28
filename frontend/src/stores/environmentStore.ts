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

import { onEnvironmentSwitched } from "@/lib/remote/env-state-isolation";
import {
  remoteEnvironmentsApi,
  type RemoteEnvironmentSummary,
} from "@/api/remote-environments";
import {
  LOCAL_ENVIRONMENT_ID,
  setTransportEnvironmentId,
} from "@/lib/remote/active-environment";

export { LOCAL_ENVIRONMENT_ID };

/**
 * Canonical supervisor FSM vocabulary (§6.5); "connected" is all local ever is.
 *
 * Plus ONE presentation-only value the FSM never produces: `health_only`. A
 * background environment completes descriptor + socket + hello + probe but never
 * sends `subscribe` and never projects (full background projection is a v1
 * non-goal), so rendering it as `connected` would assert a stream liveness that
 * does not exist — "never a probe alone" (§6.5, P-25). The runtime projects a
 * non-active environment's `connected` to this instead. It is deliberately NOT in
 * `SUPERVISOR_STATES`: the FSM vocabulary the mobile client consumes is unchanged.
 */
export type EnvironmentConnectionState =
  | "idle"
  | "connecting"
  | "connected"
  | "backoff"
  | "offline"
  | "blocked"
  | "suspended"
  | "health_only";

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
    const isKnown = () => get().environments.some((entry) => entry.id === id);

    // An id that is not in the list may simply mean the registry has not loaded yet.
    // Clamping on that would silently diverge from the Rust authority, which keeps
    // authorizing the environment the UI stopped showing.
    if (!isKnown()) {
      await get().loadEnvironments();
    }
    if (isKnown()) {
      set({ activeEnvironmentId: id });
      return;
    }

    // A genuine clamp: tell Rust too, so mirror and authority agree.
    set({ activeEnvironmentId: LOCAL_ENVIRONMENT_ID });
    if (id !== LOCAL_ENVIRONMENT_ID) {
      await remoteEnvironmentsApi.setActiveEnvironment(LOCAL_ENVIRONMENT_ID);
    }
  },

  setConnectionState: (id, connection) => {
    if (id === LOCAL_ENVIRONMENT_ID) return; // local has no supervisor
    set((state) => ({
      connectionStates: { ...state.connectionStates, [id]: connection },
    }));
  },
}));

/**
 * Single writer of the transport mirror and environment-state isolation funnel.
 *
 * Subscribing here rather than calling `setTransportEnvironmentId` beside each
 * `set({ activeEnvironmentId })` means a future assignment cannot forget to mirror —
 * every path through this store, including the optimistic switch, its revert, the
 * removed-environment clamp, and startup hydration, funnels through one listener.
 *
 * Ordering note: the optimistic switch mirrors BEFORE the Rust round-trip completes,
 * so an invoke issued in that window targets the new environment while Rust still
 * points at the old one. Rust refuses it with `REMOTE_FORBIDDEN` (P-26) — a
 * fail-closed rejection, never a call served by the wrong environment.
 */
useEnvironmentStore.subscribe((state, previous) => {
  if (state.activeEnvironmentId !== previous.activeEnvironmentId) {
    setTransportEnvironmentId(state.activeEnvironmentId);
    onEnvironmentSwitched();
  }
});
