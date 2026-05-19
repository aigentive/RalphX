#!/usr/bin/env node

/**
 * ralphx-external-mcp — External MCP Server for RalphX Orchestration API
 *
 * Exposes orchestration-level tools to external agents (reefbot.ai, etc.) via
 * MCP over HTTP+SSE (Streamable HTTP Transport).
 *
 * Architecture:
 *   External Agent → Bearer auth → Rate limit → MCP tools → :3847 backend
 *
 * Security:
 *   - Bearer API keys (rxk_live_ prefix) validated against :3847/api/auth/validate-key
 *   - TLS required for non-localhost binds
 *   - Token bucket rate limiting per key + IP-based auth throttle
 *   - X-RalphX-Project-Scope header injected for backend enforcement
 */

// Side-effect import — installs the keep-alive dispatcher BEFORE any other
// module can issue a fetch() against the Tauri backend. Must stay first.
import "./keep-alive.js";

import { realpathSync } from "node:fs";
import { createServer as createHttpServer } from "node:http";
import { createServer as createHttpsServer } from "node:https";
import { fileURLToPath } from "node:url";
import { randomUUID } from "node:crypto";
import type { Server as HttpServer } from "node:http";

import { Server } from "@modelcontextprotocol/sdk/server/index.js";
import { StreamableHTTPServerTransport } from "@modelcontextprotocol/sdk/server/streamableHttp.js";
import type { IncomingMessage, ServerResponse } from "node:http";

import { authMiddleware, configureAuth, invalidateCacheByKeyId } from "./auth.js";
import { configureRateLimiter, getRateLimiter } from "./rate-limiter.js";
import { validateTlsConfig, buildTlsOptions } from "./tls.js";
import { configureBackendClient } from "./backend-client.js";
import { handleHealth, handleReady } from "./health.js";
import { registerTools } from "./tools/index.js";
import type { ExternalMcpConfig, ApiKeyContext } from "./types.js";

const DEFAULT_CONFIG: ExternalMcpConfig = {
  port: 3848,
  host: "127.0.0.1",
  backendUrl: "http://127.0.0.1:3847",
};

/** Active MCP transports keyed by session ID */
const activeTransports = new Map<string, StreamableHTTPServerTransport>();

/** Active connection count — tracked via httpServer connection/close events */
let _activeConnections = 0;

/** Module-scope HTTP/HTTPS server handle — set by startServer(), used by shutdown() */
let _httpServer: HttpServer | undefined;

/** Returns the current number of active TCP connections (for testing). */
export function getActiveConnections(): number {
  return _activeConnections;
}

/**
 * Create and configure the MCP server instance.
 */
function createMcpServer(): Server {
  const server = new Server(
    {
      name: "ralphx-external-mcp",
      version: "1.0.0",
    },
    {
      capabilities: {
        tools: {},
      },
    }
  );

  return server;
}

/**
 * Main request handler — dispatches to /health, /ready, or MCP endpoint.
 */
async function handleRequest(
  req: IncomingMessage,
  res: ServerResponse,
  config: ExternalMcpConfig
): Promise<void> {
  const url = req.url ?? "/";

  // Health and readiness endpoints — no auth required
  if (url === "/health") {
    handleHealth(req, res);
    return;
  }

  if (url === "/ready") {
    await handleReady(req, res);
    return;
  }

  if (url === "/api/auth/invalidate-cache" && req.method === "POST") {
    await handleInvalidateCache(req, res);
    return;
  }

  // Connection limit — checked before auth to shed load early
  const maxConnections = config.rateLimit?.maxConnections ?? 50;
  if (_activeConnections >= maxConnections) {
    sendError(res, 503, "Connection limit reached — try again later");
    return;
  }

  // All other requests require auth
  const ip = getClientIp(req);
  const rateLimiter = getRateLimiter();

  // Check IP-based auth throttle before attempting validation
  if (!rateLimiter.checkIpAuth(ip)) {
    sendError(res, 429, "Too many authentication failures — try again later");
    return;
  }

  // Validate bearer token
  const keyContext = await authMiddleware(req, res);
  if (!keyContext) {
    // authMiddleware already sent the error response
    rateLimiter.recordAuthFailure(ip);
    return;
  }

  // Auth succeeded — reset IP failure count
  rateLimiter.recordAuthSuccess(ip);

  // Apply per-key rate limit
  if (!rateLimiter.checkKey(keyContext.keyId)) {
    sendError(res, 429, "Rate limit exceeded — reduce request frequency");
    return;
  }

  // Route to MCP endpoint
  if (url === "/mcp" || url.startsWith("/mcp?")) {
    await handleMcpRequest(req, res, keyContext, config);
    return;
  }

  sendError(res, 404, "Not found");
}

/**
 * Handle MCP protocol requests using StreamableHTTPServerTransport.
 * Supports both stateful (session-based) and stateless modes.
 */
async function handleMcpRequest(
  req: IncomingMessage,
  res: ServerResponse,
  keyContext: ApiKeyContext,
  _config: ExternalMcpConfig
): Promise<void> {
  // Pre-parse the request body for POST requests before handing to transport.
  // The SDK's StreamableHTTPServerTransport converts the Node.js stream to a Web Standard
  // Request via @hono/node-server, and req.json() on the converted request can fail on
  // chunked bodies, encoding issues, or partial reads. Pre-parsing avoids the fragile
  // Node.js→Web Standard stream conversion as a failure point.
  // Non-POST requests (GET for SSE polling, DELETE for session close) have no body to parse.
  let parsedBody: unknown;
  if (req.method === "POST") {
    const bodyResult = await readBodyString(req);
    if (!bodyResult.ok) {
      sendError(res, 500, "Stream read failure");
      return;
    }
    if (bodyResult.body.trim() === "") {
      sendError(res, 400, "Empty request body");
      return;
    }
    try {
      parsedBody = JSON.parse(bodyResult.body);
    } catch {
      sendJsonRpcParseError(res);
      return;
    }
  }

  // Session management: look up existing transport or create new one
  const sessionId = req.headers["mcp-session-id"] as string | undefined;

  if (sessionId && activeTransports.has(sessionId)) {
    // Resume existing session
    const transport = activeTransports.get(sessionId)!;
    await transport.handleRequest(req, res, parsedBody);
    return;
  }

  // New session — only POST is valid for initialization
  if (req.method !== "POST") {
    sendError(res, 405, "Method not allowed — new sessions require POST");
    return;
  }
  if (!isInitializeRequest(parsedBody)) {
    sendJsonRpcServerError(
      res,
      jsonRpcIdFromBody(parsedBody),
      "Server not initialized — send initialize before other MCP requests"
    );
    return;
  }

  // Create new MCP server + transport per session (stateful mode)
  const server = createMcpServer();

  // Register tools with key context provider
  let currentKeyContext: ApiKeyContext | undefined = withRuntimeContextFromUrl(
    keyContext,
    req.url
  );
  registerTools(server, () => currentKeyContext);

  const transport = new StreamableHTTPServerTransport({
    sessionIdGenerator: () => randomUUID(),
    onsessioninitialized: (sid) => {
      activeTransports.set(sid, transport);
      console.error(`[ralphx-external-mcp] Session initialized: ${sid}`);
    },
    onsessionclosed: (sid) => {
      activeTransports.delete(sid);
      currentKeyContext = undefined;
      console.error(`[ralphx-external-mcp] Session closed: ${sid}`);
    },
  });

  transport.onerror = (err) => {
    console.error("[ralphx-external-mcp] Transport error:", err);
  };

  await server.connect(transport);
  await transport.handleRequest(req, res, parsedBody);
}

function withRuntimeContextFromUrl(
  keyContext: ApiKeyContext,
  rawUrl: string | undefined
): ApiKeyContext {
  if (keyContext.tauriOrigin !== true || !rawUrl) {
    return keyContext;
  }

  let url: URL;
  try {
    url = new URL(rawUrl, "http://127.0.0.1");
  } catch {
    return keyContext;
  }

  const runtime = {
    contextType: url.searchParams.get("context_type") ?? undefined,
    contextId: url.searchParams.get("context_id") ?? undefined,
    projectId: url.searchParams.get("project_id") ?? undefined,
    parentConversationId:
      url.searchParams.get("parent_conversation_id") ?? undefined,
  };
  if (
    !runtime.contextType &&
    !runtime.contextId &&
    !runtime.projectId &&
    !runtime.parentConversationId
  ) {
    return keyContext;
  }

  return {
    ...keyContext,
    runtime,
  };
}

/** Reset connection counter — used in tests to clean up between runs. */
export function resetActiveConnections(): void {
  _activeConnections = 0;
}

/** Returns the current HTTP server handle — for testing shutdown behavior. */
export function getHttpServer(): HttpServer | undefined {
  return _httpServer;
}

/**
 * Gracefully shut down the server: stop accepting connections, close all
 * active MCP transports, then exit. A 1-second force-exit timer ensures we
 * stay within the Rust supervisor's 2-second SIGTERM window.
 */
export async function shutdown(): Promise<void> {
  console.error("[ralphx-external-mcp] Graceful shutdown initiated...");

  // Safety net: force-exit after 1s if graceful drain stalls.
  const forceTimer = setTimeout(() => {
    console.error("[ralphx-external-mcp] Force exit after 1s drain timeout");
    process.exit(0);
  }, 1000);

  // Stop accepting new HTTP connections
  if (_httpServer) {
    await new Promise<void>((resolve) => {
      _httpServer!.close(() => resolve());
    });
  }

  // Close all active MCP transport sessions
  for (const [sid, transport] of activeTransports) {
    try {
      await transport.close();
    } catch {
      // Ignore individual transport close errors
    }
    activeTransports.delete(sid);
    console.error(`[ralphx-external-mcp] Transport closed: ${sid}`);
  }

  clearTimeout(forceTimer);
  console.error("[ralphx-external-mcp] Shutdown complete");
  process.exit(0);
}

// Register graceful shutdown handlers
process.on("SIGTERM", shutdown);
process.on("SIGINT", shutdown);

/**
 * Start the external MCP server.
 */
export async function startServer(
  config: Partial<ExternalMcpConfig> = {}
): Promise<void> {
  const cfg: ExternalMcpConfig = { ...DEFAULT_CONFIG, ...config };

  // Validate TLS requirements before starting
  validateTlsConfig(cfg.host, cfg.tls);

  // Configure singletons
  configureAuth({ backendUrl: cfg.backendUrl });
  configureBackendClient({ baseUrl: cfg.backendUrl });

  if (cfg.rateLimit) {
    configureRateLimiter({
      requestsPerSecond: cfg.rateLimit.requestsPerSecond,
      authFailuresBeforeLockout: cfg.rateLimit.authFailuresBeforeLockout,
      lockoutDurationMs: cfg.rateLimit.lockoutDurationSecs * 1000,
    });
  }

  // Create HTTP or HTTPS server based on TLS config
  const requestHandler = (req: IncomingMessage, res: ServerResponse): void => {
    handleRequest(req, res, cfg).catch((err) => {
      console.error("[ralphx-external-mcp] Unhandled request error:", err);
      if (!res.headersSent) {
        sendError(res, 500, "Internal server error");
      }
    });
  };

  const httpServer =
    cfg.tls
      ? createHttpsServer(buildTlsOptions(cfg.tls), requestHandler)
      : createHttpServer(requestHandler);

  // Expose at module scope so shutdown() can close the server
  _httpServer = httpServer as HttpServer;

  // Track active TCP connections for max-connection enforcement.
  // Increment on new socket, decrement when that socket closes.
  httpServer.on("connection", (socket) => {
    _activeConnections += 1;
    socket.once("close", () => {
      _activeConnections -= 1;
    });
  });

  await new Promise<void>((resolve, reject) => {
    httpServer.listen(cfg.port, cfg.host, () => {
      const protocol = cfg.tls ? "https" : "http";
      console.error(
        `[ralphx-external-mcp] Server listening on ${protocol}://${cfg.host}:${cfg.port}`
      );
      console.error(
        `[ralphx-external-mcp] MCP endpoint: ${protocol}://${cfg.host}:${cfg.port}/mcp`
      );
      resolve();
    });
    httpServer.once("error", reject);
  });
}

/**
 * Returns the canonical client IP from the TCP socket.
 * X-Forwarded-For is intentionally ignored: without a validated trusted-proxy
 * list, a client could spoof any IP and bypass the auth-failure throttle.
 */
function getClientIp(req: IncomingMessage): string {
  return req.socket.remoteAddress ?? "unknown";
}

function readJsonBody(req: IncomingMessage): Promise<Record<string, unknown> | null> {
  return new Promise((resolve) => {
    let data = "";
    req.on("data", (chunk: unknown) => {
      data += String(chunk);
    });
    req.on("end", () => {
      try {
        resolve(JSON.parse(data) as Record<string, unknown>);
      } catch {
        resolve(null);
      }
    });
    req.on("error", () => resolve(null));
  });
}

/**
 * Read the raw request body as a string, distinguishing stream errors from empty/parse outcomes.
 * Unlike readJsonBody, this preserves the error distinction needed for MCP body pre-parsing.
 */
export function readBodyString(
  req: IncomingMessage
): Promise<{ ok: true; body: string } | { ok: false }> {
  return new Promise((resolve) => {
    let data = "";
    req.on("data", (chunk: unknown) => {
      data += String(chunk);
    });
    req.on("end", () => {
      resolve({ ok: true, body: data });
    });
    req.on("error", () => resolve({ ok: false }));
  });
}

/** Send a JSON-RPC 2.0 parse error response (code -32700) with HTTP 400. */
function sendJsonRpcParseError(res: ServerResponse): void {
  const body = JSON.stringify({
    jsonrpc: "2.0",
    id: null,
    error: {
      code: -32700,
      message: "Parse error",
    },
  });
  res.writeHead(400, {
    "Content-Type": "application/json",
    "Content-Length": Buffer.byteLength(body),
  });
  res.end(body);
}

function sendJsonRpcServerError(
  res: ServerResponse,
  id: string | number | null,
  message: string
): void {
  const body = JSON.stringify({
    jsonrpc: "2.0",
    id,
    error: {
      code: -32000,
      message,
    },
  });
  res.writeHead(400, {
    "Content-Type": "application/json",
    "Content-Length": Buffer.byteLength(body),
  });
  res.end(body);
}

function isInitializeRequest(body: unknown): boolean {
  if (Array.isArray(body)) {
    return body.some(isInitializeRequest);
  }
  if (!body || typeof body !== "object") {
    return false;
  }
  return (body as { method?: unknown }).method === "initialize";
}

function jsonRpcIdFromBody(body: unknown): string | number | null {
  if (!body || typeof body !== "object" || Array.isArray(body)) {
    return null;
  }
  const id = (body as { id?: unknown }).id;
  return typeof id === "string" || typeof id === "number" ? id : null;
}

async function handleInvalidateCache(
  req: IncomingMessage,
  res: ServerResponse
): Promise<void> {
  const body = await readJsonBody(req);
  const keyId = body?.key_id;
  if (!keyId || typeof keyId !== "string") {
    sendError(res, 400, "Missing or invalid key_id in request body");
    return;
  }
  invalidateCacheByKeyId(keyId);
  const responseBody = JSON.stringify({ ok: true });
  res.writeHead(200, {
    "Content-Type": "application/json",
    "Content-Length": responseBody.length,
  });
  res.end(responseBody);
}

function sendError(res: ServerResponse, status: number, message: string): void {
  const body = JSON.stringify({ error: message });
  res.writeHead(status, {
    "Content-Type": "application/json",
    "Content-Length": Buffer.byteLength(body),
  });
  res.end(body);
}

function isMainEntry(scriptPath: string): boolean {
  try {
    return (
      realpathSync(fileURLToPath(import.meta.url)) === realpathSync(scriptPath)
    );
  } catch {
    return false;
  }
}

// Entry point — parse config from environment and start.
// Resolve both sides through realpathSync so the check survives symlinks
// (the app launches this from a symlinked plugin dir under ~/Library/Application Support,
// which makes import.meta.url point at the repo realpath while process.argv[1]
// keeps the symlinked path).
if (process.argv[1] && isMainEntry(process.argv[1])) {
  const port = process.env.EXTERNAL_MCP_PORT
    ? parseInt(process.env.EXTERNAL_MCP_PORT, 10)
    : 3848;
  const host = process.env.EXTERNAL_MCP_HOST ?? "127.0.0.1";
  const backendUrl =
    process.env.RALPHX_BACKEND_URL ?? "http://127.0.0.1:3847";

  const tlsCert = process.env.EXTERNAL_MCP_TLS_CERT;
  const tlsKey = process.env.EXTERNAL_MCP_TLS_KEY;
  const tls =
    tlsCert && tlsKey ? { certPath: tlsCert, keyPath: tlsKey } : undefined;

  startServer({ port, host, backendUrl, tls }).catch((err) => {
    console.error("[ralphx-external-mcp] Fatal startup error:", err);
    process.exit(1);
  });
}
