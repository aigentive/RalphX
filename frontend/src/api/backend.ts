import {
  getTransportEnvironmentId,
  isRemoteEnvironmentId,
} from "@/lib/remote/active-environment";
import { networkFetch } from "@/lib/remote/network-fetch";

const PRODUCTION_BACKEND_BASE_URL = "http://localhost:3847";
const DEVELOPMENT_BACKEND_BASE_URL = "http://localhost:3857";

function defaultBackendBaseUrl(): string {
  if (import.meta.env.MODE === "test") {
    return PRODUCTION_BACKEND_BASE_URL;
  }
  return import.meta.env.DEV
    ? DEVELOPMENT_BACKEND_BASE_URL
    : PRODUCTION_BACKEND_BASE_URL;
}

export function backendBaseUrl(): string {
  const configuredUrl =
    import.meta.env.MODE === "test"
      ? undefined
      : import.meta.env.VITE_RALPHX_BACKEND_URL;
  return (configuredUrl || defaultBackendBaseUrl()).replace(/\/+$/, "");
}

/**
 * The host-relative path for an API endpoint, with the shape guards that keep an
 * endpoint string from escaping `/api/`.
 *
 * Split out of `backendApiUrl` so the local and remote transports validate
 * IDENTICALLY: the remote path is sent to a proxy that attaches a device bearer, so
 * a value that could climb out of `/api/` or name another origin must be rejected on
 * both paths, not just the one that happens to build a URL.
 */
export function backendApiPath(endpoint: string): string {
  const trimmed = endpoint.trim();
  if (trimmed.length === 0) {
    throw new Error("Backend API endpoint must not be empty.");
  }
  // The guards apply to the PATH only. Callers append an encoded query string to the same
  // argument (`file-diff-page?path=…`), and query VALUES legitimately carry dots and colons —
  // a workspace file named `notes..md` is not traversal, and rejecting it here would fail a
  // fetch the pre-migration `fetch(backendApiUrl(e) + "?" + params)` served fine. A query can
  // only ever be a query: it cannot climb out of `/api/`.
  const queryStart = trimmed.indexOf("?");
  const pathPart = queryStart === -1 ? trimmed : trimmed.slice(0, queryStart);
  const query = queryStart === -1 ? "" : trimmed.slice(queryStart);
  if (pathPart.includes("://") || pathPart.startsWith("//")) {
    throw new Error(`Invalid backend API endpoint: ${endpoint}`);
  }
  if (pathPart.includes("..")) {
    throw new Error(`Invalid backend API endpoint traversal: ${endpoint}`);
  }
  return `/api/${pathPart.replace(/^\/+/, "")}${query}`;
}

export function backendApiUrl(endpoint: string): string {
  return new URL(backendApiPath(endpoint), `${backendBaseUrl()}/`).toString();
}

/**
 * Environment-aware fetch for the backend's HTTP surface (§6.2 point 3).
 *
 * Local environment → plain `fetch` against `backendApiUrl()`, byte-identical to the
 * direct call it replaces. Remote environment → the `remote_fetch` Rust proxy, which
 * holds the bearer and re-mounts the same paths (§3.5).
 *
 * Returns a real `Response` on both paths, which is what makes the ~16 call-site
 * migration mechanical: `res.ok`, `res.status`, and `res.json()` behave the same
 * either way, so no caller learns which transport served it.
 */
export function backendFetch(
  endpoint: string,
  ...init: [init?: RequestInit]
): Promise<Response> {
  const environmentId = getTransportEnvironmentId();
  if (!isRemoteEnvironmentId(environmentId)) {
    // Forwarded by spread, not by named parameter: the local path must call `fetch`
    // with EXACTLY the arguments its call site passed, so a migrated site is
    // indistinguishable from the direct `fetch(...)` it replaces.
    return fetch(backendApiUrl(endpoint), ...init);
  }
  return networkFetch(environmentId, backendApiPath(endpoint), ...init);
}
