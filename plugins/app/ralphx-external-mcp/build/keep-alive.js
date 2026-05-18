/**
 * Enable HTTP keep-alive on the global fetch() dispatcher.
 *
 * The external MCP server proxies external/Tauri-owned agent calls to the
 * Tauri backend on 127.0.0.1:3847. Without keep-alive each proxy hop opens
 * a fresh TCP socket, sends one request, and closes — producing one
 * TIME_WAIT entry per call. With many concurrent external agents the
 * resulting churn eats ephemeral ports (49152-65535 on macOS).
 *
 * Wired as a side-effect import from index.ts so it runs once at server
 * startup, before any backend call is issued.
 *
 * Node's global fetch() uses undici internally and honors setGlobalDispatcher.
 */
import { Agent, setGlobalDispatcher } from "undici";
const KEEP_ALIVE_AGENT = new Agent({
    // Close idle sockets after 30s of inactivity.
    keepAliveTimeout: 30_000,
    // Hard ceiling on how long a single keep-alive connection can live before
    // being recycled.
    keepAliveMaxTimeout: 600_000,
    // Max concurrent connections per origin. The external MCP only talks to
    // one backend, so 16 is plenty under heavy parallel external agent load.
    connections: 16,
});
setGlobalDispatcher(KEEP_ALIVE_AGENT);
//# sourceMappingURL=keep-alive.js.map