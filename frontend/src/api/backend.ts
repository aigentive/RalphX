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
  return (
    import.meta.env.VITE_RALPHX_BACKEND_URL || defaultBackendBaseUrl()
  ).replace(/\/+$/, "");
}

export function backendApiUrl(endpoint: string): string {
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
  return new URL(`/api/${trimmed.replace(/^\/+/, "")}`, `${backendBaseUrl()}/`)
    .toString();
}
