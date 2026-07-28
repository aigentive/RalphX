/**
 * The connection supervisor's transition table (§6.5) — a CHECKED-IN ARTIFACT.
 *
 * This is not documentation that shadows an implementation. `supervisor.ts` has no
 * `switch` over states: it looks every transition up in `SUPERVISOR_TRANSITION_TABLE`
 * and runs the effects the row names. The FSM tests iterate the same table. So "every
 * §6.5-named event against every state is defined" is falsifiable against this file
 * rather than against prose, and a row nobody implemented is a test failure, not a
 * silent gap.
 *
 * Totality is enforced by the type: `Record<SupervisorState, Record<SupervisorEvent, …>>`
 * does not compile with a missing state or a missing event.
 *
 * ## Reading a row
 *
 * `next` is the state after the event. `effects` is the ORDERED list of side effects the
 * controller performs on that edge. Effects are named rather than inlined so a test can
 * assert "entering `suspended` performs no `scheduleBackoff`" (P-25) without reaching
 * into timer internals.
 *
 * ## Invariants the table encodes (each one is separately tested)
 *
 * - `blocked` and `suspended` and `offline` NEVER carry `scheduleBackoff`: they park
 *   without burning a retry attempt (P-10, P-25, §6.5 offline).
 * - Nothing reaches `connected` except `connect_succeeded`, which the controller only
 *   raises after WS open + `hello` + replay complete + scope refresh + probe. There is
 *   no edge that promotes a bare probe to `connected` (§6.5, P-25).
 * - `resume` from `suspended` goes to `connecting` with `beginAttempt` — the FULL
 *   attempt, which reopens the socket before the probe. It deliberately does NOT
 *   `resetAttempts`: parking burned nothing, and resetting would let a
 *   background/foreground cycle defeat the retry ladder (churn safety).
 * - `resume` from `connected` stays `connected` and only `probe`s. The socket-is-gone
 *   case is not a separate row — the controller raises `socket_lost` first, which lands
 *   in `backoff`, and then `retry_now` drives the full reopen-then-probe sequence.
 * - All three `blocked` entries (401/403, version mismatch, malformed descriptor) come
 *   from `connecting` only, and every `blocked` row's effects are timer-free.
 * - `stable_period_elapsed` is the ONLY producer of `resetAttempts` on the happy path:
 *   the ladder resets after 30 s connected, not on the connect itself (§6.5).
 */

/** Canonical FSM states (§6.5, CROSS-SPEC — the mobile client consumes this exact set). */
export const SUPERVISOR_STATES = [
  "idle",
  "connecting",
  "connected",
  "backoff",
  "offline",
  "blocked",
  "suspended",
] as const;

export type SupervisorState = (typeof SUPERVISOR_STATES)[number];

/** Every §6.5-named event. The table is total over this set. */
export const SUPERVISOR_EVENTS = [
  /** Supervisor started (app start) or an explicit connect. */
  "start",
  /** WS open + hello + replay + scope refresh + probe all succeeded. */
  "connect_succeeded",
  /** The attempt failed for a retryable reason (unreachable, socket refused, probe failed). */
  "connect_failed_transient",
  /** The 15 s connect budget elapsed with the attempt unfinished. */
  "connect_budget_expired",
  /** 401/403 anywhere in the attempt — revoked or bad token. */
  "connect_failed_unauthorized",
  /** `protocolVersion < clientMinProtocol` or `minClientProtocol > PROTOCOL_VERSION`. */
  "connect_failed_version",
  /** The descriptor did not parse, or its environmentId disagreed with the paired row. */
  "connect_failed_malformed_descriptor",
  /** The retry-ladder timer fired. */
  "backoff_elapsed",
  /** The socket died while connected (close, error, or the heartbeat watchdog). */
  "socket_lost",
  /** `navigator.onLine` went false. */
  "offline",
  /** `navigator.onLine` went true — also a `blocked` wakeup. */
  "online",
  /** The app went to the background (`document.hidden`). */
  "suspend",
  /** The app came to the foreground — also a `blocked` wakeup. */
  "resume",
  /** Credentials changed — a `blocked` wakeup. */
  "credentials_changed",
  /** The user explicitly asked to retry — a `blocked` wakeup. */
  "retry_now",
  /** 30 s connected without interruption: the retry ladder resets. */
  "stable_period_elapsed",
  /** The supervisor was shut down (environment removed, app teardown). */
  "stop",
] as const;

export type SupervisorEvent = (typeof SUPERVISOR_EVENTS)[number];

/**
 * Named side effects. The controller owns HOW; the table owns WHETHER and IN WHAT ORDER.
 */
export const SUPERVISOR_EFFECTS = [
  /** Abandon the in-flight attempt and clear its 15 s budget timer. */
  "cancelAttempt",
  /** Ask the Rust proxy to drop this environment's socket. */
  "releaseSocket",
  /** Clear the retry, budget, and stable timers. Never schedules anything. */
  "clearTimers",
  /** Zero the retry-ladder attempt counter. */
  "resetAttempts",
  /** Run one one-shot connect attempt and arm the 15 s budget. */
  "beginAttempt",
  /** Increment the attempt counter and arm the ladder timer (1,2,4,8,16 s, clamped at 16). */
  "scheduleBackoff",
  /** Arm the 30 s stable-period timer. */
  "armStableTimer",
  /** Foreground health probe against an already-live socket. */
  "probe",
] as const;

export type SupervisorEffect = (typeof SUPERVISOR_EFFECTS)[number];

export interface SupervisorTransition {
  readonly next: SupervisorState;
  readonly effects: readonly SupervisorEffect[];
}

/** A flattened row, for table-driven tests and for diffing the table in review. */
export interface SupervisorTransitionRow extends SupervisorTransition {
  readonly from: SupervisorState;
  readonly event: SupervisorEvent;
}

const NONE: readonly SupervisorEffect[] = [];

/** Every event that is inert in a given state maps to "stay put, do nothing". */
function stay(state: SupervisorState): SupervisorTransition {
  return { next: state, effects: NONE };
}

/**
 * The three `blocked` entries share one edge shape: cancel, release, and arm NOTHING.
 * P-10 asserts zero scheduled timers after any of them.
 */
const ENTER_BLOCKED: SupervisorTransition = {
  next: "blocked",
  effects: ["cancelAttempt", "releaseSocket", "clearTimers"],
};

/** A `blocked` wakeup: user- or environment-driven, never a timer. */
const WAKE_FROM_BLOCKED: SupervisorTransition = {
  next: "connecting",
  effects: ["resetAttempts", "beginAttempt"],
};

export const SUPERVISOR_TRANSITION_TABLE: Readonly<
  Record<SupervisorState, Readonly<Record<SupervisorEvent, SupervisorTransition>>>
> = {
  // ---------------------------------------------------------------------------
  // idle — created but not managing a connection (pre-start, or stopped).
  // Inert on purpose: only an explicit start/retry may begin burning attempts.
  // ---------------------------------------------------------------------------
  idle: {
    start: { next: "connecting", effects: ["resetAttempts", "beginAttempt"] },
    connect_succeeded: stay("idle"),
    connect_failed_transient: stay("idle"),
    connect_budget_expired: stay("idle"),
    connect_failed_unauthorized: stay("idle"),
    connect_failed_version: stay("idle"),
    connect_failed_malformed_descriptor: stay("idle"),
    backoff_elapsed: stay("idle"),
    socket_lost: stay("idle"),
    offline: stay("idle"),
    online: stay("idle"),
    suspend: stay("idle"),
    resume: stay("idle"),
    credentials_changed: stay("idle"),
    retry_now: { next: "connecting", effects: ["resetAttempts", "beginAttempt"] },
    stable_period_elapsed: stay("idle"),
    stop: stay("idle"),
  },

  // ---------------------------------------------------------------------------
  // connecting — exactly one one-shot attempt in flight, 15 s budget armed.
  // ---------------------------------------------------------------------------
  connecting: {
    start: stay("connecting"),
    connect_succeeded: { next: "connected", effects: ["armStableTimer"] },
    connect_failed_transient: { next: "backoff", effects: ["scheduleBackoff"] },
    connect_budget_expired: {
      next: "backoff",
      effects: ["cancelAttempt", "releaseSocket", "scheduleBackoff"],
    },
    connect_failed_unauthorized: ENTER_BLOCKED,
    connect_failed_version: ENTER_BLOCKED,
    connect_failed_malformed_descriptor: ENTER_BLOCKED,
    backoff_elapsed: stay("connecting"),
    socket_lost: {
      next: "backoff",
      effects: ["cancelAttempt", "releaseSocket", "scheduleBackoff"],
    },
    offline: {
      next: "offline",
      effects: ["cancelAttempt", "releaseSocket", "clearTimers"],
    },
    online: stay("connecting"),
    suspend: {
      next: "suspended",
      effects: ["cancelAttempt", "releaseSocket", "clearTimers"],
    },
    resume: stay("connecting"),
    credentials_changed: {
      next: "connecting",
      effects: ["cancelAttempt", "releaseSocket", "resetAttempts", "beginAttempt"],
    },
    retry_now: {
      next: "connecting",
      effects: ["cancelAttempt", "releaseSocket", "resetAttempts", "beginAttempt"],
    },
    stable_period_elapsed: stay("connecting"),
    stop: { next: "idle", effects: ["cancelAttempt", "releaseSocket", "clearTimers"] },
  },

  // ---------------------------------------------------------------------------
  // connected — WS open, replay done, probe passed.
  // ---------------------------------------------------------------------------
  connected: {
    start: stay("connected"),
    connect_succeeded: stay("connected"),
    connect_failed_transient: stay("connected"),
    connect_budget_expired: stay("connected"),
    connect_failed_unauthorized: ENTER_BLOCKED,
    connect_failed_version: ENTER_BLOCKED,
    connect_failed_malformed_descriptor: stay("connected"),
    backoff_elapsed: stay("connected"),
    socket_lost: { next: "backoff", effects: ["releaseSocket", "scheduleBackoff"] },
    offline: { next: "offline", effects: ["releaseSocket", "clearTimers"] },
    online: stay("connected"),
    suspend: { next: "suspended", effects: ["releaseSocket", "clearTimers"] },
    // Foreground with a live socket: probe only. If the socket is gone the controller
    // raises `socket_lost` instead, which lands in backoff and reopens before probing.
    resume: { next: "connected", effects: ["probe"] },
    credentials_changed: {
      next: "connecting",
      effects: ["releaseSocket", "resetAttempts", "beginAttempt"],
    },
    retry_now: stay("connected"),
    stable_period_elapsed: { next: "connected", effects: ["resetAttempts"] },
    stop: { next: "idle", effects: ["releaseSocket", "clearTimers"] },
  },

  // ---------------------------------------------------------------------------
  // backoff — retry timer armed at the ladder delay for the current attempt count.
  // ---------------------------------------------------------------------------
  backoff: {
    start: stay("backoff"),
    connect_succeeded: stay("backoff"),
    connect_failed_transient: stay("backoff"),
    connect_budget_expired: stay("backoff"),
    connect_failed_unauthorized: ENTER_BLOCKED,
    connect_failed_version: ENTER_BLOCKED,
    connect_failed_malformed_descriptor: ENTER_BLOCKED,
    backoff_elapsed: { next: "connecting", effects: ["beginAttempt"] },
    socket_lost: stay("backoff"),
    // Parked, but the attempt count is KEPT: a flapping NIC must not reset the ladder.
    offline: { next: "offline", effects: ["clearTimers"] },
    online: { next: "connecting", effects: ["clearTimers", "beginAttempt"] },
    suspend: { next: "suspended", effects: ["clearTimers"] },
    resume: stay("backoff"),
    credentials_changed: {
      next: "connecting",
      effects: ["clearTimers", "resetAttempts", "beginAttempt"],
    },
    retry_now: {
      next: "connecting",
      effects: ["clearTimers", "resetAttempts", "beginAttempt"],
    },
    stable_period_elapsed: stay("backoff"),
    stop: { next: "idle", effects: ["clearTimers"] },
  },

  // ---------------------------------------------------------------------------
  // offline — `navigator.onLine` is false. Parked without burning attempts.
  // ---------------------------------------------------------------------------
  offline: {
    start: stay("offline"),
    connect_succeeded: stay("offline"),
    connect_failed_transient: stay("offline"),
    connect_budget_expired: stay("offline"),
    connect_failed_unauthorized: stay("offline"),
    connect_failed_version: stay("offline"),
    connect_failed_malformed_descriptor: stay("offline"),
    backoff_elapsed: stay("offline"),
    socket_lost: stay("offline"),
    offline: stay("offline"),
    online: { next: "connecting", effects: ["beginAttempt"] },
    suspend: stay("suspended"),
    resume: stay("offline"),
    // No network: re-attempting would burn the attempt for nothing. `online` picks it up.
    credentials_changed: stay("offline"),
    // The user may know better than `navigator.onLine` (captive portals, VPN races).
    retry_now: { next: "connecting", effects: ["beginAttempt"] },
    stable_period_elapsed: stay("offline"),
    stop: { next: "idle", effects: ["clearTimers"] },
  },

  // ---------------------------------------------------------------------------
  // blocked — 401/403, version mismatch, malformed descriptor. ZERO timers, ever.
  // Exits only on a wakeup: credentials-changed, explicit retry, foreground, online.
  // ---------------------------------------------------------------------------
  blocked: {
    start: stay("blocked"),
    connect_succeeded: stay("blocked"),
    connect_failed_transient: stay("blocked"),
    connect_budget_expired: stay("blocked"),
    connect_failed_unauthorized: stay("blocked"),
    connect_failed_version: stay("blocked"),
    connect_failed_malformed_descriptor: stay("blocked"),
    backoff_elapsed: stay("blocked"),
    socket_lost: stay("blocked"),
    // Losing the network does not un-block, and does not start a timer.
    offline: stay("blocked"),
    online: WAKE_FROM_BLOCKED,
    // Backgrounding does not un-block either, and must not downgrade an actionable
    // block to a benign `suspended`: that erases the re-pair affordance (P-10) until
    // another attempt burns. `blocked` already parks with zero timers.
    suspend: stay("blocked"),
    resume: WAKE_FROM_BLOCKED,
    credentials_changed: WAKE_FROM_BLOCKED,
    retry_now: WAKE_FROM_BLOCKED,
    stable_period_elapsed: stay("blocked"),
    stop: { next: "idle", effects: ["clearTimers"] },
  },

  // ---------------------------------------------------------------------------
  // suspended — app backgrounded. Parked WITHOUT burning retry attempts (P-25).
  // Resume runs the full attempt: reopen the WS and complete hello/replay BEFORE
  // the probe. There is no edge from here to `connected`.
  // ---------------------------------------------------------------------------
  suspended: {
    start: stay("suspended"),
    connect_succeeded: stay("suspended"),
    connect_failed_transient: stay("suspended"),
    connect_budget_expired: stay("suspended"),
    connect_failed_unauthorized: stay("suspended"),
    connect_failed_version: stay("suspended"),
    connect_failed_malformed_descriptor: stay("suspended"),
    backoff_elapsed: stay("suspended"),
    socket_lost: stay("suspended"),
    // Connectivity changes are absorbed while parked and re-evaluated on resume.
    offline: stay("suspended"),
    online: stay("suspended"),
    suspend: stay("suspended"),
    resume: { next: "connecting", effects: ["beginAttempt"] },
    credentials_changed: stay("suspended"),
    retry_now: { next: "connecting", effects: ["beginAttempt"] },
    stable_period_elapsed: stay("suspended"),
    stop: { next: "idle", effects: ["clearTimers"] },
  },
};

/** Flattened rows, in a stable order, for table-driven tests. */
export const SUPERVISOR_TRANSITIONS: readonly SupervisorTransitionRow[] =
  SUPERVISOR_STATES.flatMap((from) =>
    SUPERVISOR_EVENTS.map((event) => ({
      from,
      event,
      ...SUPERVISOR_TRANSITION_TABLE[from][event],
    }))
  );

export function lookupTransition(
  from: SupervisorState,
  event: SupervisorEvent
): SupervisorTransition {
  return SUPERVISOR_TRANSITION_TABLE[from][event];
}

// ---------------------------------------------------------------------------
// Presentation projection (§6.5)
// ---------------------------------------------------------------------------

/** The six presentation states the UI renders (§6.5, §6.7). */
export type SupervisorPresentation =
  | "connecting"
  | "reconnecting"
  | "connected"
  | "offline"
  | "error"
  | "suspended";

/**
 * `connecting` presents as `reconnecting` once the environment has ever been connected,
 * so P-9's "lands in reconnecting, never connected" holds across the whole
 * backoff→connecting→backoff cycle rather than flickering back to first-run copy.
 */
export function presentationFor(
  state: SupervisorState,
  hasEverConnected: boolean
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
      return "reconnecting";
    case "idle":
    case "connecting":
      return hasEverConnected ? "reconnecting" : "connecting";
  }
}

// ---------------------------------------------------------------------------
// Retry ladder (§6.5)
// ---------------------------------------------------------------------------

/** `1s, 2s, 4s, 8s, 16s`, then clamped at 16 s forever. */
export const RETRY_LADDER_MS = [1_000, 2_000, 4_000, 8_000, 16_000] as const;

/** The clamp. Named rather than derived so the "forever" half is explicit. */
export const RETRY_LADDER_MAX_MS = 16_000;

export function retryDelayMs(attempt: number): number {
  if (attempt <= 0) {
    return RETRY_LADDER_MS[0];
  }
  return RETRY_LADDER_MS[attempt] ?? RETRY_LADDER_MAX_MS;
}

/** §6.5: one connect attempt gets 15 s end to end. */
export const CONNECT_BUDGET_MS = 15_000;

/** §6.5: the ladder resets after 30 s of uninterrupted connection. */
export const STABLE_PERIOD_MS = 30_000;

/**
 * §3.2 / P-9: the host heartbeats every 20 s, so 50 s of TOTAL frame silence means the
 * host is gone. Combined with the 15 s connect budget this bounds dead-host detection at
 * roughly a minute.
 */
export const FRAME_SILENCE_TIMEOUT_MS = 50_000;
