/**
 * The presentation projection (§6.5, §6.7) — split from the transition table so the
 * FSM rows stay a pure checked-in artifact while presentation grows the `syncing`
 * distinction.
 *
 * ## Why `syncing` exists
 *
 * `presentationFor(state, hasEverConnected)` used to collapse "the host is unreachable
 * and the ladder is armed" and "the socket is open and we are mid-hydration" into one
 * amber `reconnecting`. The second is routine — every foreground, stream reset, and
 * cache sweep runs it — and presenting it as an outage made the app look broken many
 * times a day. `syncing` names the healthy half: transport provably fine (or a fresh
 * optimistic redial right after a healthy period), data being read.
 *
 * ## The anti-lying rule
 *
 * `syncing` is calm chrome, so it must never mask a real failure. Two independent
 * ceilings escalate it back to `reconnecting`, one-way per disconnect episode:
 *
 * - K: [`MAX_SYNCING_BARRIER_FAILURES`] consecutive failed hydration barriers. One is
 *   a normal race; two in a row is the pathology the connection journal exists for.
 * - T: [`SYNCING_GRACE_MS`] from the start of the episode, deliberately under the 15 s
 *   connect budget so the amber banner appears before the budget kills the attempt —
 *   never as a side effect of a redial.
 */

import type { SupervisorState } from "./supervisor-transition-table";

/** The presentation states the UI renders. `syncing` is chip-only chrome (no banner). */
export type SupervisorPresentation =
  | "connecting"
  | "reconnecting"
  | "syncing"
  | "connected"
  | "offline"
  | "error"
  | "suspended";

/** T ceiling: past this many ms into a disconnect episode, syncing escalates. */
export const SYNCING_GRACE_MS = 12_000;

/** K ceiling: this many consecutive barrier failures escalate the episode. */
export const MAX_SYNCING_BARRIER_FAILURES = 2;

/**
 * The supervisor-owned facts `presentationFor` needs to distinguish a healthy
 * mid-hydration attempt from a degraded transport. All of them reset per episode.
 */
export interface SyncingHint {
  /** The socket completed `hello` during this episode's current or a prior attempt. */
  readonly streamOpen: boolean;
  /** The retry-ladder attempt counter at evaluation time. */
  readonly attempts: number;
  /** Consecutive failed hydration barriers within this episode. */
  readonly barrierFailures: number;
  /** ms since the disconnect episode began; `null` when no episode is being tracked. */
  readonly episodeElapsedMs: number | null;
  /** The silence watchdog or an undecodable frame said the host is gone, not slow. */
  readonly deadHostSuspected: boolean;
}

/**
 * The inert hint: `episodeElapsedMs: null` fails the grace condition, so callers
 * without episode tracking (and the pinned projection tests) keep the pre-`syncing`
 * mapping verbatim.
 */
export const NO_SYNCING_HINT: SyncingHint = {
  streamOpen: false,
  attempts: 0,
  barrierFailures: 0,
  episodeElapsedMs: null,
  deadHostSuspected: false,
};

/**
 * `connecting` presents as `reconnecting` once the environment has ever been connected,
 * so P-9's "lands in reconnecting, never connected" holds across the whole
 * backoff→connecting→backoff cycle rather than flickering back to first-run copy.
 *
 * Within that band, `syncing` is produced iff ALL hold:
 * 1. the state is in the disconnect band (`idle`/`connecting`/`backoff`),
 * 2. this environment has connected before (first-ever connect has no cached data —
 *    "Syncing" over an empty board would be a lie),
 * 3. the host is not suspected dead,
 * 4. fewer than K consecutive barrier failures,
 * 5. the episode is inside the T grace,
 * 6. the socket opened this episode, or the ladder has burned at most one attempt
 *    (`attempts <= 1` covers the first rung after a routine `socket_lost` and the
 *    ~200 ms redial between rungs, so a healthy reconnect never flashes amber
 *    mid-cycle; a second failed attempt is real trouble and escalates).
 */
export function presentationFor(
  state: SupervisorState,
  hasEverConnected: boolean,
  hint: SyncingHint = NO_SYNCING_HINT
): SupervisorPresentation {
  switch (state) {
    case "connected":
      return "connected";
    case "offline":
      return "offline";
    case "blocked":
      return "error";
    case "suspended":
      return "suspended";
    case "backoff":
    case "idle":
    case "connecting": {
      if (
        hasEverConnected &&
        !hint.deadHostSuspected &&
        hint.barrierFailures < MAX_SYNCING_BARRIER_FAILURES &&
        hint.episodeElapsedMs !== null &&
        hint.episodeElapsedMs <= SYNCING_GRACE_MS &&
        (hint.streamOpen || hint.attempts <= 1)
      ) {
        return "syncing";
      }
      if (state === "backoff") {
        return "reconnecting";
      }
      return hasEverConnected ? "reconnecting" : "connecting";
    }
  }
}
