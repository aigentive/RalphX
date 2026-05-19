#!/usr/bin/env node
/**
 * RalphX MCP Server
 *
 * A proxy MCP server that forwards tool calls to the RalphX Tauri backend via HTTP.
 * All business logic lives in Rust - this server is a thin transport layer.
 *
 * Tool scoping:
 * - Reads agent type from CLI args (--agent-type=<type>) or environment (RALPHX_AGENT_TYPE)
 * - CLI args take precedence (because Claude CLI doesn't pass env vars to MCP servers)
 * - Filters available tools based on agent type (hard enforcement)
 * - Each agent only sees tools appropriate for its role
 */
import "./keep-alive.js";
/**
 * Semantic keyword patterns for cross-project detection in plan text.
 * Exported for unit testing.
 */
export declare const CROSS_PROJECT_KEYWORDS: string[];
/**
 * Strip fenced and inline markdown code blocks from text before path scanning.
 * Prevents false-positive path detection on code snippets like `...>>` or `...`.
 * Exported for unit testing.
 */
export declare function stripMarkdownCodeBlocks(text: string): string;
/**
 * Filter out detected paths that belong to the same project root.
 * Returns only paths that genuinely reference a different project.
 *
 * @param detectedPaths - Raw list of absolute or relative paths found in plan text
 * @param projectWorkingDir - The project's working directory (e.g. /Users/alice/Code/ralphx)
 * @returns Paths that do NOT start with projectWorkingDir (i.e. are truly cross-project)
 */
export declare function filterCrossProjectPaths(detectedPaths: string[], projectWorkingDir: string | null): string[];
//# sourceMappingURL=index.d.ts.map