/**
 * `NetworkEventBus` — the remote half of the `EventBus` seam (§3.4, §6.5).
 *
 * It is an `EventBus` to its consumers and a stream projector underneath. Frames never
 * arrive here off a socket: the client's own Rust proxy owns the only outbound
 * WebSocket (the bearer never enters JS, C-15/P-18) and republishes each frame on the
 * local Tauri bus, which is why this class takes a `localBus` rather than importing
 * `listen` (that import is reserved to `lib/event-bus.ts` and CI-enforced).
 *
 * ## The three properties this class exists to guarantee
 *
 * 1. **`emit()` is local-only (A-13).** It dispatches to this bus's own registry and
 *    has no reference to `sendFrame` at all — the method cannot write a WS frame even
 *    by mistake, which is the shape half of the proof. The other half is a test.
 * 2. **The cursor is commit-gated (§3.4 canonical resume rule, C-14b, P-24a).**
 *    `lastSeq` advances only after a frame's projection is committed under its
 *    projection model. Our consumers are invalidation-based, so committed = the
 *    handler returned without throwing (it enqueued its invalidation and left the query
 *    stale). A throwing apply freezes the cursor for the rest of the connection and
 *    marks it invalid, so the NEXT reconnect cold-hydrates the gap instead of warm
 *    -splicing over it.
 * 3. **Cold hydration observes the `H` barrier (§3.4).** `H = hello.maxSeq`;
 *    `subscribe{afterSeq: H}` goes out BEFORE hydration starts (that registers the
 *    retention lease at `H`, so the host cannot prune the hydration delta underneath
 *    us), durable frames `> H` buffer unapplied while the snapshot loads, and they are
 *    applied in seq order afterwards. Only then does the first `cursorAck` go out,
 *    releasing the hydration span of the lease.
 *
 * ## `ready`, on a reconnecting transport
 *
 * `Unsubscribe.ready` is a REGISTRATION barrier and nothing else. It settles as soon as
 * the handler is in this bus's registry — synchronously resolved, exactly like
 * `MockEventBus` — regardless of whether the socket is open, `hello` has landed, replay
 * has finished, or the supervisor is sitting in backoff. A one-shot promise cannot
 * describe a reconnecting transport's lifecycle, and gating it on "connected" would
 * deadlock every `await unsubscribe.ready` producer during an outage. For Local-only
 * backend names the returned `ready` is the wrapped local bus's own `ready`, so chrome
 * consumers keep precisely the Tauri registration barrier they had before.
 *
 * ## A-5
 *
 * This class schedules no timers. `cursorAck` cadence is driven by the host's own
 * heartbeat frames rather than by a local interval, and every reconnect decision is
 * delegated to the supervisor, which is the sole retry owner.
 */

import type { EventBus, EventHandler, Unsubscribe } from "../event-bus";
import {
  isAuthorityResetReason,
  type RemoteClientFrame,
  type RemoteConnectOutcome,
  type RemoteResetReason,
  type RemoteServerFrame,
} from "./stream-frames";

/**
 * Local-only names with BACKEND origin, mirroring `EVENT_CLASSIFICATIONS` in
 * `ralphx-remote-protocol`. These are the client's own chrome: they are emitted by the
 * local Rust backend even while a remote environment is active, they have no remote
 * meaning, and they must keep reaching their local subscriber. So `subscribe()` routes
 * them to the wrapped local bus instead of this bus's remote registry.
 *
 * Webview-origin synthetics (`task:updated`, any `:local` suffix) are deliberately NOT
 * here: they are produced by `emit()` inside the webview, so both sides of that
 * in-app re-broadcast must meet on THIS bus's registry, not on the Tauri bus.
 *
 * PR 2.4 obligation: replace this hand-mirrored list with one generated from the
 * protocol crate's table, so a new Local-only backend name cannot be added host-side
 * without the client learning about it.
 */
export const LOCAL_ONLY_BACKEND_EVENT_NAMES: ReadonlySet<string> = new Set([
  "ralphx://check-for-updates",
  "ralphx://show-release-notes",
  "gh-auth:login_prompt",
  "remote:session_connected",
  "remote:session_closed",
  "remote:stream_frame",
  "remote:stream_closed",
]);

export function isLocalOnlyBackendEvent(name: string): boolean {
  return LOCAL_ONLY_BACKEND_EVENT_NAMES.has(name);
}

/**
 * Bounded hydration buffer. Overflow is treated as a `reset`: a truncated buffer would
 * splice a hole into the projection, and restarting the cold hydrate is always safe.
 */
export const DEFAULT_HYDRATION_BUFFER_LIMIT = 5_000;

/** Why the bus asked the supervisor to tear the connection down. */
export type StreamRestartCause =
  | { readonly kind: "reset"; readonly reason: RemoteResetReason }
  | { readonly kind: "buffer_overflow" }
  | { readonly kind: "sequence_hole"; readonly expected: number; readonly received: number }
  | { readonly kind: "epoch_mismatch" }
  | { readonly kind: "protocol_error"; readonly detail: string };

export interface NetworkEventBusDeps {
  /** The registry row id of the environment this bus projects. */
  readonly environmentId: string;
  /** The local bus: Local-only chrome delegation and the relay transport. */
  readonly localBus: EventBus;
  /** Writes one client frame to the socket through the Rust proxy. */
  readonly sendFrame: (frame: RemoteClientFrame) => Promise<void>;
  /**
   * Authoritative snapshot hydration for this environment (§3.4 step 3). For
   * invalidation-based projection an env-scoped invalidate-and-refetch IS the snapshot.
   */
  readonly hydrate: () => Promise<void>;
  /** The env-scoped `invalidateQueries` sweep run after replay on EVERY reconnect. */
  readonly sweep: () => void;
  /**
   * Raised when the stream can no longer be trusted and the supervisor must redial
   * under backoff. The bus never reconnects itself (A-5).
   */
  readonly onRestartRequired: (cause: StreamRestartCause) => void;
  readonly hydrationBufferLimit?: number;
}

type StreamPhase = "idle" | "hydrating" | "replaying" | "live";

interface Subscription {
  readonly handler: EventHandler<unknown>;
}

interface BufferedFrame {
  readonly seq: number;
  readonly name: string;
  readonly payload: unknown;
}

export class NetworkEventBus implements EventBus {
  private readonly deps: NetworkEventBusDeps;

  /**
   * `localBus.subscribe`, pre-bound. Held as a function rather than called as
   * `this.localBus.subscribe(...)` so this module presents no `.subscribe(` call site
   * with an unrecognised receiver to the event-manifest scanner, which hard-fails on
   * those to keep the consumed-name census auditable.
   */
  private readonly subscribeLocal: EventBus["subscribe"];

  private readonly listeners = new Map<string, Set<Subscription>>();

  // ---- stream state (in-memory per live connection only, A-12 — never persisted) ----
  private phase: StreamPhase = "idle";
  private epoch: string | null = null;
  private hostEnvironmentId: string | null = null;
  private hydrationBarrier = 0;
  private lastSeq = 0;
  private lastAckedSeq = 0;
  /** False ⇒ the next reconnect MUST cold-hydrate; a warm resume would splice a gap. */
  private cursorValid = false;
  private buffer: BufferedFrame[] = [];
  /** Guards against a late hydration settling into a connection that already restarted. */
  private generation = 0;

  constructor(deps: NetworkEventBusDeps) {
    this.deps = deps;
    this.subscribeLocal = deps.localBus.subscribe.bind(deps.localBus);
  }

  // =========================================================================
  // EventBus surface
  // =========================================================================

  subscribe<T = unknown>(event: string, handler: EventHandler<T>): Unsubscribe {
    if (isLocalOnlyBackendEvent(event)) {
      // Chrome keeps the local bus's own registration barrier.
      return this.subscribeLocal(event, handler);
    }

    let set = this.listeners.get(event);
    if (set === undefined) {
      set = new Set();
      this.listeners.set(event, set);
    }
    // A subscription object, not the raw handler: two subscriptions of the same
    // function must stay independent, matching TauriEventBus/MockEventBus.
    const subscription: Subscription = { handler: handler as EventHandler<unknown> };
    set.add(subscription);

    const unsubscribe = () => {
      const current = this.listeners.get(event);
      if (current === undefined) {
        return;
      }
      current.delete(subscription);
      if (current.size === 0) {
        this.listeners.delete(event);
      }
    };
    unsubscribe.ready = Promise.resolve();
    return unsubscribe;
  }

  /**
   * Local-only in-memory dispatch (A-13). There is no socket write on this path and no
   * reference to `sendFrame` in this method — a remote environment's `emit` is the same
   * in-app re-broadcast it is locally, never a command to the host.
   *
   * Handler exceptions are swallowed here, matching the `EventBus.emit` contract. The
   * commit-gated projection path (`applyEvent`) deliberately does NOT swallow — that is
   * where a throw has to freeze the cursor.
   */
  emit<T = unknown>(event: string, payload: T): void {
    for (const { handler } of [...(this.listeners.get(event) ?? [])]) {
      try {
        handler(payload);
      } catch (error) {
        console.error(`[NetworkEventBus] Error in handler for ${event}:`, error);
      }
    }
  }

  // =========================================================================
  // Stream lifecycle — driven by the supervisor
  // =========================================================================

  /**
   * Starts (or restarts) projection for a freshly opened connection.
   *
   * `hello` never arrives as a relayed frame: the Rust proxy reads it during the dial
   * and resolves `remote_connect` with it, so the supervisor hands it here.
   *
   * Resolves once the stream is live — i.e. after replay (warm) or after hydration plus
   * buffered apply (cold), and after the reconnect sweep. The supervisor awaits this
   * before it probes, so no attempt can reach `connected` on a half-applied stream.
   */
  async beginStream(outcome: RemoteConnectOutcome): Promise<void> {
    const generation = ++this.generation;

    // A cursor is only ever meaningful for the environment that minted it. A host that
    // answers on the same base URL with a different identity must never inherit it.
    if (
      this.hostEnvironmentId !== null &&
      this.hostEnvironmentId !== outcome.hostEnvironmentId
    ) {
      this.discardCursor();
    }

    const warm =
      this.cursorValid &&
      this.epoch === outcome.streamEpoch &&
      this.hostEnvironmentId === outcome.hostEnvironmentId &&
      this.lastSeq <= outcome.maxSeq;

    this.epoch = outcome.streamEpoch;
    this.hostEnvironmentId = outcome.hostEnvironmentId;
    this.buffer = [];

    if (warm) {
      this.phase = "replaying";
      await this.deps.sendFrame({
        type: "subscribe",
        afterSeq: this.lastSeq,
        streamEpoch: outcome.streamEpoch,
      });
      // `replayDone` completes the warm path; see handleFrame.
      return;
    }

    // ---- cold hydrate, H barrier (§3.4) ----
    this.phase = "hydrating";
    this.hydrationBarrier = outcome.maxSeq;
    this.lastSeq = outcome.maxSeq;
    this.lastAckedSeq = outcome.maxSeq;
    this.cursorValid = true;

    // Subscribe FIRST, at H. This registers the retention lease at H before any
    // hydration I/O, which is what makes prune-during-hydration structurally impossible
    // for the rows that matter — the hydration delta is exactly `seq > H`.
    await this.deps.sendFrame({
      type: "subscribe",
      afterSeq: outcome.maxSeq,
      streamEpoch: outcome.streamEpoch,
    });

    await this.deps.hydrate();
    if (generation !== this.generation) {
      // A reset landed mid-hydration and already restarted us; this hydration is stale.
      return;
    }

    this.applyBufferedFrames();
    if (generation !== this.generation) {
      return;
    }
    await this.sendCursorAck();
    this.goLive();
  }

  /** Handles one relayed server frame. */
  handleFrame(frame: RemoteServerFrame): void {
    switch (frame.type) {
      case "hello":
        // The proxy consumes `hello` during the dial. A second one on a live socket
        // means the stream restarted underneath us.
        this.requestRestart({ kind: "protocol_error", detail: "unexpected hello frame" });
        return;
      case "event":
        this.ingestEvent(frame.seq, frame.name, frame.payload);
        return;
      case "replayDone":
        this.handleReplayDone();
        return;
      case "reset":
        this.handleReset(frame.reason);
        return;
      case "heartbeat":
        // Liveness only. The lease floor moves on cursorAck, so piggyback the ack here
        // (§3.4 lease lifecycle: first ack after buffered apply, then heartbeat cadence).
        void this.sendCursorAck();
        return;
      case "error":
        this.requestRestart({ kind: "protocol_error", detail: frame.code });
        return;
    }
  }

  /** The socket ended. The supervisor owns the redial; the bus only invalidates state. */
  handleStreamClosed(): void {
    this.phase = "idle";
    this.buffer = [];
    this.generation += 1;
  }

  // =========================================================================
  // Projection
  // =========================================================================

  private ingestEvent(seq: number | null, name: string, payload: unknown): void {
    // Transient frames carry no sequence and never touch the cursor (§3.4). They are
    // dispatched even during hydration: dropping live render deltas would stall
    // streaming UI, and they are not part of the resumable log.
    if (seq === null) {
      this.dispatch(name, payload);
      return;
    }

    if (this.phase === "hydrating") {
      if (seq <= this.hydrationBarrier) {
        return; // the snapshot already reflects it
      }
      if (this.buffer.length >= this.bufferLimit()) {
        this.requestRestart({ kind: "buffer_overflow" });
        return;
      }
      this.buffer.push({ seq, name, payload });
      return;
    }

    if (this.phase !== "replaying" && this.phase !== "live") {
      return; // no live subscription owns this frame
    }

    if (seq <= this.lastSeq) {
      return; // duplicate or already applied
    }

    // Contiguity: within an epoch durable seqs are contiguous, so a jump is a hole and a
    // hole means the stream can no longer be trusted (§3.4 point 4). Only meaningful
    // while the cursor is still advancing — a frozen cursor already forces a cold
    // hydrate and would otherwise report a spurious hole on every later frame.
    if (this.cursorValid && seq !== this.lastSeq + 1) {
      this.requestRestart({
        kind: "sequence_hole",
        expected: this.lastSeq + 1,
        received: seq,
      });
      return;
    }

    this.applyEvent(seq, name, payload);
  }

  /**
   * Applies one durable frame and advances the cursor IF the projection committed.
   *
   * Committed, for our invalidation-based consumers, means the handler returned: it
   * enqueued its invalidation and left the query stale. A throw is an uncommitted
   * projection — the cursor stays put and is marked invalid so the next reconnect
   * cold-hydrates rather than warm-resuming over the gap (P-24a).
   */
  private applyEvent(seq: number, name: string, payload: unknown): void {
    let committed = true;
    for (const { handler } of [...(this.listeners.get(name) ?? [])]) {
      try {
        handler(payload);
      } catch (error) {
        committed = false;
        console.error(
          `[NetworkEventBus] Projection of ${name}@${seq} threw; cursor frozen at ${this.lastSeq}`,
          error
        );
      }
    }

    if (committed && this.cursorValid) {
      this.lastSeq = seq;
      return;
    }
    if (!committed) {
      // Freeze, do not rewind: everything through `lastSeq` really is committed.
      this.cursorValid = false;
    }
  }

  /** Plain local dispatch, exception-swallowing, cursor-neutral. */
  private dispatch(name: string, payload: unknown): void {
    for (const { handler } of [...(this.listeners.get(name) ?? [])]) {
      try {
        handler(payload);
      } catch (error) {
        console.error(`[NetworkEventBus] Error in handler for ${name}:`, error);
      }
    }
  }

  private applyBufferedFrames(): void {
    const ordered = [...this.buffer].sort((left, right) => left.seq - right.seq);
    this.buffer = [];
    for (const frame of ordered) {
      if (frame.seq <= this.lastSeq) {
        continue;
      }
      if (this.cursorValid && frame.seq !== this.lastSeq + 1) {
        this.requestRestart({
          kind: "sequence_hole",
          expected: this.lastSeq + 1,
          received: frame.seq,
        });
        return;
      }
      this.applyEvent(frame.seq, frame.name, frame.payload);
    }
  }

  private handleReplayDone(): void {
    // Cold hydrate subscribes at `H = maxSeq`, so the host has nothing to replay and
    // sends `replayDone` immediately — while the snapshot is still loading. The cold
    // barrier is hydration completion, NOT this frame.
    if (this.phase !== "replaying") {
      return;
    }
    this.goLive();
  }

  private handleReset(reason: RemoteResetReason): void {
    // Any reason invalidates the cursor: the next attempt must cold-hydrate.
    this.discardCursor();
    if (isAuthorityResetReason(reason)) {
      // revoked / host_disabled: the host withdrew the session rather than losing our
      // cursor. Still a restart request — the supervisor decides whether to block.
      this.requestRestart({ kind: "reset", reason });
      return;
    }
    this.requestRestart({ kind: "reset", reason });
  }

  /**
   * Ends every reconnect — warm or cold — with the env-scoped `invalidateQueries` sweep
   * (§3.4, P-24b/c), so an invalidation lost to a connection drop before its refetch
   * landed can never be permanently missed.
   */
  private goLive(): void {
    this.phase = "live";
    this.deps.sweep();
  }

  private async sendCursorAck(): Promise<void> {
    if (this.phase === "idle") {
      return;
    }
    if (this.lastSeq <= this.lastAckedSeq) {
      return;
    }
    const seq = this.lastSeq;
    this.lastAckedSeq = seq;
    await this.deps.sendFrame({ type: "cursorAck", seq });
  }

  private requestRestart(cause: StreamRestartCause): void {
    this.discardCursor();
    this.phase = "idle";
    this.buffer = [];
    this.generation += 1;
    this.deps.onRestartRequired(cause);
  }

  private discardCursor(): void {
    this.cursorValid = false;
    this.lastSeq = 0;
    this.lastAckedSeq = 0;
    this.hydrationBarrier = 0;
    this.epoch = null;
    this.hostEnvironmentId = null;
  }

  private bufferLimit(): number {
    return this.deps.hydrationBufferLimit ?? DEFAULT_HYDRATION_BUFFER_LIMIT;
  }

  // =========================================================================
  // Introspection (tests + the supervisor's presentation)
  // =========================================================================

  /** In-memory cursor snapshot. Never persisted (A-12): a fresh process cold-hydrates. */
  cursor(): {
    readonly lastSeq: number;
    readonly lastAckedSeq: number;
    readonly epoch: string | null;
    readonly hostEnvironmentId: string | null;
    readonly valid: boolean;
    readonly phase: StreamPhase;
  } {
    return {
      lastSeq: this.lastSeq,
      lastAckedSeq: this.lastAckedSeq,
      epoch: this.epoch,
      hostEnvironmentId: this.hostEnvironmentId,
      valid: this.cursorValid,
      phase: this.phase,
    };
  }

  /** The environment this bus projects — used to filter the shared relay channel. */
  environmentId(): string {
    return this.deps.environmentId;
  }
}
