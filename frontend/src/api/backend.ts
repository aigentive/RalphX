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
  if (trimmed.includes("://") || trimmed.startsWith("//")) {
    throw new Error(`Invalid backend API endpoint: ${endpoint}`);
  }
  if (trimmed.includes("..")) {
    throw new Error(`Invalid backend API endpoint traversal: ${endpoint}`);
  }
  return `/api/${trimmed.replace(/^\/+/, "")}`;
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
