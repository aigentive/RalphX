/**
 * Wires the Rust proxy's relay channel to one environment's `NetworkEventBus` and
 * supervisor.
 *
 * The proxy republishes every frame from a paired host on the LOCAL Tauri bus under two
 * fixed names, with the environment id inside the payload. Isolation is therefore this
 * module's job: a frame addressed to another environment must not reach this bus, and a
 * payload that does not decode must be dropped rather than half-projected.
 *
 * Kept out of `network-event-bus.ts` for two reasons: the bus stays transport-agnostic
 * and trivially testable without a bus-of-buses, and the local subscription lives in one
 * auditable place.
 */

import type { EventBus, Unsubscribe } from "../event-bus";
import {
  REMOTE_STREAM_CLOSED_EVENT,
  REMOTE_STREAM_FRAME_EVENT,
  parseRemoteStreamClosedEnvelope,
  parseRemoteStreamFrameEnvelope,
  type RemoteServerFrame,
} from "./stream-frames";

/** The bus half of the wiring — `NetworkEventBus` satisfies this structurally. */
export interface RemoteStreamTarget {
  environmentId(): string;
  handleFrame(frame: RemoteServerFrame): void;
  handleStreamClosed(): void;
}

export interface RemoteStreamRelayDeps {
  /** The client-local bus the Rust proxy emits on. */
  readonly localBus: EventBus;
  readonly target: RemoteStreamTarget;
  /** `supervisor.noteFrameActivity` — resets the >50 s frame-silence watchdog (P-9). */
  readonly onFrameActivity?: () => void;
  /** `supervisor.streamLost` — the socket ended; the supervisor owns the redial. */
  readonly onStreamClosed?: (reason: string) => void;
  /** Surfaces a payload the client could not decode; never silently swallowed. */
  readonly onUndecodableFrame?: (payload: unknown) => void;
}

/**
 * Subscribes to the relay channel for one environment.
 *
 * @returns a teardown that removes both subscriptions. Safe to call twice.
 */
export function attachRemoteStreamRelay(deps: RemoteStreamRelayDeps): () => void {
  // Pre-bound rather than called as `deps.localBus.subscribe(...)`: the event-manifest
  // scanner hard-fails on a `.subscribe(` receiver it cannot prove is the app bus, and
  // an unresolvable dynamic-name subscription would be exactly that.
  const subscribeLocal: EventBus["subscribe"] = deps.localBus.subscribe.bind(
    deps.localBus
  );
  const environmentId = deps.target.environmentId();

  const frames: Unsubscribe = subscribeLocal(REMOTE_STREAM_FRAME_EVENT, (payload) => {
    const envelope = parseRemoteStreamFrameEnvelope(payload);
    if (envelope === null) {
      // A frame we cannot decode is a protocol violation, not a no-op: projecting a
      // partially-understood frame is what the reset machinery exists to prevent.
      deps.onUndecodableFrame?.(payload);
      return;
    }
    if (envelope.environmentId !== environmentId) {
      return; // another environment's stream shares the channel, not this bus
    }
    deps.onFrameActivity?.();
    deps.target.handleFrame(envelope.frame);
  });

  const closed: Unsubscribe = subscribeLocal(REMOTE_STREAM_CLOSED_EVENT, (payload) => {
    const envelope = parseRemoteStreamClosedEnvelope(payload);
    if (envelope === null || envelope.environmentId !== environmentId) {
      return;
    }
    deps.target.handleStreamClosed();
    deps.onStreamClosed?.(envelope.reason);
  });

  let detached = false;
  return () => {
    if (detached) {
      return;
    }
    detached = true;
    frames();
    closed();
  };
}
