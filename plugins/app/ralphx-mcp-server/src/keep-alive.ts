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

import { Agent, setGlobalDispatcher } from "undici";

const KEEP_ALIVE_AGENT = new Agent({
  // Close idle sockets after 30s of inactivity. Long enough to cover bursts
  // of agent tool calls; short enough that nothing stale lingers in the pool.
  keepAliveTimeout: 30_000,
  // Hard ceiling on how long a single keep-alive connection can live before
  // being recycled. Protects against pathological cases where a single
  // connection somehow stays "warm" forever.
  keepAliveMaxTimeout: 600_000,
  // Max concurrent connections per origin. The MCP server only talks to one
  // backend, so 16 is plenty even under heavy parallel tool use.
  connections: 16,
});

setGlobalDispatcher(KEEP_ALIVE_AGENT);
