/**
 * The per-environment connection supervisor (§6.5).
 *
 * One instance per REGISTERED remote environment, created at app start — never lazily on
 * first activation, because a background environment's connection is what 3.3's per-env
 * liveness badges read. `"local"` has no supervisor at all (§6.4).
 *
 * This is the SOLE retry owner (A-5). `NetworkInvoke` performs exactly one request and
 * `NetworkEventBus` schedules no timers; every backoff, ladder decision, and redial
 * lives here.
 *
 * ## The FSM is table-driven, not switch-driven
 *
 * There is no `switch (state)` below. `dispatch()` looks the edge up in
 * `SUPERVISOR_TRANSITION_TABLE` and runs the effects the row names, so the checked-in
 * table is the implementation rather than a description of it. A transition the table
 * defines and this file fails to honour is a test failure against the table itself.
 *
 * ## The connect attempt (§6.5 key point 2)
 *
 * descriptor (version gate + environmentId check) → ws-ticket + WS open + `hello`
 * (both inside the Rust proxy's one-shot `remote_connect`) → **scope refresh** →
 * cold hydrate at the `H` barrier or a cursor-validated warm resume, ending in the
 * env-scoped `invalidateQueries` sweep → `health_check` probe → `connected`. 15 s budget.
 *
 * `connected` requires the socket AND `hello`/replay AND the probe. There is deliberately
 * no path from a bare successful probe to `connected` (P-25): a probe answers happily on
 * a host that has no live socket for us, which would leave the UI "connected" to a dead
 * event stream.
 *
 * ## Decision — P-28 introspection placement: AFTER `hello`, BEFORE the probe
 *
 * The source spec leaves the exact position open. We refresh effective scopes with
 * `GET /remote/v1/session` once `hello` has landed and before the probe declares the
 * attempt good, because:
 *
 * - `hello` is what authenticates the host's identity and protocol version. Refreshing
 *   scopes only after it means a proxy-cached descriptor can never make the client adopt
 *   a scope set belonging to a different environment.
 * - Doing it before the dial would spend a round-trip inside the 15 s budget on attempts
 *   that die at the ticket or the upgrade.
 * - Doing it before the probe is what makes the binding constraint hold: no attempt can
 *   reach `connected` on a stale scope view. Widening is visible on the next reconnect
 *   without re-pairing, narrowing takes effect on the next reconnect, and an
 *   introspection failure fails CLOSED — the last confirmed set is retained (never
 *   widened), the failure is surfaced through `onScopeRefreshFailed`, and the attempt
 *   fails rather than connecting on an unverified view.
 *
 * ## Decision — desktop `suspended` granularity: app-level background, not window blur
 *
 * `suspend`/`resume` are driven by `document.visibilitychange` (plus `pagehide`), NOT by
 * window blur. Blur fires on every focus change — devtools, Spotlight, a glance at
 * another app — and treating each one as suspend/resume would release and redial the
 * socket many times a minute. That is the reconnect churn `suspended` exists to avoid,
 * and each redial burns a single-use ws-ticket and a `MAX_SESSIONS_PER_DEVICE` slot on
 * the host. `document.hidden` tracks real backgrounding, fires once per real transition,
 * and is the desktop analogue of the RN `AppState` transitions this state was added for.
 * A short debounce additionally absorbs visibility flapping.
 */

import {
  CONNECT_BUDGET_MS,
  FRAME_SILENCE_TIMEOUT_MS,
  STABLE_PERIOD_MS,
  lookupTransition,
  presentationFor,
  retryDelayMs,
  type SupervisorEffect,
  type SupervisorEvent,
  type SupervisorPresentation,
  type SupervisorState,
} from "./supervisor-transition-table";
import type { RemoteConnectOutcome } from "./stream-frames";
import { RemoteTransportError } from "./transport-errors";

/** Visibility flapping shorter than this never becomes a suspend (churn safety). */
export const SUSPEND_DEBOUNCE_MS = 2_000;

/** Why an attempt failed, in the vocabulary the FSM understands. */
export type AttemptFailure =
  | "transient"
  | "unauthorized"
  | "version"
  | "malformed_descriptor"
  | "invalid_request";

/** A typed attempt failure so classification never depends on message matching (rule 5). */
export class ConnectAttemptError extends Error {
  readonly failure: AttemptFailure;
  readonly cause: unknown;

  constructor(failure: AttemptFailure, message: string, cause?: unknown) {
    super(message);
    this.name = "ConnectAttemptError";
    this.failure = failure;
    this.cause = cause;
  }
}

/** The descriptor fields the version/identity gate reads (§3.2). */
export interface EnvironmentDescriptorView {
  readonly environmentId: string;
  readonly protocolVersion: number;
  readonly minClientProtocol: number;
}

export interface SupervisorDeps {
  readonly environmentId: string;
  /** The host identity this client paired with; a descriptor that disagrees is malformed. */
  readonly expectedHostEnvironmentId: string;
  /** This client's protocol version (`ralphx_remote_protocol::PROTOCOL_VERSION`). */
  readonly clientProtocolVersion: number;
  /** The oldest host protocol this client still speaks. */
  readonly clientMinProtocol: number;

  /** `GET /.well-known/ralphx/environment` through the proxy. */
  readonly fetchDescriptor: () => Promise<EnvironmentDescriptorView>;
  /** One-shot `remote_connect`: ws-ticket → WS dial → `hello`. No retry inside. */
  readonly openStream: () => Promise<RemoteConnectOutcome>;
  /** `GET /remote/v1/session` — the effective grant, re-read on every (re)connect (P-28). */
  readonly refreshScopes: () => Promise<readonly string[]>;
  /** Adopts a CONFIRMED scope set. Never called with an unconfirmed or stale set. */
  readonly applyScopes: (scopes: readonly string[]) => void;
  /** `NetworkEventBus.beginStream` — cold hydrate or warm resume, ending in the sweep. */
  readonly beginStream: (outcome: RemoteConnectOutcome) => Promise<void>;
  /** The `health_check` probe. */
  readonly probe: () => Promise<void>;
  /** `remote_disconnect`. Must be idempotent. */
  readonly releaseStream: () => Promise<void>;
  /** Whether the Rust proxy still holds a live socket for this environment. */
  readonly hasLiveSocket: () => boolean;

  readonly onStateChange?: (
    state: SupervisorState,
    presentation: SupervisorPresentation
  ) => void;
  /** Surfaced, never swallowed: a failed introspection must be visible (P-28). */
  readonly onScopeRefreshFailed?: (error: unknown) => void;
  readonly onBlocked?: (failure: AttemptFailure, message: string) => void;
}

interface TimerHandles {
  retry: ReturnType<typeof setTimeout> | null;
  budget: ReturnType<typeof setTimeout> | null;
  stable: ReturnType<typeof setTimeout> | null;
  silence: ReturnType<typeof setTimeout> | null;
  suspend: ReturnType<typeof setTimeout> | null;
}

export class ConnectionSupervisor {
  private readonly deps: SupervisorDeps;

  private state: SupervisorState = "idle";
  private hasEverConnected = false;
  private attempts = 0;
  /** Bumped on every attempt start/abort so a late resolution cannot settle a stale one. */
  private attemptGeneration = 0;
  private blockedReason: { failure: AttemptFailure; message: string } | null = null;
  /** The last CONFIRMED scope set. Gating never widens past this (P-28 fail-closed). */
  private confirmedScopes: readonly string[] | null = null;

  private readonly timers: TimerHandles = {
    retry: null,
    budget: null,
    stable: null,
    silence: null,
    suspend: null,
  };

  constructor(deps: SupervisorDeps) {
    this.deps = deps;
  }

  // =========================================================================
  // Public surface
  // =========================================================================

  /**
   * The FSM entry point. Every convenience method below and every environment adapter
   * (visibility, `navigator.onLine`, the relay's close notification) funnels through
   * here, so there is exactly one writer of `state` and the table-driven tests can
   * exercise the same edges production takes.
   */
  handleEvent(event: SupervisorEvent): void {
    this.dispatch(event);
  }

  start(): void {
    this.dispatch("start");
  }

  stop(): void {
    this.clearTimer("suspend");
    this.dispatch("stop");
  }

  /** Explicit user retry — one of the four `blocked` wakeups. */
  retryNow(): void {
    this.dispatch("retry_now");
  }

  /** A `blocked` wakeup: the device credential changed. */
  credentialsChanged(): void {
    this.dispatch("credentials_changed");
  }

  /** `navigator.onLine` transitions. */
  networkChanged(online: boolean): void {
    this.dispatch(online ? "online" : "offline");
  }

  /**
   * App-level visibility. Debounced (D2): a blur-shaped flicker must not become a
   * suspend/resume cycle, because each cycle costs a socket, a ws-ticket, and a
   * per-device session slot.
   */
  visibilityChanged(hidden: boolean): void {
    this.clearTimer("suspend");
    if (!hidden) {
      this.foreground();
      return;
    }
    this.timers.suspend = setTimeout(() => {
      this.timers.suspend = null;
      this.dispatch("suspend");
    }, SUSPEND_DEBOUNCE_MS);
  }

  /**
   * Foreground handling (§6.5): while `connected`, probe if the socket is still open;
   * if the socket is gone, run the FULL reopen-then-probe sequence rather than trusting
   * a bare probe (P-25).
   */
  foreground(): void {
    if (this.state === "connected" && !this.deps.hasLiveSocket()) {
      this.dispatch("socket_lost");
      this.dispatch("retry_now");
      return;
    }
    this.dispatch("resume");
  }

  /** The stream died or can no longer be trusted (bus restart request, socket close). */
  streamLost(): void {
    this.dispatch("socket_lost");
  }

  /**
   * The host WITHDREW this device's session rather than losing our place in it —
   * `reset(revoked)` / `reset(host_disabled)` (§3.2, stream-frames' authority reasons).
   *
   * This is deliberately NOT `streamLost`: redialling a host that has already refused
   * this device spins the retry ladder forever and burns a ws-ticket per cycle, while
   * the user never sees the actionable re-pair state (P-10). It parks in `blocked` with
   * a populated reason, exactly like a 401 from the attempt itself.
   */
  authorityWithdrawn(message: string): void {
    this.blockedReason = { failure: "unauthorized", message };
    this.deps.onBlocked?.("unauthorized", message);
    this.dispatch("connect_failed_unauthorized");
  }

  /**
   * Called for every frame the relay delivers. Resets the frame-silence watchdog:
   * >50 s of total silence means the host is gone (P-9), which the host's own 20 s
   * heartbeat makes a 2.5-beat budget.
   */
  noteFrameActivity(): void {
    if (this.state !== "connected" && this.state !== "connecting") {
      return;
    }
    this.armSilenceWatchdog();
  }

  currentState(): SupervisorState {
    return this.state;
  }

  presentation(): SupervisorPresentation {
    return presentationFor(this.state, this.hasEverConnected);
  }

  /** Populated only while `blocked`; drives the actionable UI (P-10). */
  blocked(): { readonly failure: AttemptFailure; readonly message: string } | null {
    return this.state === "blocked" ? this.blockedReason : null;
  }

  /** The last CONFIRMED effective scopes. `null` until one introspection succeeded. */
  scopes(): readonly string[] | null {
    return this.confirmedScopes;
  }

  /** Retry-ladder attempt count — asserted by the P-25 "parks without burning" test. */
  attemptCount(): number {
    return this.attempts;
  }

  // =========================================================================
  // FSM core
  // =========================================================================

  private dispatch(event: SupervisorEvent): void {
    const from = this.state;
    const transition = lookupTransition(from, event);
    this.state = transition.next;

    for (const effect of transition.effects) {
      this.runEffect(effect);
    }

    if (this.state !== "blocked") {
      this.blockedReason = null;
    }
    if (from !== this.state) {
      this.deps.onStateChange?.(this.state, this.presentation());
    }
  }

  private runEffect(effect: SupervisorEffect): void {
    switch (effect) {
      case "cancelAttempt":
        this.attemptGeneration += 1;
        this.clearTimer("budget");
        return;
      case "releaseSocket":
        this.clearTimer("silence");
        void this.deps.releaseStream().catch(() => {
          // Teardown is best effort: a proxy that already dropped the socket is the
          // outcome we wanted, and a throw here must not abort the transition.
        });
        return;
      case "clearTimers":
        this.clearTimer("retry");
        this.clearTimer("budget");
        this.clearTimer("stable");
        this.clearTimer("silence");
        return;
      case "resetAttempts":
        this.attempts = 0;
        return;
      case "beginAttempt":
        this.beginAttempt();
        return;
      case "scheduleBackoff":
        this.scheduleBackoff();
        return;
      case "armStableTimer":
        this.hasEverConnected = true;
        this.clearTimer("stable");
        this.timers.stable = setTimeout(() => {
          this.timers.stable = null;
          this.dispatch("stable_period_elapsed");
        }, STABLE_PERIOD_MS);
        this.armSilenceWatchdog();
        return;
      case "probe":
        void this.deps.probe().catch(() => {
          // A foreground probe that fails means the socket is not usable after all.
          this.dispatch("socket_lost");
        });
        return;
    }
  }

  // =========================================================================
  // The one-shot connect attempt
  // =========================================================================

  private beginAttempt(): void {
    const generation = ++this.attemptGeneration;

    this.clearTimer("budget");
    this.timers.budget = setTimeout(() => {
      this.timers.budget = null;
      if (generation !== this.attemptGeneration) {
        return;
      }
      this.dispatch("connect_budget_expired");
    }, CONNECT_BUDGET_MS);

    void this.runAttempt(generation);
  }

  private async runAttempt(generation: number): Promise<void> {
    const live = () => generation === this.attemptGeneration;
    try {
      // 1. Descriptor: version gate + identity check, before any credential is spent.
      //    A descriptor that cannot be FETCHED is transient (an unreachable host must
      //    retry, not park in `blocked`). A descriptor that cannot be PARSED is the
      //    third `blocked` entry, and `fetchDescriptor` raises that itself as a
      //    `ConnectAttemptError("malformed_descriptor")`.
      const descriptor = await this.deps.fetchDescriptor().catch((error: unknown) => {
        throw toAttemptError(error, "transient", "descriptor fetch failed");
      });
      if (!live()) return;
      this.gateDescriptor(descriptor);

      // 2. Rust one-shot: ws-ticket → WS dial → hello.
      const outcome = await this.deps.openStream().catch((error: unknown) => {
        throw toAttemptError(error, "transient", "opening the event stream failed");
      });
      if (!live()) return;

      // 3. `hello` repeats the version, so a proxy-cached descriptor cannot lie (P-10).
      this.gateHelloVersion(descriptor, outcome);
      if (outcome.hostEnvironmentId !== this.deps.expectedHostEnvironmentId) {
        throw new ConnectAttemptError(
          "malformed_descriptor",
          `host identity ${outcome.hostEnvironmentId} is not the paired environment ${this.deps.expectedHostEnvironmentId}`
        );
      }

      // 4. P-28: refresh the EFFECTIVE grant on every (re)connect, never by re-pairing.
      //    Fail closed — an unverified refresh must not widen gating, and must not be
      //    silently tolerated into a connected session.
      let scopes: readonly string[];
      try {
        scopes = await this.deps.refreshScopes();
      } catch (error: unknown) {
        this.deps.onScopeRefreshFailed?.(error);
        if (error instanceof RemoteTransportError && error.code === "REMOTE_UNAUTHORIZED") {
          throw new ConnectAttemptError("unauthorized", error.message, error);
        }
        throw new ConnectAttemptError(
          "transient",
          "effective scopes could not be confirmed; keeping the last confirmed set",
          error
        );
      }
      if (!live()) return;
      this.confirmedScopes = scopes;
      this.deps.applyScopes(scopes);

      // 5. Cold hydrate at the H barrier or a cursor-validated warm resume, ending in
      //    the env-scoped invalidateQueries sweep.
      await this.deps.beginStream(outcome).catch((error: unknown) => {
        throw toAttemptError(error, "transient", "hydrating the event stream failed");
      });
      if (!live()) return;

      // 6. Only now the probe. Never a probe alone (§6.5, P-25).
      await this.deps.probe().catch((error: unknown) => {
        throw toAttemptError(error, "transient", "health probe failed");
      });
      if (!live()) return;

      this.clearTimer("budget");
      this.dispatch("connect_succeeded");
    } catch (error: unknown) {
      if (!live()) return;
      this.clearTimer("budget");
      this.failAttempt(error);
    }
  }

  private gateDescriptor(descriptor: EnvironmentDescriptorView): void {
    if (descriptor.environmentId !== this.deps.expectedHostEnvironmentId) {
      throw new ConnectAttemptError(
        "malformed_descriptor",
        `descriptor reports environment ${descriptor.environmentId}, expected ${this.deps.expectedHostEnvironmentId}`
      );
    }
    if (descriptor.minClientProtocol > this.deps.clientProtocolVersion) {
      throw new ConnectAttemptError(
        "version",
        `host requires client protocol >= ${descriptor.minClientProtocol}, this client speaks ${this.deps.clientProtocolVersion}`
      );
    }
    if (descriptor.protocolVersion < this.deps.clientMinProtocol) {
      throw new ConnectAttemptError(
        "version",
        `host speaks protocol ${descriptor.protocolVersion}, this client requires >= ${this.deps.clientMinProtocol}`
      );
    }
  }

  /**
   * The whole point of `hello` repeating the version (§3.2, P-10).
   *
   * Two checks, and only these two. A host may legitimately run a NEWER protocol than
   * this client as long as its `minClientProtocol` still admits us — the descriptor gate
   * above already decided that — so "hello is newer than us" is not by itself skew, and
   * blocking on it would park every paired client the moment a host upgrades.
   *
   * What IS skew: a host too old for us, and a host whose `hello` CONTRADICTS the
   * descriptor we version-gated on. The second is the lying-cached-descriptor case: the
   * descriptor claimed a version that admitted us, the live socket reports a different
   * one, and the gate we passed was therefore evaluated against a value the host does
   * not actually serve.
   */
  private gateHelloVersion(
    descriptor: EnvironmentDescriptorView,
    outcome: RemoteConnectOutcome
  ): void {
    if (outcome.protocolVersion < this.deps.clientMinProtocol) {
      throw new ConnectAttemptError(
        "version",
        `hello reports protocol ${outcome.protocolVersion}, this client requires >= ${this.deps.clientMinProtocol}`
      );
    }
    if (outcome.protocolVersion !== descriptor.protocolVersion) {
      throw new ConnectAttemptError(
        "version",
        `hello reports protocol ${outcome.protocolVersion} but the descriptor advertised ${descriptor.protocolVersion}; the version gate was evaluated against a value this host does not serve`
      );
    }
  }

  private failAttempt(error: unknown): void {
    const failure = classifyFailure(error);
    const message = error instanceof Error ? error.message : String(error);
    if (failure !== "transient") {
      this.blockedReason = { failure, message };
      this.deps.onBlocked?.(failure, message);
    }
    switch (failure) {
      case "unauthorized":
        this.dispatch("connect_failed_unauthorized");
        return;
      case "version":
        this.dispatch("connect_failed_version");
        return;
      case "malformed_descriptor":
        this.dispatch("connect_failed_malformed_descriptor");
        return;
      case "invalid_request":
        this.dispatch("connect_failed_invalid_request");
        return;
      case "transient":
        this.dispatch("connect_failed_transient");
    }
  }

  // =========================================================================
  // Timers
  // =========================================================================

  private scheduleBackoff(): void {
    const delay = retryDelayMs(this.attempts);
    this.attempts += 1;
    this.clearTimer("retry");
    this.timers.retry = setTimeout(() => {
      this.timers.retry = null;
      this.dispatch("backoff_elapsed");
    }, delay);
  }

  /**
   * P-9: the host heartbeats every 20 s, so 50 s of TOTAL frame silence means it is gone.
   * The client closes rather than waiting for TCP to notice, which is what bounds
   * dead-host detection at about a minute.
   */
  private armSilenceWatchdog(): void {
    this.clearTimer("silence");
    this.timers.silence = setTimeout(() => {
      this.timers.silence = null;
      this.dispatch("socket_lost");
    }, FRAME_SILENCE_TIMEOUT_MS);
  }

  private clearTimer(name: keyof TimerHandles): void {
    const handle = this.timers[name];
    if (handle !== null) {
      clearTimeout(handle);
      this.timers[name] = null;
    }
  }

  /** Test/diagnostic view: which timers are armed. P-10 asserts none while `blocked`. */
  armedTimers(): readonly (keyof TimerHandles)[] {
    return (Object.keys(this.timers) as (keyof TimerHandles)[]).filter(
      (name) => this.timers[name] !== null
    );
  }
}

// ===========================================================================
// Failure classification — typed, never message matching (rule 5)
// ===========================================================================

function classifyFailure(error: unknown): AttemptFailure {
  if (error instanceof ConnectAttemptError) {
    return error.failure;
  }
  if (error instanceof RemoteTransportError) {
    switch (error.code) {
      case "REMOTE_UNAUTHORIZED":
      case "REMOTE_FORBIDDEN":
        return "unauthorized";
      case "REMOTE_VERSION_MISMATCH":
        return "version";
      case "REMOTE_INVALID_ARGUMENTS":
        // A client-side request bug. Retrying an identical malformed request cannot
        // succeed, so it must not enter the backoff ladder as transient. It is NOT a
        // malformed descriptor: the host's identity was fine, the request was not.
        return "invalid_request";
      default:
        return "transient";
    }
  }
  return "transient";
}

/**
 * Wraps a raw failure, preserving a typed transport code where one exists. The
 * `fallback` applies only when the error carries no classification of its own — a 401
 * from the descriptor fetch is `unauthorized`, not `malformed_descriptor`.
 */
function toAttemptError(
  error: unknown,
  fallback: AttemptFailure,
  message: string
): ConnectAttemptError {
  if (error instanceof ConnectAttemptError) {
    return error;
  }
  const classified = classifyFailure(error);
  const failure = classified === "transient" ? fallback : classified;
  const detail = error instanceof Error ? error.message : String(error);
  return new ConnectAttemptError(failure, `${message}: ${detail}`, error);
}
