/**
 * HTTP client for calling RalphX Tauri backend
 * All MCP tools forward to these endpoints (proxy pattern)
 */
import { safeTrace } from "./redact.js";
const DEFAULT_TAURI_API_URL = "http://127.0.0.1:3847";
const ALLOWED_TAURI_HOSTS = new Set(["127.0.0.1", "localhost", "::1"]);
function resolveTauriApiBaseUrl() {
    const raw = process.env.TAURI_API_URL || DEFAULT_TAURI_API_URL;
    let parsed;
    try {
        parsed = new URL(raw);
    }
    catch {
        throw new Error(`Invalid TAURI_API_URL: ${raw}`);
    }
    if (parsed.protocol !== "http:") {
        throw new Error(`Invalid TAURI_API_URL protocol: ${parsed.protocol}`);
    }
    if (!ALLOWED_TAURI_HOSTS.has(parsed.hostname)) {
        throw new Error(`Invalid TAURI_API_URL host: ${parsed.hostname}`);
    }
    const effectivePort = parsed.port;
    if (!/^\d+$/.test(effectivePort)) {
        throw new Error(`Invalid TAURI_API_URL port: ${effectivePort}`);
    }
    const portNumber = Number(effectivePort);
    if (!Number.isInteger(portNumber) || portNumber < 1 || portNumber > 65535) {
        throw new Error(`Invalid TAURI_API_URL port: ${effectivePort}`);
    }
    parsed.pathname = "";
    parsed.search = "";
    parsed.hash = "";
    return parsed.toString().replace(/\/$/, "");
}
function normalizeEndpoint(endpoint) {
    const trimmed = endpoint.trim();
    if (trimmed.length === 0) {
        throw new Error("Endpoint must not be empty.");
    }
    if (trimmed.includes("://") || trimmed.startsWith("//")) {
        throw new Error(`Invalid endpoint: ${endpoint}`);
    }
    if (trimmed.includes("..")) {
        throw new Error(`Invalid endpoint traversal sequence: ${endpoint}`);
    }
    if (/[\r\n]/.test(trimmed)) {
        throw new Error(`Invalid endpoint control characters: ${endpoint}`);
    }
    return trimmed.replace(/^\/+/, "");
}
export function buildTauriApiUrl(endpoint) {
    const base = resolveTauriApiBaseUrl();
    const safeEndpoint = normalizeEndpoint(endpoint);
    return new URL(`/api/${safeEndpoint}`, `${base}/`).toString();
}
const MAX_RETRIES = 3;
const BACKOFF_DELAYS_MS = [500, 1000, 2000];
export class TauriClientError extends Error {
    statusCode;
    details;
    constructor(message, statusCode, details) {
        super(message);
        this.statusCode = statusCode;
        this.details = details;
        this.name = "TauriClientError";
    }
}
/**
 * Safely parse a 2xx HTTP response body as JSON.
 * Returns null for empty bodies or non-JSON text instead of throwing.
 */
async function safeJsonParse(response) {
    const text = await response.text();
    if (!text)
        return null;
    try {
        return JSON.parse(text);
    }
    catch {
        return null;
    }
}
/**
 * Returns true if the error is retryable (network errors or 502/503/504).
 * Does NOT retry 4xx client errors or 408 (permission await timeout).
 */
function isRetryable(error) {
    // statusCode 0 = network error (ECONNREFUSED, ECONNRESET, fetch failure)
    if (error.statusCode === 0)
        return true;
    // Retry on server-side transient errors only
    return [502, 503, 504].includes(error.statusCode);
}
/**
 * Sleep for the given number of milliseconds.
 */
function sleep(ms) {
    return new Promise((resolve) => setTimeout(resolve, ms));
}
/**
 * Parse HTTP error response into a TauriClientError.
 * Reads body as text first to avoid consuming the stream before fallback.
 */
async function parseErrorResponse(response, _url) {
    let errorMessage = `Tauri API error: ${response.statusText}`;
    let details;
    try {
        const text = await response.text();
        if (text) {
            try {
                const errorData = JSON.parse(text);
                if (errorData.error) {
                    errorMessage = errorData.error;
                    details = errorData.details;
                }
                else if (typeof errorData.message === "string") {
                    errorMessage = errorData.message;
                }
            }
            catch {
                // Not JSON — use raw text as the error message
                errorMessage = text;
            }
        }
    }
    catch {
        // text() failed, fall back to statusText
    }
    return new TauriClientError(errorMessage, response.status, details);
}
/**
 * Execute a fetch function with exponential backoff retry.
 * Retries on network errors and 502/503/504.
 * Does NOT retry on 4xx (including 408 permission await timeout).
 */
async function withRetry(fetchFn, label) {
    let lastError;
    for (let attempt = 0; attempt <= MAX_RETRIES; attempt++) {
        try {
            return await fetchFn();
        }
        catch (error) {
            if (!(error instanceof TauriClientError)) {
                throw error;
            }
            lastError = error;
            const isLastAttempt = attempt === MAX_RETRIES;
            if (isLastAttempt || !isRetryable(error)) {
                throw error;
            }
            const delayMs = BACKOFF_DELAYS_MS[attempt] ?? 2000;
            console.error(`[RalphX MCP] ${label} failed (attempt ${attempt + 1}/${MAX_RETRIES + 1}): ${error.message} — retrying in ${delayMs}ms`);
            await sleep(delayMs);
        }
    }
    // Unreachable but satisfies TypeScript
    throw lastError;
}
/**
 * Shared fetch executor: performs a single fetch attempt with error parsing.
 * Used by callTauri and callTauriGet to eliminate duplicated logic.
 */
async function executeFetch(url, init, label) {
    return withRetry(async () => {
        safeTrace("backend.request", {
            label,
            method: init.method ?? "GET",
            url,
            has_body: typeof init.body === "string" ? init.body.length > 0 : Boolean(init.body),
            body: typeof init.body === "string" ? init.body : undefined,
        });
        try {
            const response = await fetch(url, init);
            safeTrace("backend.response", {
                label,
                method: init.method ?? "GET",
                url,
                status: response.status,
                ok: response.ok,
            });
            if (!response.ok) {
                throw await parseErrorResponse(response, url);
            }
            return await safeJsonParse(response);
        }
        catch (error) {
            if (error instanceof TauriClientError) {
                safeTrace("backend.error", {
                    label,
                    method: init.method ?? "GET",
                    url,
                    status_code: error.statusCode,
                    message: error.message,
                    details: error.details,
                });
                throw error;
            }
            // Network or other fetch errors
            safeTrace("backend.error", {
                label,
                method: init.method ?? "GET",
                url,
                status_code: 0,
                message: error instanceof Error ? error.message : String(error),
            });
            throw new TauriClientError(`Failed to connect to Tauri backend at ${url}: ${error instanceof Error ? error.message : String(error)}`, 0);
        }
    }, label);
}
/**
 * Call a Tauri backend endpoint via HTTP POST
 * @param endpoint - Endpoint path (e.g., "create_task_proposal")
 * @param args - Request body (JSON)
 * @returns Response data
 * @throws TauriClientError on HTTP errors
 */
export async function callTauri(endpoint, args, options) {
    const url = buildTauriApiUrl(endpoint);
    return executeFetch(url, {
        method: "POST",
        headers: {
            "Content-Type": "application/json",
            ...(options?.headers ?? {}),
        },
        body: JSON.stringify(args),
    }, `POST /api/${endpoint}`);
}
/**
 * Call a Tauri backend endpoint via HTTP GET
 * @param endpoint - Endpoint path (e.g., "task_context/task-123")
 * @returns Response data
 * @throws TauriClientError on HTTP errors
 */
export async function callTauriGet(endpoint, options) {
    const url = buildTauriApiUrl(endpoint);
    return executeFetch(url, {
        method: "GET",
        headers: {
            "Content-Type": "application/json",
            ...(options?.headers ?? {}),
        },
    }, `GET /api/${endpoint}`);
}
//# sourceMappingURL=tauri-client.js.map