// Failure taxonomy for the add-environment flow (PR 2.5).
//
// The Rust service already decided what went wrong and encoded it as a stable code on
// the IPC boundary (`"{CODE}: {message}"`, `RemoteEnvironmentError::to_command_error`).
// This module only READS that code. It never re-derives a protocol comparison and never
// pattern-matches the prose — a message that says "expired" may well be a transport
// failure, and rendering it as a bad pairing code would send the user to regenerate a
// code that was never the problem.

/** What the user can do about it, which is the only distinction the UI needs. */
export type PairingFailureKind =
  "version" | "code" | "unreachable" | "url" | "identity" | "unknown";

export interface PairingFailure {
  kind: PairingFailureKind;
  /** The backend's stable code, or `""` when the throwable carried none. */
  code: string;
  message: string;
}

const KIND_BY_CODE: Record<string, PairingFailureKind> = {
  REMOTE_VERSION_MISMATCH: "version",
  PAIRING_REJECTED: "code",
  REMOTE_UNAUTHORIZED: "code",
  REMOTE_UNREACHABLE: "unreachable",
  INVALID_PAIRING_URL: "url",
  HOST_IDENTITY_MISMATCH: "identity",
};

const CODE_PREFIX = /^([A-Z][A-Z0-9_]*): ([\s\S]*)$/;

export function classifyPairingError(error: unknown): PairingFailure {
  const raw =
    error instanceof Error
      ? error.message
      : typeof error === "string" && error.length > 0
        ? error
        : "The pairing attempt failed for an unknown reason.";

  const match = CODE_PREFIX.exec(raw);
  if (match === null) {
    return { kind: "unknown", code: "", message: raw };
  }
  const code = match[1] ?? "";
  const message = match[2] ?? raw;
  return { kind: KIND_BY_CODE[code] ?? "unknown", code, message };
}

export interface PairingFailureCopy {
  title: string;
  detail: string;
}

/**
 * User-facing copy per kind. Every sentence names the next action, because there is no
 * automatic retry anywhere in this feature (A-5) — the user is the retry mechanism.
 */
export function describePairingFailure(
  failure: PairingFailure,
): PairingFailureCopy {
  switch (failure.kind) {
    case "version":
      return {
        title: "Versions are incompatible",
        detail: `${failure.message}. Update RalphX on this Mac or on the host, then try again.`,
      };
    case "code":
      return {
        title: "Pairing failed",
        detail:
          "The code was rejected (expired or already used). Generate a fresh code on the host and try again.",
      };
    case "unreachable":
      return {
        title: "Host unreachable",
        detail:
          "This Mac could not reach the host. Check that the host is awake, on the same tailnet, and has Remote Access running, then try again.",
      };
    case "url":
      return {
        title: "That address cannot be used",
        detail: `${failure.message}. Enter the host exactly as the host's Remote Access pane shows it.`,
      };
    case "identity":
      return {
        title: "Host identity changed",
        detail:
          "The host that answered is not the one this code was issued for. Generate a fresh code on the host you intend to pair with.",
      };
    case "unknown":
      return { title: "Pairing failed", detail: failure.message };
  }
}
