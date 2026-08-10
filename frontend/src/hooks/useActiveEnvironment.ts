/**
 * The ONE component-level view of which environment is active (PR 2.6, A1).
 *
 * Every capability gate in the app keys on these selectors. They are synchronous
 * zustand reads — a gate must never make a click path wait on a fetch (rule 24) —
 * and they are the only sanctioned component-level answer to "is this a remote
 * environment?". Components must NOT import `isRemoteEnvironmentId` from
 * `@/lib/remote/active-environment`: that helper answers the same question for the
 * TRANSPORT, whose notion of "active" is mirrored asynchronously from this store and
 * is deliberately allowed to lead it during a switch.
 *
 * Fail-closed resolution: an `activeEnvironmentId` that is not in `environments`
 * resolves to kind `"remote"`, not `"local"`. That state is reachable while startup
 * hydration adopts the Rust-authoritative id before the registry list arrives
 * (`environmentStore.hydrateActiveEnvironment`). Guessing "local" there would hand a
 * genuinely remote session the full set of host-only affordances for the length of
 * that window; guessing "remote" only hides a button that reappears a tick later.
 */

import { useEnvironmentStore } from "@/stores/environmentStore";
import type { EnvironmentEntry } from "@/stores/environmentStore";

/**
 * The active environment's registry entry, or `null` when the active id is not (yet)
 * resolvable. `null` is NOT "local" — see `useActiveEnvironmentKind`.
 */
export function useActiveEnvironment(): EnvironmentEntry | null {
  return useEnvironmentStore(
    (state) =>
      state.environments.find(
        (entry) => entry.id === state.activeEnvironmentId,
      ) ?? null,
  );
}

/** The active environment's kind; unresolvable ids read as `"remote"` (fail closed). */
export function useActiveEnvironmentKind(): EnvironmentEntry["kind"] {
  return useEnvironmentStore((state) => {
    const entry = state.environments.find(
      (candidate) => candidate.id === state.activeEnvironmentId,
    );
    return entry?.kind ?? "remote";
  });
}

/**
 * `true` when this client is driving a remote host. The gate for every host-impossible
 * affordance (2.6-a); permission-shaped gates additionally consult `useAgentGate`.
 */
export function useIsRemoteEnvironment(): boolean {
  return useEnvironmentStore((state) => {
    const entry = state.environments.find(
      (candidate) => candidate.id === state.activeEnvironmentId,
    );
    return (entry?.kind ?? "remote") !== "local";
  });
}

/** Non-reactive read for callbacks and module-level code paths. */
export function isRemoteEnvironmentActive(): boolean {
  const state = useEnvironmentStore.getState();
  const entry = state.environments.find(
    (candidate) => candidate.id === state.activeEnvironmentId,
  );
  return (entry?.kind ?? "remote") !== "local";
}
