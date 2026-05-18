#!/usr/bin/env node
/**
 * ralphx-external-mcp — External MCP Server for RalphX Orchestration API
 *
 * Exposes 27 orchestration-level tools to external agents (reefbot.ai, etc.) via
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
import "./keep-alive.js";
import type { Server as HttpServer } from "node:http";
import type { IncomingMessage } from "node:http";
import type { ExternalMcpConfig } from "./types.js";
/** Returns the current number of active TCP connections (for testing). */
export declare function getActiveConnections(): number;
/** Reset connection counter — used in tests to clean up between runs. */
export declare function resetActiveConnections(): void;
/** Returns the current HTTP server handle — for testing shutdown behavior. */
export declare function getHttpServer(): HttpServer | undefined;
/**
 * Gracefully shut down the server: stop accepting connections, close all
 * active MCP transports, then exit. A 1-second force-exit timer ensures we
 * stay within the Rust supervisor's 2-second SIGTERM window.
 */
export declare function shutdown(): Promise<void>;
/**
 * Start the external MCP server.
 */
export declare function startServer(config?: Partial<ExternalMcpConfig>): Promise<void>;
/**
 * Read the raw request body as a string, distinguishing stream errors from empty/parse outcomes.
 * Unlike readJsonBody, this preserves the error distinction needed for MCP body pre-parsing.
 */
export declare function readBodyString(req: IncomingMessage): Promise<{
    ok: true;
    body: string;
} | {
    ok: false;
}>;
//# sourceMappingURL=index.d.ts.map