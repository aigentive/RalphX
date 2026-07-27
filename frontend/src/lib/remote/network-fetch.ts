/**
 * The fetch half of the client transport switch (§6.2 point 3, §3.5).
 *
 * A remote `backendFetch` goes through the local `remote_fetch` Tauri command, so the
 * bearer stays in Rust and the webview issues no cross-origin request — no CORS, no
 * preflight, no token in JS (C-15, P-18).
 *
 * The proxy answers `{status, body}` and this module rebuilds a real `Response` from
 * it. That is what makes the migration mechanical: every call site keeps reading
 * `res.ok`, `res.status`, and `res.json()` exactly as it does over local HTTP. A
 * non-2xx is DATA here — only 401/403 are lifted into the transport taxonomy by the
 * Rust side, because those describe the transport's authority rather than the
 * endpoint's answer.
 */

import { invoke as primitiveInvoke } from "#tauri-core-primitive";

import {
  RemoteTransportError,
  parseRemoteTransportErrorCode,
  parseRemoteTransportErrorMessage,
} from "./transport-errors";

/** Statuses the `Response` constructor refuses a body for. */
const NULL_BODY_STATUSES: ReadonlySet<number> = new Set([204, 205, 304]);

/**
 * Reason phrases worth preserving: a few call sites put `res.statusText` in their
 * error message, and the proxy envelope carries only the numeric status.
 */
const REASON_PHRASES: Readonly<Record<number, string>> = {
  400: "Bad Request",
  401: "Unauthorized",
  403: "Forbidden",
  404: "Not Found",
  409: "Conflict",
  422: "Unprocessable Entity",
  429: "Too Many Requests",
  500: "Internal Server Error",
  502: "Bad Gateway",
  503: "Service Unavailable",
  504: "Gateway Timeout",
};

interface RemoteFetchEnvelope {
  readonly status: number;
  readonly body: string;
}

function isRemoteFetchEnvelope(value: unknown): value is RemoteFetchEnvelope {
  if (typeof value !== "object" || value === null) {
    return false;
  }
  const candidate = value as { status?: unknown; body?: unknown };
  return typeof candidate.status === "number" && typeof candidate.body === "string";
}

/** Normalises any `HeadersInit` into the pair list the proxy command takes. */
export function toHeaderPairs(headers: HeadersInit | undefined): [string, string][] {
  if (headers === undefined) {
    return [];
  }
  if (headers instanceof Headers) {
    return [...headers.entries()];
  }
  if (Array.isArray(headers)) {
    return headers.map(([name, value]) => [String(name), String(value)]);
  }
  return Object.entries(headers).map(([name, value]) => [name, String(value)]);
}

/**
 * Only text bodies cross the proxy. Binary uploads have their own authenticated
 * streaming endpoint (§3.1, C-16) and must not be smuggled through a JSON command.
 */
function toTextBody(
  path: string,
  environmentId: string,
  body: BodyInit | null | undefined
): string | null {
  if (body === null || body === undefined) {
    return null;
  }
  if (typeof body === "string") {
    return body;
  }
  throw new RemoteTransportError({
    code: "REMOTE_COMMAND_UNAVAILABLE",
    message: `A non-text body for ${path} cannot cross the remote fetch proxy; binary bytes use the attachment endpoints (§3.1).`,
    environmentId,
  });
}

/**
 * Performs one proxied request against a remounted host route and returns it as a
 * `Response`, so callers cannot tell which transport served them.
 */
export async function networkFetch(
  environmentId: string,
  path: string,
  init?: RequestInit | undefined
): Promise<Response> {
  const method = (init?.method ?? "GET").toUpperCase();
  const headers = toHeaderPairs(init?.headers);
  const body = toTextBody(path, environmentId, init?.body);

  let envelope: unknown;
  try {
    // Rule 14 + struct-param wrapping: `remote_fetch(input: RemoteFetchInput)`.
    envelope = await primitiveInvoke<unknown>("remote_fetch", {
      input: {
        id: environmentId,
        path,
        method,
        headers,
        ...(body === null ? {} : { body }),
      },
    });
  } catch (reason: unknown) {
    throw toFetchTransportError(reason, environmentId, path);
  }

  if (!isRemoteFetchEnvelope(envelope)) {
    throw new RemoteTransportError({
      code: "REMOTE_VERSION_MISMATCH",
      message: `The remote proxy answered ${path} with an envelope this client does not understand.`,
      environmentId,
    });
  }

  return toResponse(envelope, environmentId, path);
}

function toResponse(
  envelope: RemoteFetchEnvelope,
  environmentId: string,
  path: string
): Response {
  // `Response` refuses statuses outside 200-599, so an out-of-range value has to be
  // surfaced as a transport failure rather than crashing the call site.
  if (envelope.status < 200 || envelope.status > 599) {
    throw new RemoteTransportError({
      code: "REMOTE_VERSION_MISMATCH",
      message: `The remote proxy reported HTTP ${envelope.status} for ${path}, which is not a representable response status.`,
      environmentId,
    });
  }
  const init: ResponseInit = {
    status: envelope.status,
    ...(REASON_PHRASES[envelope.status] === undefined
      ? {}
      : { statusText: REASON_PHRASES[envelope.status] as string }),
    headers: { "Content-Type": "application/json" },
  };
  return new Response(
    NULL_BODY_STATUSES.has(envelope.status) ? null : envelope.body,
    init
  );
}

function toFetchTransportError(
  reason: unknown,
  environmentId: string,
  path: string
): unknown {
  if (reason instanceof RemoteTransportError) {
    return reason;
  }
  // Reuse the invoke taxonomy parser: `remote_fetch` renders its failures with the
  // same `"{CODE}: {message}"` convention.
  const code = parseRemoteTransportErrorCode(reason);
  if (code === null) {
    return reason;
  }
  return new RemoteTransportError({
    code,
    message: `${parseRemoteTransportErrorMessage(reason)} (${path})`,
    environmentId,
  });
}
