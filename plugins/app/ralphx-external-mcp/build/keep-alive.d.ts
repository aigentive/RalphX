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
export {};
//# sourceMappingURL=keep-alive.d.ts.map