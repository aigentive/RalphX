/**
 * Secret redaction for MCP server logs.
 *
 * Mirrors the Rust secret_redactor patterns as JS regexps.
 * Apply to all console.error calls that log variable data to prevent
 * API keys, tokens, and bearer credentials from appearing in server logs.
 *
 * Pattern application order matters: more-specific prefixes (sk-ant-, sk-or-v1-)
 * MUST match before the generic sk- catch-all to prevent double-redaction.
 */

import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const TRACE_SUBDIR = "mcp-proxy";

interface RedactPattern {
  regex: RegExp;
  replacement: string;
}

/**
 * Ordered list of secret patterns with their replacements.
 * Patterns are applied in order — specific before generic.
 */
const PATTERNS: RedactPattern[] = [
  // 1. Anthropic API keys (most specific sk- variant first)
  { regex: /sk-ant-[a-zA-Z0-9_-]{20,}/g, replacement: "sk-ant-***REDACTED***" },
  // 2. OpenRouter keys
  { regex: /sk-or-v1-[a-zA-Z0-9]{20,}/g, replacement: "sk-or-v1-***REDACTED***" },
  // 3. RalphX API keys
  { regex: /rxk_live_[a-zA-Z0-9]{20,}/g, replacement: "rxk_live_***REDACTED***" },
  // 4. Generic OpenAI-style keys (catch-all after specific sk- patterns)
  { regex: /sk-[a-zA-Z0-9]{20,}/g, replacement: "sk-***REDACTED***" },
  // 5. Bearer tokens
  { regex: /Bearer [a-zA-Z0-9_.+-]{20,}/g, replacement: "Bearer ***REDACTED***" },
  // 6. ANTHROPIC_AUTH_TOKEN in JSON
  { regex: /"ANTHROPIC_AUTH_TOKEN"\s*:\s*"[^"]+"/g, replacement: '"ANTHROPIC_AUTH_TOKEN":"***REDACTED***"' },
  // 7. ANTHROPIC_API_KEY in JSON
  { regex: /"ANTHROPIC_API_KEY"\s*:\s*"[^"]+"/g, replacement: '"ANTHROPIC_API_KEY":"***REDACTED***"' },
  // 8. GitHub PATs
  { regex: /ghp_[a-zA-Z0-9]{20,}/g, replacement: "ghp_***REDACTED***" },
  // 9. GitHub OAuth tokens
  { regex: /gho_[a-zA-Z0-9]{20,}/g, replacement: "gho_***REDACTED***" },
  // 10. Generic env var with sensitive name in JSON ("MY_API_KEY": "value")
  { regex: /"([A-Z_0-9]*(?:KEY|TOKEN|SECRET|PASSWORD|CREDENTIAL)[A-Z_0-9]*)"\s*:\s*"[^"]+"/gi, replacement: '"$1":"***REDACTED***"' },
  // 11. Generic env var with sensitive name in assignment (MY_API_KEY=value)
  { regex: /\b([A-Z_0-9]*(?:KEY|TOKEN|SECRET|PASSWORD|CREDENTIAL)[A-Z_0-9]*)=("[^"]*"|\S+)/gi, replacement: "$1=***REDACTED***" },
];

/**
 * Apply all redaction patterns to a string.
 * Non-secret strings pass through unchanged.
 */
export function redactSecrets(input: string): string {
  let result = input;
  for (const { regex, replacement } of PATTERNS) {
    regex.lastIndex = 0; // reset stateful regex
    result = result.replace(regex, replacement);
  }
  return result;
}

/**
 * Stringify an unknown value for redaction.
 * Objects are JSON-serialized; primitives are coerced to string.
 */
function stringify(arg: unknown): string {
  if (typeof arg === "string") return arg;
  if (arg instanceof Error) return arg.stack ?? arg.message;
  try {
    return JSON.stringify(arg) ?? String(arg);
  } catch {
    return String(arg);
  }
}

/**
 * Safe drop-in replacement for console.error that redacts secrets from all arguments.
 * Use this instead of console.error wherever variable data (errors, objects, env values) is logged.
 *
 * Usage: safeError("[RalphX MCP] Error calling", name, error)
 */
export function safeError(...args: unknown[]): void {
  const redacted = args.map((arg) => redactSecrets(stringify(arg)));
  console.error(...redacted);
}

let traceLogPath: string | null = null;
const SAFE_TRACE_EVENTS = new Set([
  "backend.error",
  "backend.request",
  "backend.response",
  "server.ready",
  "server.start",
  "tool.denied",
  "tool.dispatch",
  "tool.error",
  "tool.request",
  "tool.success",
  "tools.list",
]);

function buildTraceFilename(): string {
  const timestamp = new Date().toISOString().replace(/[:.]/g, "-");
  return `${timestamp}-${process.pid}.jsonl`;
}

function resolveTraceDir(): string {
  const moduleDir = path.dirname(fileURLToPath(import.meta.url));
  return path.resolve(moduleDir, "../../../../.artifacts/logs", TRACE_SUBDIR);
}

export function getTraceLogPath(): string {
  if (traceLogPath) {
    return traceLogPath;
  }

  const traceDir = resolveTraceDir();
  fs.mkdirSync(traceDir, { recursive: true });
  traceLogPath = path.join(traceDir, buildTraceFilename());
  return traceLogPath;
}

export function resetTraceLogPathForTests(): void {
  traceLogPath = null;
}

type TraceRecord = {
  ts: string;
  pid: number;
  event: string;
};

function normalizeTraceEvent(event: string): string {
  return SAFE_TRACE_EVENTS.has(event) ? event : "unknown";
}

export function safeTrace(event: string, _payload?: unknown): void {
  const record: TraceRecord = {
    ts: new Date().toISOString(),
    pid: process.pid,
    event: normalizeTraceEvent(event),
  }

  try {
    fs.appendFileSync(getTraceLogPath(), `${JSON.stringify(record)}\n`, "utf8");
  } catch (error) {
    safeError("[RalphX MCP] Failed to append MCP trace log:", error);
  }
}
