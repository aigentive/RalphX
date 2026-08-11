/**
 * HTTP client for calling RalphX Tauri backend
 * All MCP tools forward to these endpoints (proxy pattern)
 */

import { safeTrace } from "./redact.js";
import { request as httpRequest } from "node:http";
import type { ClientRequest, IncomingHttpHeaders } from "node:http";

const DEFAULT_TAURI_API_URL = "http://127.0.0.1:3847";
const ALLOWED_TAURI_HOSTS = new Set(["127.0.0.1", "localhost", "::1"]);

function resolveTauriApiBaseUrl(): string {
  const raw = process.env.TAURI_API_URL || DEFAULT_TAURI_API_URL;

  let parsed: URL;
  try {
    parsed = new URL(raw);
  } catch {
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

function normalizeEndpoint(endpoint: string): string {
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

export function buildTauriApiUrl(endpoint: string): string {
  const base = resolveTauriApiBaseUrl();
  const safeEndpoint = normalizeEndpoint(endpoint);
  return new URL(`/api/${safeEndpoint}`, `${base}/`).toString();
}

const MAX_RETRIES = 3;
const BACKOFF_DELAYS_MS = [500, 1000, 2000];

export interface TauriApiError {
  error: string;
  details?: string;
}

export class TauriClientError extends Error {
  constructor(
    message: string,
    public statusCode: number,
    public details?: string
  ) {
    super(message);
    this.name = "TauriClientError";
  }
}

export interface TauriCallOptions {
  headers?: Record<string, string>;
}

type TauriHttpTransport = (url: string, init: RequestInit) => Promise<Response>;

let tauriHttpTransport: TauriHttpTransport = executeNodeHttpRequest;

export function __setTauriHttpTransportForTests(
  transport: TauriHttpTransport | undefined
): void {
  tauriHttpTransport = transport ?? executeNodeHttpRequest;
}

/**
 * Safely parse a 2xx HTTP response body as JSON.
 * Returns null for empty bodies or non-JSON text instead of throwing.
 */
async function safeJsonParse(response: Response): Promise<unknown> {
  const text = await response.text();
  if (!text) return null;
  try {
    return JSON.parse(text);
  } catch {
    return null;
  }
}

/**
 * Returns true if the error is retryable (network errors or 502/503/504).
 * Does NOT retry 4xx client errors or 408 (permission await timeout).
 */
function isRetryable(error: TauriClientError): boolean {
  // statusCode 0 = network error (ECONNREFUSED, ECONNRESET, fetch failure)
  if (error.statusCode === 0) return true;
  // Retry on server-side transient errors only
  return [502, 503, 504].includes(error.statusCode);
}

/**
 * Sleep for the given number of milliseconds.
 */
function sleep(ms: number): Promise<void> {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

function parseValidatedTauriUrl(url: string): URL {
  const parsed = new URL(url);
  if (parsed.protocol !== "http:") {
    throw new TauriClientError(`Invalid Tauri backend protocol: ${parsed.protocol}`, 0);
  }
  if (!ALLOWED_TAURI_HOSTS.has(parsed.hostname)) {
    throw new TauriClientError(`Invalid Tauri backend host: ${parsed.hostname}`, 0);
  }
  if (!/^\d+$/.test(parsed.port)) {
    throw new TauriClientError(`Invalid Tauri backend port: ${parsed.port}`, 0);
  }
  if (!parsed.pathname.startsWith("/api/")) {
    throw new TauriClientError(`Invalid Tauri backend path: ${parsed.pathname}`, 0);
  }
  return parsed;
}

function normalizeRequestHeaders(
  headers: RequestInit["headers"]
): Record<string, string> | undefined {
  if (!headers) return undefined;
  if (headers instanceof Headers) {
    return Object.fromEntries(headers.entries());
  }
  if (Array.isArray(headers)) {
    return Object.fromEntries(headers);
  }
  return headers as Record<string, string>;
}

function toResponseHeaders(headers: IncomingHttpHeaders): Headers {
  const responseHeaders = new Headers();
  for (const [name, value] of Object.entries(headers)) {
    if (Array.isArray(value)) {
      for (const entry of value) {
        responseHeaders.append(name, entry);
      }
    } else if (typeof value === "string") {
      responseHeaders.set(name, value);
    }
  }
  return responseHeaders;
}

function writeRequestBody(request: ClientRequest, body: RequestInit["body"]): void {
  if (body == null) return;
  if (typeof body === "string" || body instanceof Uint8Array) {
    request.write(body);
    return;
  }
  if (body instanceof ArrayBuffer) {
    request.write(new Uint8Array(body));
    return;
  }
  if (body instanceof URLSearchParams) {
    request.write(body.toString());
    return;
  }
  throw new TauriClientError("Unsupported Tauri backend request body type", 0);
}

async function executeNodeHttpRequest(
  url: string,
  init: RequestInit
): Promise<Response> {
  const parsed = parseValidatedTauriUrl(url);

  return new Promise<Response>((resolve, reject) => {
    const request = httpRequest(
      {
        protocol: "http:",
        hostname: "127.0.0.1",
        port: parsed.port,
        path: `${parsed.pathname}${parsed.search}`,
        method: init.method ?? "GET",
        headers: normalizeRequestHeaders(init.headers),
      },
      (response) => {
        const chunks: Buffer[] = [];
        response.on("data", (chunk: Buffer) => {
          chunks.push(chunk);
        });
        response.on("end", () => {
          resolve(
            new Response(Buffer.concat(chunks), {
              status: response.statusCode ?? 500,
              statusText: response.statusMessage,
              headers: toResponseHeaders(response.headers),
            })
          );
        });
        response.on("error", reject);
      }
    );

    request.on("error", reject);

    try {
      writeRequestBody(request, init.body);
      request.end();
    } catch (error) {
      request.destroy();
      reject(error);
    }
  });
}

/**
 * Parse HTTP error response into a TauriClientError.
 * Reads body as text first to avoid consuming the stream before fallback.
 */
async function parseErrorResponse(
  response: Response,
  _url: string
): Promise<TauriClientError> {
  let errorMessage = `Tauri API error: ${response.statusText}`;
  let details: string | undefined;

  try {
    const text = await response.text();
    if (text) {
      try {
        const errorData = JSON.parse(text) as TauriApiError;
        if (errorData.error) {
          errorMessage = errorData.error;
          details = errorData.details;
        } else if (typeof (errorData as unknown as Record<string, unknown>).message === "string") {
          errorMessage = (errorData as unknown as Record<string, unknown>).message as string;
        }
      } catch {
        // Not JSON — use raw text as the error message
        errorMessage = text;
      }
    }
  } catch {
    // text() failed, fall back to statusText
  }

  return new TauriClientError(errorMessage, response.status, details);
}

/**
 * Execute a fetch function with exponential backoff retry.
 * Retries on network errors and 502/503/504.
 * Does NOT retry on 4xx (including 408 permission await timeout).
 */
async function withRetry(
  fetchFn: () => Promise<unknown>,
  label: string
): Promise<unknown> {
  let lastError: TauriClientError | undefined;

  for (let attempt = 0; attempt <= MAX_RETRIES; attempt++) {
    try {
      return await fetchFn();
    } catch (error) {
      if (!(error instanceof TauriClientError)) {
        throw error;
      }
      lastError = error;

      const isLastAttempt = attempt === MAX_RETRIES;
      if (isLastAttempt || !isRetryable(error)) {
        throw error;
      }

      const delayMs = BACKOFF_DELAYS_MS[attempt] ?? 2000;
      console.error(
        `[RalphX MCP] ${label} failed (attempt ${attempt + 1}/${MAX_RETRIES + 1}): ${error.message} — retrying in ${delayMs}ms`
      );
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
async function executeFetch(
  url: string,
  init: RequestInit,
  label: string
): Promise<unknown> {
  return withRetry(async () => {
    safeTrace("backend.request", {
      label,
      method: init.method ?? "GET",
      url,
      has_body: typeof init.body === "string" ? init.body.length > 0 : Boolean(init.body),
      body: typeof init.body === "string" ? init.body : undefined,
    });

    try {
      const response = await tauriHttpTransport(url, init);

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
    } catch (error) {
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
      throw new TauriClientError(
        `Failed to connect to Tauri backend at ${url}: ${
          error instanceof Error ? error.message : String(error)
        }`,
        0
      );
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
export async function callTauri(
  endpoint: string,
  args: Record<string, unknown>,
  options?: TauriCallOptions,
): Promise<unknown> {
  const url = buildTauriApiUrl(endpoint);
  return executeFetch(
    url,
    {
      method: "POST",
      headers: {
        "Content-Type": "application/json",
        ...(options?.headers ?? {}),
      },
      body: JSON.stringify(args),
    },
    `POST /api/${endpoint}`
  );
}

/**
 * Call a Tauri backend endpoint via HTTP GET
 * @param endpoint - Endpoint path (e.g., "task_context/task-123")
 * @returns Response data
 * @throws TauriClientError on HTTP errors
 */
export async function callTauriGet(
  endpoint: string,
  options?: TauriCallOptions,
): Promise<unknown> {
  const url = buildTauriApiUrl(endpoint);
  return executeFetch(
    url,
    {
      method: "GET",
      headers: {
        "Content-Type": "application/json",
        ...(options?.headers ?? {}),
      },
    },
    `GET /api/${endpoint}`
  );
}
