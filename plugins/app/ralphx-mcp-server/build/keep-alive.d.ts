/**
 * Enable HTTP keep-alive on the global fetch() dispatcher.
 *
 * Every MCP tool call hits the Tauri backend on 127.0.0.1:3847 over HTTP.
 * Without keep-alive each call opens a fresh TCP socket, sends one request,
 * and closes — producing one TIME_WAIT entry per call. On a busy agent day
 * that's tens of thousands of short-lived sockets, all eating ephemeral
 * ports from a finite pool (49152-65535 on macOS).
 *
 * Wired as a side-effect import from index.ts so it runs once at server
 * startup, before any tool handler can issue a fetch.
 *
 * Node's global fetch() uses undici internally and honors setGlobalDispatcher.
 */
export {};
//# sourceMappingURL=keep-alive.d.ts.map