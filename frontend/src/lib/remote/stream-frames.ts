/**
 * Client-side mirror of the remote event-stream wire framing (§3.2).
 *
 * The authority is `ralphx-remote-protocol` (`ServerFrame` / `ClientFrame` /
 * `ResetReason`). Both enums are serialized `#[serde(tag = "type", rename_all =
 * "camelCase", rename_all_fields = "camelCase")]`, so the variant names arrive
 * camelCased (`replayDone`, `cursorAck`, `heartbeatAck`) and so do the fields.
 *
 * Frames do NOT come off a socket in the webview. The client's own Rust proxy holds the
 * only outbound WebSocket (the bearer never enters JS, C-15/P-18) and republishes each
 * frame on the local Tauri bus under a FIXED name with the environment id in the
 * payload — see `RemoteStreamFrameEnvelope`. A dynamic per-environment emit name would
 * be unresolvable to the P-6 emit scanner and cannot live in the protocol crate's
 * literal-name classification table, so isolation is enforced here by filtering the
 * envelope's `environmentId` rather than by the channel name.
 *
 * Every decoder below is fail-closed: an unrecognised shape returns `null` and the
 * caller treats it as a protocol violation. A partially-understood frame must never be
 * projected as truth — that is exactly the silent-splice the `reset` machinery exists
 * to prevent.
 */

import { PROTOCOL_RESET_REASONS } from "./remote-vocabulary.generated";

/**
 * `ralphx_remote_protocol::ResetReason`. Any reason means the same thing to a client: cold
 * hydrate. GENERATED from the protocol vocabulary snapshot and guarded by
 * `scripts/check-remote-vocabulary-mirror.mjs`: a hand mirror let a new Rust reason make the
 * client reject the whole frame as malformed.
 */
export const REMOTE_RESET_REASONS = PROTOCOL_RESET_REASONS;

export type RemoteResetReason = (typeof REMOTE_RESET_REASONS)[number];

const RESET_REASON_SET: ReadonlySet<string> = new Set(REMOTE_RESET_REASONS);

/**
 * `reset(revoked)` and `reset(host_disabled)` are terminal for the ATTEMPT, not merely a
 * cold-hydrate trigger: the host has withdrawn the session rather than lost the cursor.
 * Reconnecting on `revoked` without a credentials change would spin against a host that
 * has already refused this device.
 */
const AUTHORITY_RESET_REASONS: ReadonlySet<RemoteResetReason> = new Set([
  "revoked",
  "host_disabled",
]);

export function isAuthorityResetReason(reason: RemoteResetReason): boolean {
  return AUTHORITY_RESET_REASONS.has(reason);
}

/** `ralphx_remote_protocol::ServerFrame`. */
export type RemoteServerFrame =
  | {
      readonly type: "hello";
      readonly protocolVersion: number;
      readonly environmentId: string;
      readonly streamEpoch: string;
      readonly serverVersion: string;
      readonly maxSeq: number;
      readonly heartbeatSecs: number;
    }
  | {
      readonly type: "event";
      /** `null` for the transient class — live-only frames carry no sequence (§3.4). */
      readonly seq: number | null;
      readonly name: string;
      readonly payload: unknown;
    }
  | { readonly type: "replayDone"; readonly throughSeq: number }
  | { readonly type: "reset"; readonly reason: RemoteResetReason }
  | { readonly type: "heartbeat"; readonly t: number }
  | { readonly type: "error"; readonly code: string; readonly message: string };

/** `ralphx_remote_protocol::ClientFrame` — everything this client may write to the socket. */
export type RemoteClientFrame =
  | { readonly type: "subscribe"; readonly afterSeq: number; readonly streamEpoch: string }
  | { readonly type: "cursorAck"; readonly seq: number }
  | { readonly type: "heartbeatAck"; readonly t: number };

/**
 * The relay channel names, mirroring `remote_event_relay.rs`. FIXED, never formatted per
 * environment: a dynamic `AppHandle::emit` name is unresolvable to the P-6 emit scanner
 * (which fails the build rather than skipping it), and the protocol crate's
 * classification table is a literal-name allowlist. Both are classified Local-only, so a
 * client's own relay can never be captured and fanned back out.
 */
export const REMOTE_STREAM_FRAME_EVENT = "remote:stream_frame";
export const REMOTE_STREAM_CLOSED_EVENT = "remote:stream_closed";

/** Payload of the fixed `remote:stream_frame` relay event. */
export interface RemoteStreamFrameEnvelope {
  readonly environmentId: string;
  readonly frame: RemoteServerFrame;
}

/** Payload of the fixed `remote:stream_closed` relay event. */
export interface RemoteStreamClosedEnvelope {
  readonly environmentId: string;
  readonly reason: string;
}

/** Hello, as `remote_connect` resolves it (application `RemoteConnectOutcome`). */
export interface RemoteConnectOutcome {
  readonly environmentId: string;
  readonly hostEnvironmentId: string;
  readonly streamEpoch: string;
  readonly maxSeq: number;
  readonly heartbeatSecs: number;
  readonly protocolVersion: number;
}

function asRecord(value: unknown): Record<string, unknown> | null {
  return typeof value === "object" && value !== null
    ? (value as Record<string, unknown>)
    : null;
}

function asFiniteNumber(value: unknown): number | null {
  return typeof value === "number" && Number.isFinite(value) ? value : null;
}

function asString(value: unknown): string | null {
  return typeof value === "string" ? value : null;
}

/**
 * Decodes one server frame. Returns `null` for anything not exactly on the wire contract.
 *
 * Deliberately strict about `seq`: a durable frame with a missing or non-numeric `seq`
 * would be indistinguishable from a transient one, and treating it as transient would
 * skip it in the cursor without ever forcing a cold hydrate.
 */
export function parseRemoteServerFrame(value: unknown): RemoteServerFrame | null {
  const record = asRecord(value);
  if (record === null) {
    return null;
  }
  switch (record.type) {
    case "hello": {
      const protocolVersion = asFiniteNumber(record.protocolVersion);
      const environmentId = asString(record.environmentId);
      const streamEpoch = asString(record.streamEpoch);
      const serverVersion = asString(record.serverVersion);
      const maxSeq = asFiniteNumber(record.maxSeq);
      const heartbeatSecs = asFiniteNumber(record.heartbeatSecs);
      if (
        protocolVersion === null ||
        environmentId === null ||
        streamEpoch === null ||
        serverVersion === null ||
        maxSeq === null ||
        heartbeatSecs === null
      ) {
        return null;
      }
      return {
        type: "hello",
        protocolVersion,
        environmentId,
        streamEpoch,
        serverVersion,
        maxSeq,
        heartbeatSecs,
      };
    }
    case "event": {
      const name = asString(record.name);
      if (name === null) {
        return null;
      }
      const rawSeq = record.seq;
      if (rawSeq !== null && rawSeq !== undefined && asFiniteNumber(rawSeq) === null) {
        return null;
      }
      return {
        type: "event",
        seq: rawSeq === null || rawSeq === undefined ? null : (rawSeq as number),
        name,
        payload: record.payload ?? null,
      };
    }
    case "replayDone": {
      const throughSeq = asFiniteNumber(record.throughSeq);
      return throughSeq === null ? null : { type: "replayDone", throughSeq };
    }
    case "reset": {
      const reason = asString(record.reason);
      return reason !== null && RESET_REASON_SET.has(reason)
        ? { type: "reset", reason: reason as RemoteResetReason }
        : null;
    }
    case "heartbeat": {
      const t = asFiniteNumber(record.t);
      return t === null ? null : { type: "heartbeat", t };
    }
    case "error": {
      const code = asString(record.code);
      const message = asString(record.message);
      return code === null || message === null ? null : { type: "error", code, message };
    }
    default:
      return null;
  }
}

/**
 * Reads ONLY the addressee of a relay envelope, without decoding the frame.
 *
 * The shared channel carries every environment's frames, so a relay must decide
 * whose frame this is BEFORE deciding whether it decodes: an undecodable frame
 * addressed to environment B is B's protocol violation, and letting it tear down
 * environment A's healthy stream would defeat the isolation this layer exists for.
 */
export function readRemoteStreamFrameEnvironmentId(value: unknown): string | null {
  const record = asRecord(value);
  return record === null ? null : asString(record.environmentId);
}

/** Decodes a `remote:stream_frame` payload, rejecting anything without a usable frame. */
export function parseRemoteStreamFrameEnvelope(
  value: unknown
): RemoteStreamFrameEnvelope | null {
  const record = asRecord(value);
  if (record === null) {
    return null;
  }
  const environmentId = asString(record.environmentId);
  if (environmentId === null) {
    return null;
  }
  const frame = parseRemoteServerFrame(record.frame);
  return frame === null ? null : { environmentId, frame };
}

/** Decodes a `remote:stream_closed` payload. */
export function parseRemoteStreamClosedEnvelope(
  value: unknown
): RemoteStreamClosedEnvelope | null {
  const record = asRecord(value);
  if (record === null) {
    return null;
  }
  const environmentId = asString(record.environmentId);
  if (environmentId === null) {
    return null;
  }
  return { environmentId, reason: asString(record.reason) ?? "closed" };
}
