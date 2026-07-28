/**
 * The canonical 10-code remote transport error taxonomy (§6.3 / §3.3, R5-L1).
 *
 * These describe the TRANSPORT, never the command. A registered command that runs
 * on the host and returns `Err(E)` is NOT a transport error — `NetworkInvoke`
 * rejects with `E` itself so existing `catch` paths behave exactly as they do over
 * local Tauri IPC (§6.3).
 *
 * Two of the eight are the SAME unknown outcome and must be handled identically:
 * `REMOTE_TIMEOUT_UNKNOWN` (30 s elapsed, the host may or may not have applied the
 * mutation) and `REMOTE_REQUEST_IN_PROGRESS` (the host is already executing this
 * exact `requestId`). Both mean "reconcile by refetching the affected entity" and
 * NEVER "send it again" — a blind re-send with a fresh id is precisely the double
 * agent turn / double approval the host-side reservation exists to prevent (§3.3).
 */

export const REMOTE_TRANSPORT_ERROR_CODES = [
  "REMOTE_COMMAND_UNAVAILABLE",
  "REMOTE_FORBIDDEN",
  "REMOTE_UNAUTHORIZED",
  "REMOTE_UNREACHABLE",
  "REMOTE_VERSION_MISMATCH",
  "REMOTE_TIMEOUT_UNKNOWN",
  "REMOTE_REQUEST_IN_PROGRESS",
  "REMOTE_REQUEST_ID_REUSED",
  // A registered command whose ARGUMENTS were rejected (400). Kept distinct from
  // REMOTE_COMMAND_UNAVAILABLE (404) because that code means "this host does not
  // support the command at all" and is about to gate remote affordances: an argument
  // bug must never be read as a missing capability.
  "REMOTE_INVALID_ARGUMENTS",
  // The host failed while producing the answer (500) — a host-side fault, not a
  // statement about the request.
  "REMOTE_INTERNAL_ERROR",
] as const;

export type RemoteTransportErrorCode =
  (typeof REMOTE_TRANSPORT_ERROR_CODES)[number];

const TRANSPORT_ERROR_CODE_SET: ReadonlySet<string> = new Set(
  REMOTE_TRANSPORT_ERROR_CODES
);

/**
 * The two codes that describe an UNKNOWN outcome (§3.3/§6.3). A caller that sees
 * either must reconcile by refetching; neither authorizes a re-send.
 */
const UNKNOWN_OUTCOME_CODES: ReadonlySet<RemoteTransportErrorCode> = new Set([
  "REMOTE_TIMEOUT_UNKNOWN",
  "REMOTE_REQUEST_IN_PROGRESS",
]);

/**
 * Codes the CLIENT-side Rust proxy can emit that predate the taxonomy
 * (`remote_environment_service.rs`). They are mapped, never dropped, so a proxy
 * refusal can never reach a caller as an untyped string.
 */
const PROXY_ERROR_CODE_ALIASES: Readonly<Record<string, RemoteTransportErrorCode>> =
  {
    // The outbound socket/HTTP path refused to start: nothing left this Mac.
    NOT_CONNECTED: "REMOTE_UNREACHABLE",
    // No bearer could be read, so no authenticated request is possible.
    SECRET_STORE_UNAVAILABLE: "REMOTE_UNAUTHORIZED",
    // The proxy could not prove which environment it was allowed to talk to.
    DATABASE_ERROR: "REMOTE_UNREACHABLE",
  };

export interface RemoteTransportErrorInit {
  readonly code: RemoteTransportErrorCode;
  readonly message: string;
  readonly environmentId: string;
  readonly cmd?: string | null;
  readonly requestId?: string | null;
}

/** A typed transport failure. Never used for a command's own `Err` value. */
export class RemoteTransportError extends Error {
  readonly code: RemoteTransportErrorCode;
  readonly environmentId: string;
  readonly cmd: string | null;
  readonly requestId: string | null;

  constructor(init: RemoteTransportErrorInit) {
    super(init.message);
    this.name = "RemoteTransportError";
    this.code = init.code;
    this.environmentId = init.environmentId;
    this.cmd = init.cmd ?? null;
    this.requestId = init.requestId ?? null;
  }

  /**
   * True when the outcome of the request is UNKNOWN — the mutation may already
   * have been applied host-side. Reconcile by refetch; never re-send (§3.3).
   */
  get isUnknownOutcome(): boolean {
    return isUnknownOutcomeCode(this.code);
  }
}

export function isRemoteTransportError(
  value: unknown
): value is RemoteTransportError {
  return value instanceof RemoteTransportError;
}

export function isRemoteTransportErrorCode(
  value: unknown
): value is RemoteTransportErrorCode {
  return typeof value === "string" && TRANSPORT_ERROR_CODE_SET.has(value);
}

/**
 * `REMOTE_TIMEOUT_UNKNOWN` and `REMOTE_REQUEST_IN_PROGRESS` are the same outcome
 * class: reconcile by refetch, never blind re-send (§6.3 point 6).
 */
export function isUnknownOutcomeCode(code: RemoteTransportErrorCode): boolean {
  return UNKNOWN_OUTCOME_CODES.has(code);
}

/**
 * Extracts a taxonomy code from the Rust proxy's `"{CODE}: {message}"` IPC
 * rendering (`RemoteEnvironmentError::to_command_error`). Returns `null` when the
 * value is not a transport error at all — that is the signal to treat the
 * rejection as the command's own error and re-throw it untouched.
 */
export function parseRemoteTransportErrorCode(
  value: unknown
): RemoteTransportErrorCode | null {
  if (typeof value !== "string") {
    return null;
  }
  const separator = value.indexOf(":");
  const token = (separator === -1 ? value : value.slice(0, separator)).trim();
  if (isRemoteTransportErrorCode(token)) {
    return token;
  }
  return PROXY_ERROR_CODE_ALIASES[token] ?? null;
}

/**
 * Lifts a raw proxy rejection into the taxonomy, or returns it untouched when it
 * carries no transport code (that is the signal to treat it as the command's own
 * error). Every seam that awaits a `remote_*` Tauri command must go through this:
 * `classifyFailure` in the supervisor recognises `RemoteTransportError` instances,
 * never the `"{CODE}: {message}"` strings the proxy actually rejects with, so a raw
 * rejection classifies a revoked credential as `transient` and loops the backoff
 * ladder instead of parking in `blocked`.
 */
export function toRemoteTransportError(
  reason: unknown,
  environmentId: string,
  context?: string
): unknown {
  if (reason instanceof RemoteTransportError) {
    return reason;
  }
  const code = parseRemoteTransportErrorCode(reason);
  if (code === null) {
    return reason;
  }
  const message = parseRemoteTransportErrorMessage(reason);
  return new RemoteTransportError({
    code,
    message: context === undefined ? message : `${message} (${context})`,
    environmentId,
  });
}

/** The human-readable half of `"{CODE}: {message}"`, or the whole value. */
export function parseRemoteTransportErrorMessage(value: unknown): string {
  if (typeof value !== "string") {
    return String(value);
  }
  const separator = value.indexOf(":");
  if (separator === -1) {
    return value;
  }
  return value.slice(separator + 1).trim() || value;
}
