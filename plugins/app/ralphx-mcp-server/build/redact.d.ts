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
export declare const ARG_REDACTED_TOOLS: Set<string>;
/**
 * Apply all redaction patterns to a string.
 * Non-secret strings pass through unchanged.
 */
export declare function redactSecrets(input: string): string;
export declare function redactToolArgsForLog(toolName: string, args: unknown): unknown;
export declare function redactToolResultForLog(toolName: string, result: unknown): unknown;
/**
 * Safe drop-in replacement for console.error that redacts secrets from all arguments.
 * Use this instead of console.error wherever variable data (errors, objects, env values) is logged.
 *
 * Usage: safeError("[RalphX MCP] Error calling", name, error)
 */
export declare function safeError(...args: unknown[]): void;
export declare function getTraceLogPath(): string;
export declare function resetTraceLogPathForTests(): void;
export declare function safeTrace(event: string, _payload?: unknown): void;
//# sourceMappingURL=redact.d.ts.map