// Pure helpers for the Remote Access pane (PR 1.7).
//
// R-12 decisions live here:
// - The pairing QR/URL encodes ONLY the preferred endpoint (single `host=` param);
//   all candidates stay visible in the endpoints list. The client's §6.1 candidate
//   upsert merges alternates after pairing, so multi-host QR payloads buy nothing.
// - Manual entry: the code renders as its `rxp_` prefix plus four-character groups.
//   Grouping is visual only — the clipboard always carries the canonical raw code.

import type { AdvertisedEndpoint, RemoteListenerStatus } from "@/api/remote-host";

const PAIRING_CODE_PREFIX = "rxp_";
const MANUAL_ENTRY_GROUP_SIZE = 4;

export interface GroupedPairingCode {
  prefix: string;
  groups: string[];
}

/** Splits a pairing code into its prefix and 4-char groups for manual entry (R-12). */
export function groupPairingCode(code: string): GroupedPairingCode {
  const prefix = code.startsWith(PAIRING_CODE_PREFIX) ? PAIRING_CODE_PREFIX : "";
  const body = code.slice(prefix.length);
  const groups: string[] = [];
  for (let index = 0; index < body.length; index += MANUAL_ENTRY_GROUP_SIZE) {
    groups.push(body.slice(index, index + MANUAL_ENTRY_GROUP_SIZE));
  }
  return { prefix, groups };
}

/**
 * Builds `ralphx://pair?host=…#code=…` with the code in the HASH FRAGMENT (§3.7):
 * fragments never reach intermediary servers, so the code stays out of logs.
 */
export function buildPairingUrl(host: string, code: string): string {
  return `ralphx://pair?host=${encodeURIComponent(host)}#code=${code}`;
}

/**
 * Why a pairing input was refused. Rendered as inline field copy, never a banner —
 * every reason names something the user can fix in the field they are looking at.
 */
export type PairingParseReason =
  | "not-a-pairing-url"
  | "missing-host"
  | "missing-code"
  | "code-in-query"
  | "bad-code-prefix"
  | "bad-host-url"
  | "host-url-has-query"
  | "host-url-has-fragment";

export type ParsedPairingUrl =
  | { ok: true; host: string; code: string }
  | { ok: false; reason: PairingParseReason };

export type NormalizedPairingHost =
  | { ok: true; url: string }
  | { ok: false; reason: PairingParseReason };

export type ParsedPairingCode =
  | { ok: true; code: string }
  | { ok: false; reason: PairingParseReason };

const PAIRING_URL_PREFIX = "ralphx://pair";

/**
 * Canonicalises a host string into the `scheme://host[:port]` shape the registry
 * stores as `base_url`.
 *
 * A query string or fragment is REJECTED rather than stripped: `base_url` is the
 * stem every derived URL is built from (join, ws events), so anything unshaped that
 * survives here reappends after the path and produces a URL nobody authored. Bare
 * `host:port` defaults to https; an explicit `http://` is preserved because the
 * tailnet-direct listener terminates plaintext inside WireGuard (see
 * `pickPreferredEndpoint`).
 */
export function normalizePairingHostUrl(raw: string): NormalizedPairingHost {
  const trimmed = raw.trim();
  if (trimmed === "") {
    return { ok: false, reason: "missing-host" };
  }
  if (trimmed.includes("#")) {
    return { ok: false, reason: "host-url-has-fragment" };
  }
  if (trimmed.includes("?")) {
    return { ok: false, reason: "host-url-has-query" };
  }

  const hasScheme = /^[a-zA-Z][a-zA-Z0-9+.-]*:\/\//.test(trimmed);
  const candidate = hasScheme ? trimmed : `https://${trimmed}`;

  let parsed: URL;
  try {
    parsed = new URL(candidate);
  } catch {
    return { ok: false, reason: "bad-host-url" };
  }
  if (parsed.protocol !== "https:" && parsed.protocol !== "http:") {
    return { ok: false, reason: "bad-host-url" };
  }
  if (parsed.hostname === "") {
    return { ok: false, reason: "missing-host" };
  }
  // Only the root path is meaningful for a base URL; a deeper path is a paste error.
  if (parsed.pathname !== "" && parsed.pathname !== "/") {
    return { ok: false, reason: "bad-host-url" };
  }
  return { ok: true, url: `${parsed.protocol}//${parsed.host}` };
}

/**
 * Accepts the grouped display form (`rxp_ ABCD 1234`) or the canonical raw code.
 * Grouping is presentation only (R-12) — what comes back is always the raw code that
 * `pair_remote_environment` receives.
 */
export function parseManualPairingCode(raw: string): ParsedPairingCode {
  const compact = raw.replace(/\s+/g, "");
  if (compact === "") {
    return { ok: false, reason: "missing-code" };
  }
  if (!compact.startsWith(PAIRING_CODE_PREFIX)) {
    return { ok: false, reason: "bad-code-prefix" };
  }
  if (compact.length === PAIRING_CODE_PREFIX.length) {
    return { ok: false, reason: "missing-code" };
  }
  return { ok: true, code: compact };
}

/**
 * Inverse of `buildPairingUrl`: `ralphx://pair?host=…#code=…`.
 *
 * The code is read from the HASH FRAGMENT ONLY. A code found in the query string is
 * REJECTED rather than accepted — the query is exactly the part that reaches proxies
 * and access logs, so a code that arrived there must be treated as burned and
 * regenerated on the host, not quietly used.
 *
 * Never throws; every failure is a typed reason the form renders beside the field.
 */
export function parsePairingUrl(raw: string): ParsedPairingUrl {
  const trimmed = raw.trim();
  if (!trimmed.toLowerCase().startsWith(PAIRING_URL_PREFIX)) {
    return { ok: false, reason: "not-a-pairing-url" };
  }

  // `ralphx:` is not a special scheme, so `URL` leaves the authority in the pathname;
  // splitting the query/fragment by hand keeps the parse independent of that quirk.
  const withoutPrefix = trimmed.slice(PAIRING_URL_PREFIX.length);
  const hashIndex = withoutPrefix.indexOf("#");
  const query = (hashIndex === -1 ? withoutPrefix : withoutPrefix.slice(0, hashIndex))
    .replace(/^\?/, "");
  const fragment = hashIndex === -1 ? "" : withoutPrefix.slice(hashIndex + 1);

  const queryParams = new URLSearchParams(query);
  if (queryParams.has("code")) {
    return { ok: false, reason: "code-in-query" };
  }

  const host = queryParams.get("host");
  if (host === null || host.trim() === "") {
    return { ok: false, reason: "missing-host" };
  }
  const normalizedHost = normalizePairingHostUrl(host);
  if (!normalizedHost.ok) {
    return normalizedHost;
  }

  const rawCode = new URLSearchParams(fragment).get("code");
  if (rawCode === null || rawCode.trim() === "") {
    return { ok: false, reason: "missing-code" };
  }
  const code = parseManualPairingCode(rawCode);
  if (!code.ok) {
    return code;
  }

  return { ok: true, host: normalizedHost.url, code: code.code };
}

/**
 * Preferred pairing endpoint (R-12): first available advertised endpoint, else the
 * first advertised endpoint, else — for tailnet-direct only — the actual bound
 * address as plain `http://` (the direct listener terminates plaintext HTTP inside
 * WireGuard; an https URL would fail its handshake). Serve mode without endpoint
 * data yields null: an honest "enter host manually" beats a fabricated URL.
 *
 * Always derived from `bindAddress`, never `status.port` — `RALPHX_REMOTE_PORT`
 * overrides surface only through the bound address.
 */
export function pickPreferredEndpoint(
  endpoints: AdvertisedEndpoint[] | null,
  status: RemoteListenerStatus,
): string | null {
  if (endpoints && endpoints.length > 0) {
    const available = endpoints.find((endpoint) => endpoint.available);
    return (available ?? endpoints[0])?.url ?? null;
  }
  if (status.exposureMode === "tailnetDirect" && status.running && status.bindAddress) {
    return `http://${status.bindAddress}`;
  }
  return null;
}

/** Seconds until `expiresAt`, clamped at zero; unparseable input counts as expired. */
export function remainingSeconds(expiresAt: string, nowMs: number): number {
  const expiryMs = Date.parse(expiresAt);
  if (Number.isNaN(expiryMs)) {
    return 0;
  }
  return Math.max(0, Math.floor((expiryMs - nowMs) / 1000));
}

/** Renders a countdown as `M:SS`. */
export function formatCountdown(totalSeconds: number): string {
  const clamped = Math.max(0, totalSeconds);
  const minutes = Math.floor(clamped / 60);
  const seconds = clamped % 60;
  return `${minutes}:${String(seconds).padStart(2, "0")}`;
}
