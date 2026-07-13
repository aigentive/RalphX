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
const TRACE_DISABLED = "(disabled)";
const PERSONA_LOG_REDACTION = "***PERSONA_ARGS_REDACTED***";

export const ARG_REDACTED_TOOLS = new Set([
  "save_persona_draft",
  "get_persona_draft",
]);

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

export function redactToolArgsForLog(toolName: string, args: unknown): unknown {
  return isPersonaTool(toolName) ? PERSONA_LOG_REDACTION : args;
}

export function redactToolResultForLog(toolName: string, result: unknown): unknown {
  return isPersonaTool(toolName) ? PERSONA_LOG_REDACTION : result;
}

function isPersonaTool(toolName: string): boolean {
  return ARG_REDACTED_TOOLS.has(toolName) || toolName.startsWith("persona_");
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
let traceLogDisabled = false;
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

function resolveModuleTraceDir(): string {
  const moduleDir = path.dirname(fileURLToPath(import.meta.url));
  return path.resolve(moduleDir, "../../../../.artifacts/logs", TRACE_SUBDIR);
}

function resolveFallbackTraceDir(): string {
  const fallbackRoot = process.platform === "win32" ? "C:\\Windows\\Temp" : "/tmp";
  return path.join(fallbackRoot, "ralphx-mcp-proxy-traces");
}

function isPathInside(childPath: string, parentPath: string): boolean {
  const relative = path.relative(parentPath, childPath);
  return relative.length > 0 && !relative.startsWith("..") && !path.isAbsolute(relative);
}

function isTargetProjectPath(resolvedPath: string): boolean {
  const workingDirectory = process.env.RALPHX_WORKING_DIRECTORY;
  if (!workingDirectory || !path.isAbsolute(workingDirectory)) {
    return false;
  }

  const resolvedWorkingDirectory = path.resolve(workingDirectory);
  return (
    resolvedPath === resolvedWorkingDirectory ||
    isPathInside(resolvedPath, resolvedWorkingDirectory)
  );
}

function resolveSafeTraceDir(traceDir: string): string | null {
  if (!path.isAbsolute(traceDir)) {
    return null;
  }

  const resolvedTraceDir = path.resolve(traceDir);
  if (path.basename(resolvedTraceDir) !== TRACE_SUBDIR) {
    safeError("[RalphX MCP] Ignoring trace dir with unexpected leaf directory");
    return null;
  }

  if (isTargetProjectPath(resolvedTraceDir)) {
    safeError("[RalphX MCP] Ignoring trace dir inside target working directory");
    return null;
  }

  return resolvedTraceDir;
}

function resolveConfiguredTraceDir(): string | null {
  const configuredTraceDir = process.env.RALPHX_MCP_TRACE_DIR;
  if (!configuredTraceDir) {
    return null;
  }

  return resolveSafeTraceDir(configuredTraceDir);
}

function buildTraceLogPathInExistingDir(traceDir: string): string | null {
  try {
    const safeTraceDir = resolveSafeTraceDir(traceDir);
    if (!safeTraceDir) {
      return null;
    }

    // safeTraceDir passed the same fixed-leaf and target-project containment checks
    // before the realpath lookup.
    // codeql[js/path-injection]
    const realTraceDir = fs.realpathSync.native(safeTraceDir);
    if (isTargetProjectPath(realTraceDir)) {
      safeError("[RalphX MCP] Ignoring trace dir symlinked into target working directory");
      return null;
    }

    return path.join(realTraceDir, buildTraceFilename());
  } catch (error) {
    safeError("[RalphX MCP] Failed to use existing MCP trace dir:", error);
    return null;
  }
}

function buildTraceLogPathInOwnedDir(traceDir: string): string | null {
  try {
    const safeTraceDir = resolveSafeTraceDir(traceDir);
    if (!safeTraceDir) {
      return null;
    }

    // safeTraceDir is one of this module's fixed RalphX-owned fallback roots,
    // not a configured runtime path from CLI/env.
    // codeql[js/path-injection]
    fs.mkdirSync(safeTraceDir, { recursive: true });

    return buildTraceLogPathInExistingDir(safeTraceDir);
  } catch (error) {
    safeError("[RalphX MCP] Failed to initialize MCP trace dir:", error);
    return null;
  }
}

function resolveTraceLogPath(): string | null {
  const configuredTraceDir = resolveConfiguredTraceDir();
  if (configuredTraceDir) {
    const configuredCandidate = buildTraceLogPathInExistingDir(configuredTraceDir);
    if (configuredCandidate) {
      return configuredCandidate;
    }
  }

  const candidateDirs = [resolveModuleTraceDir(), resolveFallbackTraceDir()];
  for (const traceDir of candidateDirs) {
    const candidate = buildTraceLogPathInOwnedDir(traceDir);
    if (candidate) {
      return candidate;
    }
  }

  safeError("[RalphX MCP] MCP trace logging disabled: no writable trace dir");
  return null;
}

export function getTraceLogPath(): string {
  if (traceLogPath) {
    return traceLogPath;
  }
  if (traceLogDisabled) {
    return TRACE_DISABLED;
  }

  const resolvedPath = resolveTraceLogPath();
  if (!resolvedPath) {
    traceLogDisabled = true;
    return TRACE_DISABLED;
  }
  traceLogPath = resolvedPath;
  return traceLogPath;
}

export function resetTraceLogPathForTests(): void {
  traceLogPath = null;
  traceLogDisabled = false;
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
  const logPath = getTraceLogPath();
  if (logPath === TRACE_DISABLED) {
    return;
  }

  const record: TraceRecord = {
    ts: new Date().toISOString(),
    pid: process.pid,
    event: normalizeTraceEvent(event),
  }

  try {
    // logPath is produced only by buildTraceLogPathInDir, which creates and
    // realpath-validates the fixed mcp-proxy trace directory first.
    // codeql[js/path-injection]
    fs.appendFileSync(logPath, `${JSON.stringify(record)}\n`, "utf8");
  } catch (error) {
    safeError("[RalphX MCP] Failed to append MCP trace log:", error);
    traceLogPath = null;
    traceLogDisabled = true;
  }
}
