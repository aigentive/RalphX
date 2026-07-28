/**
 * Which copy of the feature flags is authoritative under a remote environment
 * (PR 2.6, review-4 scope addition).
 *
 * ## The two copies, and why they disagree
 *
 * 1. `useFeatureFlags()` — a react-query hook calling `get_ui_feature_flags`. That
 *    invoke goes through the aliased transport wrapper and `get_ui_feature_flags` is
 *    NOT in `local-only-commands.ts`, so under a remote environment it is served by
 *    the HOST. The QueryClient is per-environment (`queryClient.ts:57-66`), so each
 *    environment caches its own answer.
 * 2. `useUiStore.getState().featureFlags` — populated ONCE at module load
 *    (`uiStore.ts`, bottom) and never refreshed on an environment switch. At module
 *    load the transport still points at local, so this copy is permanently THIS
 *    Mac's flags.
 *
 * Neither is wrong; they answer different questions. The bug is reading one when you
 * meant the other, and until now nothing said which was which.
 *
 * ## The decision
 *
 * **Flags are split by whose behaviour they describe.**
 *
 * - **Host-behaviour flags** (everything except the client-owned list below) describe
 *   what the BACKEND serving the active environment can do — which pages have data,
 *   which integrations are configured, which agent features exist. Authority is the
 *   `useFeatureFlags()` query, because it is the only copy that follows the active
 *   environment. Reading `uiStore.featureFlags` for one of these under a remote
 *   environment shows the local Mac's capabilities for a host that may not share
 *   them. `uiStore.featureFlags` remains valid as a non-reactive convenience ONLY
 *   where the caller is on a local-only path.
 *
 * - **Client-owned flags** describe what THIS device does, and must never be answered
 *   by a host. Today that is exactly `remoteEnvironments`: it decides whether this
 *   client runs the remote runtime at all. Sourcing it from the active environment
 *   would be circular (a remote host telling the client whether to do remote) and, in
 *   the flag-off state this PR ships behind, a host could switch the client's remote
 *   machinery on. It is therefore read from `uiStore`, whose boot-time fetch is
 *   local by construction.
 *
 * `environment-runtime.ts` already reads `remoteEnvironments` from `uiStore` — this
 * module makes that load-bearing rather than incidental, and `feature-flag-authority.test.ts`
 * fails if a client-owned flag starts being read from the env-scoped query.
 */

import type { FeatureFlags } from "@/types/feature-flags";
import { useUiStore } from "@/stores/uiStore";

/**
 * Flags describing THIS client, never the active environment's host.
 *
 * Adding a row here is a deliberate authority decision: ask "if a malicious or merely
 * differently-configured host answered this flag, what would it change on my Mac?"
 * If the answer is anything, the flag belongs here.
 */
export const CLIENT_OWNED_FEATURE_FLAGS = ["remoteEnvironments"] as const;

export type ClientOwnedFeatureFlag = (typeof CLIENT_OWNED_FEATURE_FLAGS)[number];

const CLIENT_OWNED_SET: ReadonlySet<string> = new Set(CLIENT_OWNED_FEATURE_FLAGS);

export function isClientOwnedFeatureFlag(flag: string): boolean {
  return CLIENT_OWNED_SET.has(flag);
}

/**
 * Reads a client-owned flag from the authoritative (local) copy.
 *
 * Non-reactive by design: these flags are read at runtime-composition time, not in
 * render, and a reactive read would invite a component to re-derive remote behaviour
 * from a value that must not change with the active environment.
 */
export function getClientOwnedFeatureFlag(flag: ClientOwnedFeatureFlag): boolean {
  return useUiStore.getState().featureFlags[flag] === true;
}

/**
 * Narrows an env-scoped flag payload to the host-behaviour flags, dropping any
 * client-owned key a host may have answered.
 *
 * Defence in depth: a host that returns `remoteEnvironments: true` cannot flip this
 * client into remote mode through the query cache, whatever the call site does with
 * the payload.
 */
export function stripClientOwnedFlags(flags: FeatureFlags): FeatureFlags {
  const next = { ...flags } as Record<string, unknown>;
  for (const flag of CLIENT_OWNED_FEATURE_FLAGS) {
    delete next[flag];
  }
  return next as FeatureFlags;
}
