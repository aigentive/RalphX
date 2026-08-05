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
import type { AttemptFailure, EnvironmentDescriptorView } from "@/lib/remote/supervisor";
import type { SupervisorPresentation } from "@/lib/remote/supervisor-transition-table";

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

/**
 * What the UI renders about a connection, as opposed to what the FSM is doing.
 *
 * Kept BESIDE `connectionStates` rather than replacing it. The raw state is the
 * vocabulary the switcher dot and the mobile client consume (§6.5) and must not shift
 * meaning; the presentation projection folds `idle`/`connecting`/`backoff` into a
 * first-run vs. reconnect distinction the FSM deliberately does not make, and carries
 * the blocked cause the actionable UI needs (P-10).
 *
 * `blockedFailure`/`blockedMessage` are populated ONLY while blocked; every other
 * presentation clears them, so no surface can render a stale re-pair prompt over a
 * healthy connection.
 */
export interface EnvironmentConnectionPresentation {
  readonly presentation: SupervisorPresentation;
  readonly blockedFailure: AttemptFailure | null;
  readonly blockedMessage: string | null;
}

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
   * SINGLE WRITER: the composition root's `onStateChange` wiring in
   * `lib/remote/environment-runtime.ts`. Banner, switcher dot, and the read-only
   * selector are READERS; no component subscribes to a supervisor directly.
   *
   * A missing entry means "no supervisor has reported yet", which every reader must
   * treat as NOT connected (fail closed) rather than as an absence of trouble.
   */
  connectionPresentations: Record<string, EnvironmentConnectionPresentation>;
  /**
   * The last CONFIRMED effective scopes per environment (P-28 client consumption).
   *
   * `null` — including a missing key — means "never confirmed", NOT "no scopes":
   * capability gates read the absence as lacking every scope and stay closed. That
   * is the whole point of holding it here rather than defaulting to the pairing-time
   * `remote.scopes` snapshot, which records what was granted when the environment was
   * added and can be arbitrarily stale relative to what the host will actually serve.
   *
   * SINGLE WRITER: the composition root's supervisor wiring in
   * `lib/remote/environment-runtime.ts` (`applyScopes`, plus disposal on quiesce /
   * registry removal). Components are readers. The supervisor only ever calls
   * `applyScopes` with a set it confirmed via `GET /remote/v1/session`, and a failed
   * refresh leaves the previous value untouched — gating never widens past the
   * last-confirmed set (Fixed Decision 2).
   */
  effectiveScopes: Record<string, readonly string[] | null>;
  /** Live descriptor facts only. Persisting these would lie as soon as the host upgrades. */
  hostDescriptors: Record<string, Pick<EnvironmentDescriptorView, "appVersion" | "platform">>;
  /**
   * Per-environment notification tally OBSERVED while an environment is in the
   * background (PR 3.3, §6.4).
   *
   * This is a count of `notification:created` frames seen on a background socket —
   * NOT a projection. Nothing here advances a warm cursor and nothing here touches an
   * environment's retained `QueryClient` (A-12). A missing key means "nothing observed
   * since this environment was last active", which is what a reader must render as no
   * badge.
   *
   * SINGLE WRITER: the composition root's background relay in
   * `lib/remote/environment-runtime.ts`. The switcher is a READER. A second writer
   * (say, a component reacting to its own notification query) would race the observer
   * and produce a badge that disagrees with itself between renders.
   *
   * The active environment never accumulates: it is projecting, so its notifications
   * are already in its cache and the badge would double-count them.
   */
  notificationBadges: Record<string, number>;
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
  /** Composition-root only: adopts the supervisor's presentation + blocked cause. */
  setConnectionPresentation: (
    id: string,
    presentation: EnvironmentConnectionPresentation
  ) => void;
  /** Composition-root only: adopts a scope set the supervisor has CONFIRMED. */
  setEffectiveScopes: (id: string, scopes: readonly string[]) => void;
  /** Composition-root only: refreshes live host facts on every connect attempt. */
  setHostDescriptor: (id: string, descriptor: EnvironmentDescriptorView) => void;
  /** Composition-root only: forgets a scope set (quiesce / registry removal). */
  clearEffectiveScopes: (id: string) => void;
  /** Composition-root only: forgets a presentation (quiesce / registry removal). */
  clearConnectionPresentation: (id: string) => void;
  /** Composition-root only: counts one observed background notification (A-12). */
  observeNotificationBadge: (id: string) => void;
  /** Composition-root only: reconciles the tally away on activation / removal. */
  clearNotificationBadge: (id: string) => void;
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
  connectionPresentations: {},
  effectiveScopes: {},
  hostDescriptors: {},
  notificationBadges: {},

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
      // A removed environment's confirmed scopes must not survive it: a re-added row
      // can reuse an id, and inheriting the old grant would authorize agent control
      // that was never re-confirmed.
      const effectiveScopes: Record<string, readonly string[] | null> = {};
      for (const [id, scopes] of Object.entries(state.effectiveScopes)) {
        if (knownIds.has(id) && id !== LOCAL_ENVIRONMENT_ID) {
          effectiveScopes[id] = scopes;
        }
      }
      const hostDescriptors: EnvironmentState["hostDescriptors"] = {};
      for (const [id, descriptor] of Object.entries(state.hostDescriptors)) {
        if (knownIds.has(id) && id !== LOCAL_ENVIRONMENT_ID) {
          hostDescriptors[id] = descriptor;
        }
      }
      // Same reasoning as the scopes above: a re-added row can reuse an id, and
      // inheriting the old presentation would paint a removed environment's last
      // "connected" over a connection that has never been attempted.
      const connectionPresentations: Record<
        string,
        EnvironmentConnectionPresentation
      > = {};
      for (const [id, presentation] of Object.entries(
        state.connectionPresentations
      )) {
        if (knownIds.has(id) && id !== LOCAL_ENVIRONMENT_ID) {
          connectionPresentations[id] = presentation;
        }
      }
      // Same reasoning again: an observed tally belongs to the connection that
      // produced it. A re-added row reusing an id must start at zero rather than
      // claim notifications from an environment the user removed.
      const notificationBadges: Record<string, number> = {};
      for (const [id, count] of Object.entries(state.notificationBadges)) {
        if (knownIds.has(id) && id !== LOCAL_ENVIRONMENT_ID) {
          notificationBadges[id] = count;
        }
      }
      return {
        environments,
        connectionStates,
        connectionPresentations,
        effectiveScopes,
        hostDescriptors,
        notificationBadges,
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

  setConnectionPresentation: (id, presentation) => {
    if (id === LOCAL_ENVIRONMENT_ID) return; // local has no supervisor
    set((state) => ({
      connectionPresentations: {
        ...state.connectionPresentations,
        [id]: presentation,
      },
    }));
  },

  setEffectiveScopes: (id, scopes) => {
    if (id === LOCAL_ENVIRONMENT_ID) return; // local is never scope-limited
    set((state) => ({
      effectiveScopes: { ...state.effectiveScopes, [id]: [...scopes] },
    }));
  },

  setHostDescriptor: (id, descriptor) => {
    if (id === LOCAL_ENVIRONMENT_ID) return;
    set((state) => ({
      hostDescriptors: {
        ...state.hostDescriptors,
        [id]: { appVersion: descriptor.appVersion, platform: descriptor.platform },
      },
    }));
  },

  clearEffectiveScopes: (id) => {
    set((state) => {
      if (!(id in state.effectiveScopes)) return state;
      const effectiveScopes = { ...state.effectiveScopes };
      delete effectiveScopes[id];
      return { effectiveScopes };
    });
  },

  clearConnectionPresentation: (id) => {
    set((state) => {
      if (state.connectionPresentations[id] === undefined) return state;
      const connectionPresentations = { ...state.connectionPresentations };
      delete connectionPresentations[id];
      return { connectionPresentations };
    });
  },

  observeNotificationBadge: (id) => {
    if (id === LOCAL_ENVIRONMENT_ID) return; // local projects; it never observes
    set((state) => ({
      notificationBadges: {
        ...state.notificationBadges,
        [id]: (state.notificationBadges[id] ?? 0) + 1,
      },
    }));
  },

  clearNotificationBadge: (id) => {
    set((state) => {
      if (state.notificationBadges[id] === undefined) return state;
      const notificationBadges = { ...state.notificationBadges };
      delete notificationBadges[id];
      return { notificationBadges };
    });
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
