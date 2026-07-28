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
