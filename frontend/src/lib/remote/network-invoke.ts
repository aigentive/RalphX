/**
 * NetworkInvoke — the remote half of the invoke seam (§6.3).
 *
 * Calls the LOCAL `remote_invoke` Tauri command; the client's own Rust proxy performs
 * `POST {base}/remote/v1/invoke` with the bearer. The bearer never enters JS (C-15)
 * and this module opens no socket of its own (P-18 wrapper half).
 *
 * Contract kept identical to local Tauri IPC so ~116 call sites need no edits (A-3):
 * - success resolves with the command's JSON result;
 * - a command that returned `Err(E)` REJECTS with `E` itself — not wrapped, not
 *   stringified — so existing `catch` blocks read the same value they read locally;
 * - only TRANSPORT failures reject with a typed `RemoteTransportError`.
 *
 * A-5: there are no retries here. One request, one outcome. The supervisor (PR 2.3)
 * is the sole retry owner. A 30 s timeout is an UNKNOWN outcome — the mutation may
 * already be committed host-side — and so is `REMOTE_REQUEST_IN_PROGRESS`; both are
 * reconciled by refetching the affected entity, never by re-sending (§3.3).
 */

import { invoke as primitiveInvoke } from "#tauri-core-primitive";
import type { InvokeArgs } from "#tauri-core-primitive";

import {
  RemoteTransportError,
  parseRemoteTransportErrorCode,
  parseRemoteTransportErrorMessage,
} from "./transport-errors";

/** §6.3: 30 s, then the outcome is unknown. Not a retry budget — a give-up point. */
export const REMOTE_INVOKE_TIMEOUT_MS = 30_000;

/** The local proxy command this module is the only production caller of. */
export const REMOTE_INVOKE_COMMAND = "remote_invoke";

/**
 * Envelope returned by `remote_invoke` (application `RemoteInvokeOutcome`).
 *
 * The discriminant exists because `Result<Value, String>` alone cannot distinguish
 * "the host's command failed" from "the transport failed": a command's `Err` may be
 * any JSON value, and flattening it into the IPC error string would destroy the
 * shape existing `catch` paths depend on.
 */
type RemoteInvokeEnvelope =
  | { readonly outcome: "ok"; readonly result: unknown }
  | { readonly outcome: "commandError"; readonly error: unknown };

function isRemoteInvokeEnvelope(value: unknown): value is RemoteInvokeEnvelope {
  if (typeof value !== "object" || value === null) {
    return false;
  }
  const outcome = (value as { outcome?: unknown }).outcome;
  return outcome === "ok" || outcome === "commandError";
}

/**
 * A fresh id per call (§3.3). Never reuse one across calls: the host binds a
 * `requestId` to a `cmd`+args hash and answers a rebound id with
 * `REMOTE_REQUEST_ID_REUSED`.
 */
export function mintRequestId(): string {
  const webCrypto = globalThis.crypto as Crypto | undefined;
  if (webCrypto && typeof webCrypto.randomUUID === "function") {
    return webCrypto.randomUUID();
  }
  // Non-secure fallback for runtimes without WebCrypto. Uniqueness is what the
  // host needs from a requestId; unpredictability is not part of the contract.
  const random = () => Math.floor(Math.random() * 0x100000000).toString(16).padStart(8, "0");
  return `${random()}-${random()}-${random()}-${random()}`;
}

/** Binary payloads have their own authenticated endpoint (§3.1, C-16). */
function assertJsonArgs(
  cmd: string,
  environmentId: string,
  args: InvokeArgs | undefined
): Record<string, unknown> | undefined {
  if (args === undefined) {
    return undefined;
  }
  if (
    Array.isArray(args) ||
    args instanceof ArrayBuffer ||
    ArrayBuffer.isView(args)
  ) {
    throw new RemoteTransportError({
      code: "REMOTE_COMMAND_UNAVAILABLE",
      message: `\`${cmd}\` sends a binary payload, which the remote invoke wire does not carry. Binary bytes use the attachment endpoints (§3.1).`,
      environmentId,
      cmd,
    });
  }
  return args as Record<string, unknown>;
}

function toTransportError(
  reason: unknown,
  context: { environmentId: string; cmd: string; requestId: string }
): unknown {
  const code = parseRemoteTransportErrorCode(reason);
  if (code === null) {
    // Not a taxonomy code — the proxy rejected for a reason the command's own
    // caller should see verbatim. Re-throw untouched (Tauri parity).
    return reason;
  }
  return new RemoteTransportError({
    code,
    message: parseRemoteTransportErrorMessage(reason),
    environmentId: context.environmentId,
    cmd: context.cmd,
    requestId: context.requestId,
  });
}

/**
 * Dispatches one command to `environmentId` through the local Rust proxy.
 *
 * @throws {RemoteTransportError} for any of the 8 transport codes.
 * @throws the command's own `Err` value for a host-side command failure.
 */
export async function networkInvoke<T>(
  environmentId: string,
  cmd: string,
  args?: InvokeArgs
): Promise<T> {
  const requestId = mintRequestId();
  const jsonArgs = assertJsonArgs(cmd, environmentId, args);

  let timer: ReturnType<typeof setTimeout> | undefined;
  const timeout = new Promise<never>((_resolve, reject) => {
    timer = setTimeout(() => {
      reject(
        new RemoteTransportError({
          code: "REMOTE_TIMEOUT_UNKNOWN",
          message: `\`${cmd}\` did not answer within ${REMOTE_INVOKE_TIMEOUT_MS}ms. The outcome is UNKNOWN — reconcile by refetching, do not re-send.`,
          environmentId,
          cmd,
          requestId,
        })
      );
    }, REMOTE_INVOKE_TIMEOUT_MS);
  });

  // Rule 14 + struct-param wrapping: `remote_invoke(input: RemoteInvokeInput)`.
  const dispatch = primitiveInvoke<unknown>(REMOTE_INVOKE_COMMAND, {
    input: {
      id: environmentId,
      requestId,
      cmd,
      ...(jsonArgs === undefined ? {} : { args: jsonArgs }),
    },
  }).catch((reason: unknown) => {
    throw toTransportError(reason, { environmentId, cmd, requestId });
  });

  let envelope: unknown;
  try {
    // Exactly one dispatch. Whichever settles first wins; there is no second send.
    envelope = await Promise.race([dispatch, timeout]);
  } finally {
    if (timer !== undefined) {
      clearTimeout(timer);
    }
  }

  if (!isRemoteInvokeEnvelope(envelope)) {
    throw new RemoteTransportError({
      code: "REMOTE_VERSION_MISMATCH",
      message: `The remote proxy answered \`${cmd}\` with an envelope this client does not understand.`,
      environmentId,
      cmd,
      requestId,
    });
  }

  if (envelope.outcome === "commandError") {
    // Tauri parity: reject with the command's own error value, unwrapped.
    throw envelope.error;
  }
  return envelope.result as T;
}
