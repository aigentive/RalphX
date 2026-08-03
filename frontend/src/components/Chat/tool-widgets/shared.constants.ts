/**
 * Shared constants, types, and utilities for tool call widgets.
 *
 * Extracted from shared.tsx to keep component files under 500 LOC.
 * Design reference: mockups/tool-call-widgets.html
 */

import type { CSSProperties, ReactNode } from "react";
import type { FileDiff } from "@/api/diff";

// ============================================================================
// Tool Call Type (canonical definition — re-exported from ToolCallIndicator)
// ============================================================================

/** Structured stats for Task/Agent tool calls — populated from backend at TaskCompleted time. camelCase (matches Rust #[serde(rename_all = "camelCase")]). */
export interface ToolCallStats {
  model?: string;
  totalTokens?: number;
  totalToolUses?: number;
  durationMs?: number;
}

export interface ToolCallDetailRef {
  conversationId: string;
  messageId: string;
  toolCallId?: string | null;
  contentBlockIndex?: number | null;
  timelineItemId?: string | null;
}

/**
 * Tool call structure from Claude CLI stream-json output
 */
export interface ToolCall {
  /** Unique identifier for this tool call */
  id: string;
  /** Name of the tool that was called */
  name: string;
  /** Arguments passed to the tool (can be any JSON value) */
  arguments: unknown;
  /** Backend logical content-block position, present on recovered snapshots only. */
  blockIndex?: number;
  /** Result returned from the tool (can be any JSON value) */
  result?: unknown;
  /** True when result contains only a lightweight preview and full output must be lazy-loaded */
  resultPreviewTruncated?: boolean;
  /** Original result byte size when previewed */
  resultPreviewOriginalBytes?: number;
  /** Original result line count when previewed */
  resultPreviewLineCount?: number;
  /** Number of lines omitted from the preview */
  resultPreviewOmittedLines?: number;
  /** JSON paths that were structurally trimmed in the preview payload */
  resultPreviewPaths?: string[];
  /** True when arguments contain only the lightweight data needed for a collapsed preview */
  argumentsPreviewTruncated?: boolean;
  /** Original argument byte size when previewed */
  argumentsPreviewOriginalBytes?: number;
  /** Original argument line count when previewed */
  argumentsPreviewLineCount?: number;
  /** Number of argument lines omitted from the preview */
  argumentsPreviewOmittedLines?: number;
  /** Backend-computed lightweight diff for previewed Edit/Write arguments */
  diffPreview?: FileDiff;
  /** Backend locator for lazily loading a full previewed result */
  detailRef?: ToolCallDetailRef;
  /** Parent tool_use id when this call belongs to a delegated/child task transcript */
  parentToolUseId?: string;
  /** Error message if tool call failed */
  error?: string;
  /** Diff context for Edit/Write tool calls (old file content for computing diffs) */
  diffContext?: {
    oldContent?: string;
    oldFileExists?: boolean;
    filePath: string;
  };
  /** Structured stats for Task/Agent tool calls — absent for old DB rows and non-Task calls */
  stats?: ToolCallStats;
}

// ============================================================================
// CSS Constants (shared across all widgets)
// ============================================================================

export const COLLAPSED_HEIGHT = 73;
export const COLLAPSED_HEIGHT_COMPACT = 52;
export const GRADIENT_HEIGHT = 36;
export const TRANSITION_SPEED = "200ms";

export const colors = {
  bgBase: "var(--bg-base)",
  bgSurface: "var(--bg-surface)",
  bgElevated: "var(--bg-elevated)",
  bgHover: "var(--bg-hover)",
  bgTerminal: "var(--bg-surface)",
  textPrimary: "var(--text-primary)",
  textSecondary: "var(--text-secondary)",
  textMuted: "var(--text-muted)",
  accent: "var(--accent-primary)",
  accentDim: "var(--accent-muted)",
  accentBorder: "var(--accent-border)",
  success: "var(--status-success)",
  successDim: "var(--status-success-muted)",
  error: "var(--status-error)",
  errorDim: "var(--status-error-muted)",
  blue: "var(--status-info)",
  blueDim: "var(--status-info-muted)",
  border: "var(--bg-hover)",
  borderSubtle: "var(--border-subtle)",
} as const;

// ============================================================================
// Badge Variant Type
// ============================================================================

export type BadgeVariant = "muted" | "success" | "accent" | "error" | "blue" | "warning";

export const badgeStyles: Record<BadgeVariant, { bg: string; color: string }> = {
  muted: { bg: colors.border, color: colors.textMuted },
  success: { bg: colors.successDim, color: colors.success },
  accent: { bg: colors.accentDim, color: colors.accent },
  error: { bg: colors.errorDim, color: colors.error },
  blue: { bg: colors.blueDim, color: colors.blue },
  warning: { bg: "var(--status-warning-muted)", color: "var(--status-warning)" },
};

// ============================================================================
// MCP Tool Result Parser
// ============================================================================

/** Return type for parsed MCP tool results */
export interface ParsedMcpResult {
  [key: string]: unknown;
}

function hasRecordField(obj: unknown, key: string): obj is Record<string, unknown> {
  return obj != null && typeof obj === "object" && key in obj;
}

function parseMcpTextContentArray(result: unknown): unknown {
  if (!Array.isArray(result) || result.length === 0) {
    return null;
  }

  const first = result[0];
  if (first?.type === "text" && typeof first.text === "string") {
    try {
      return JSON.parse(first.text);
    } catch {
      return null;
    }
  }

  // Plain array (not MCP wrapper) — return as-is
  return result;
}

function extractMcpTextContent(result: unknown): string | null {
  if (!Array.isArray(result) || result.length === 0) {
    return null;
  }

  const texts: string[] = [];
  for (const item of result) {
    if (
      item &&
      typeof item === "object" &&
      "text" in item &&
      typeof (item as { text: unknown }).text === "string"
    ) {
      texts.push((item as { text: string }).text);
    }
  }

  return texts.length > 0 ? texts.join("\n") : null;
}

/**
 * Unwrap MCP content array to raw parsed value (object OR array).
 * Unlike parseMcpToolResult, returns `unknown` to allow Array.isArray narrowing.
 * Use for list tools where the inner JSON may be an array.
 */
export function parseMcpToolResultRaw(result: unknown): unknown {
  // Plain string — JSON parse it
  if (typeof result === "string") {
    try {
      return JSON.parse(result);
    } catch {
      return null;
    }
  }
  if (result != null && typeof result === "object" && !Array.isArray(result)) {
    const structuredContent =
      (result as Record<string, unknown>).structured_content
      ?? (result as Record<string, unknown>).structuredContent;
    if (structuredContent != null) {
      const parsedStructured = parseMcpToolResultRaw(structuredContent);
      if (parsedStructured != null) {
        return parsedStructured;
      }
    }

    const parsedWrappedContent = parseMcpTextContentArray(
      (result as Record<string, unknown>).content
    );
    if (parsedWrappedContent != null) {
      return parsedWrappedContent;
    }

    // Already a plain object (non-array)
    return result;
  }
  // MCP content array: [{type:"text", text:"..."}]
  if (Array.isArray(result)) {
    return parseMcpTextContentArray(result);
  }
  return null;
}

/**
 * Unwrap MCP content array to parsed JSON object.
 * Handles: [{type:"text", text:"{...}"}] → parsed object
 * Handles: plain string (JSON) → parsed object
 * Passthrough: already-plain objects returned as-is
 * Fallback: returns empty object on parse errors
 */
export function parseMcpToolResult(result: unknown): ParsedMcpResult {
  const raw = parseMcpToolResultRaw(result);
  if (raw != null && typeof raw === "object" && !Array.isArray(raw)) {
    return raw as ParsedMcpResult;
  }
  return {};
}

// ============================================================================
// Safe extraction helpers (used by all widgets to pull typed fields from unknown args/results)
// ============================================================================

/** Safely extract a string field from an unknown object */
export function getString(obj: unknown, key: string): string | undefined {
  if (obj != null && typeof obj === "object" && key in (obj as Record<string, unknown>)) {
    const val = (obj as Record<string, unknown>)[key];
    return typeof val === "string" ? val : undefined;
  }
  return undefined;
}

/** Safely extract a number field from an unknown object */
export function getNumber(obj: unknown, key: string): number | undefined {
  if (obj != null && typeof obj === "object" && key in (obj as Record<string, unknown>)) {
    const val = (obj as Record<string, unknown>)[key];
    return typeof val === "number" ? val : undefined;
  }
  return undefined;
}

/** Safely extract a string array field from an unknown object */
export function getStringArray(obj: unknown, key: string): string[] | undefined {
  if (obj != null && typeof obj === "object" && key in (obj as Record<string, unknown>)) {
    const val = (obj as Record<string, unknown>)[key];
    if (Array.isArray(val) && val.every((v) => typeof v === "string")) return val;
  }
  return undefined;
}

/** Safely extract a boolean field from an unknown object */
export function getBool(obj: unknown, key: string): boolean | undefined {
  if (obj != null && typeof obj === "object" && key in (obj as Record<string, unknown>)) {
    const val = (obj as Record<string, unknown>)[key];
    return typeof val === "boolean" ? val : undefined;
  }
  return undefined;
}

/** Safely extract an array field from an unknown object */
export function getArray(obj: unknown, key: string): unknown[] | undefined {
  if (obj != null && typeof obj === "object" && key in (obj as Record<string, unknown>)) {
    const val = (obj as Record<string, unknown>)[key];
    return Array.isArray(val) ? val : undefined;
  }
  return undefined;
}

// ============================================================================
// Utility Functions
// ============================================================================

/** Truncate text to maxLen characters, appending "…" if truncated. */
export function truncate(text: string, maxLen: number): string {
  if (text.length <= maxLen) return text;
  return text.slice(0, maxLen) + "…";
}


/**
 * Parse a tool result into an array of non-empty lines.
 * Handles: plain string, MCP wrapper [{text: "..."}], object with text property, and string arrays.
 */
export function parseToolResultAsLines(result: unknown): string[] {
  if (!result) return [];

  let text = "";

  if (typeof result === "string") {
    text = result;
  } else if (Array.isArray(result)) {
    // MCP result wrapper: [{type: "text", text: "..."}]
    const mcpText = extractMcpTextContent(result);
    if (mcpText != null) {
      text = mcpText;
    } else {
      // Array of strings
      return result.filter((item): item is string => typeof item === "string");
    }
  } else if (typeof result === "object") {
    if (hasRecordField(result, "text")) {
      text = String(result.text);
    } else if (hasRecordField(result, "content")) {
      const content = result.content;
      if (typeof content === "string") {
        text = content;
      } else {
        text = extractMcpTextContent(content) ?? "";
      }
    } else {
      text = "";
    }
  }

  if (!text) return [];

  return text
    .split("\n")
    .map((line) => line.trim())
    .filter(Boolean);
}

/**
 * Shorten a file path by collapsing middle directories.
 * Works on normalized paths only (never produces /.../...).
 */
export function shortenPath(path: string, maxLength: number): string {
  if (path.length <= maxLength) return path;

  const parts = path.split("/");
  if (parts.length <= 2) return path;

  // Keep first directory and last 2 segments
  const first = parts[0] || "";
  const last2 = parts.slice(-2).join("/");
  const shortened = `${first}/${last2}`;

  if (shortened.length <= maxLength) return shortened;

  // Last resort: just show filename
  const filename = parts[parts.length - 1] || "";
  return filename;
}

// ============================================================================
// Path Normalization
// ============================================================================

/** Known project-root directory anchors */
const PROJECT_ANCHORS = [
  "src-tauri",
  "src",
  "tests",
  "specs",
  "scripts",
  "docs",
  "mockups",
  "assets",
  "public",
  "plugins",
  ".claude",
];

/**
 * Normalize an absolute or messy path to repo-relative display.
 * - Converts backslashes to forward slashes.
 * - Removes leading absolute prefix by anchoring from first known project segment.
 * - Removes leading `.../` or `/.../` artifacts.
 * - Falls back to basename when no anchor exists.
 */
export function normalizeDisplayPath(path: string): string {
  if (!path) return path;

  // Normalize separators
  let normalized = path.replace(/\\/g, "/");

  // Remove leading/.../artifacts like /.../
  normalized = normalized.replace(/^\/?\.\.\.\//, "");

  // If already relative (no leading /), return as-is unless it still has absolute segments
  if (!normalized.startsWith("/")) return normalized;

  // Find earliest known project anchor in the path segments
  const segments = normalized.split("/");
  for (let i = 0; i < segments.length; i++) {
    const seg = segments[i];
    if (seg && PROJECT_ANCHORS.includes(seg)) {
      return segments.slice(i).join("/");
    }
  }

  // Fallback: strip common workspace prefixes (/Users/.../Code/project/)
  // Just take from last known path segment
  const filename = segments[segments.length - 1];
  return filename || normalized;
}

// ============================================================================
// Search Result Parser
// ============================================================================

export interface ParsedSearchResult {
  paths: string[];
  isEmpty: boolean;
  note?: string;
}

/** Metadata/header lines to skip in search results */
const SEARCH_METADATA_RE = /^(Found \d+ files?|Showing|Page \d|Results? \d|ROOT:|PATTERN:|FILE_PATTERN:|BASE_PATH:|MATCHES:|INCLUDE_HIDDEN:|RESPECT_GITIGNORE:)/i;

/** Explicit no-result markers */
const SEARCH_EMPTY_RE = /^No (matches|files|results)( found| matched)?\.?$/i;

/** Explicit search error marker emitted by the fs MCP tools */
const SEARCH_ERROR_RE = /^ERROR:\s*(.+)$/i;

/**
 * Parse a search result (Grep/Glob) into deduplicated, normalized file paths.
 * Handles metadata lines, no-result lines, and `path:line:match` content lines.
 */
export function parseSearchResult(result: unknown): ParsedSearchResult {
  const lines = parseToolResultAsLines(result);

  if (lines.length === 0) {
    return { paths: [], isEmpty: true };
  }

  const pathSet = new Set<string>();
  let note: string | undefined;

  for (const line of lines) {
    // Skip metadata lines
    if (SEARCH_METADATA_RE.test(line)) continue;

    // Detect no-result markers
    if (SEARCH_EMPTY_RE.test(line)) {
      return { paths: [], isEmpty: true, note: line };
    }

    const errorMatch = line.match(SEARCH_ERROR_RE);
    if (errorMatch?.[1]) {
      return { paths: [], isEmpty: true, note: `ERROR: ${errorMatch[1].trim()}` };
    }

    // Extract path from `path:lineNum:content` (grep content mode)
    // or `path:lineNum` (grep count mode)
    // or plain path
    let filePath = line;
    const colonMatch = line.match(/^(.+?\.\w+):(\d+)/);
    if (colonMatch?.[1]) {
      filePath = colonMatch[1];
    }

    const normalized = normalizeDisplayPath(filePath.trim());
    if (normalized) {
      pathSet.add(normalized);
    }
  }

  const paths = Array.from(pathSet);
  const parsed: ParsedSearchResult = { paths, isEmpty: paths.length === 0 };
  if (note !== undefined) parsed.note = note;
  return parsed;
}

// ============================================================================
// Read Output Parser
// ============================================================================

export interface ParsedReadOutput {
  lines: string[];
  inferredStartLine: number;
  error?: string;
}

/** Match tool-added line-number prefixes like "   500→" or "     1→" */
const LINE_PREFIX_RE = /^\s*(\d+)(?:→|\||\t)(.*)$/;

/** Match XML error wrapper */
const TOOL_ERROR_RE = /<tool_use_error>([\s\S]*?)<\/tool_use_error>/;

/** Metadata/header lines emitted by fs read_file MCP results */
const READ_METADATA_RE = /^(FILE|LINES|TRUNCATED):/;

/** Explicit read error marker emitted by the fs MCP tools */
const READ_ERROR_RE = /^ERROR:\s*(.+)$/i;

/**
 * Parse raw Read tool output:
 * - Removes tool-added line prefixes (`   N→`).
 * - Preserves actual code indentation after the arrow/tab.
 * - Extracts `<tool_use_error>...</tool_use_error>` into clean error text.
 * - Infers start line from first prefix when offset is missing.
 */
export function parseReadOutput(
  result: unknown,
  offset?: number,
): ParsedReadOutput {
  const raw = extractRawText(result);

  if (!raw) {
    return { lines: [], inferredStartLine: offset ?? 1 };
  }

  // Check for error wrapper
  const errorMatch = raw.match(TOOL_ERROR_RE);
  if (errorMatch?.[1]) {
    return {
      lines: [],
      inferredStartLine: offset ?? 1,
      error: errorMatch[1].trim(),
    };
  }

  const readErrorMatch = raw.match(READ_ERROR_RE);
  if (readErrorMatch?.[1]) {
    return {
      lines: [],
      inferredStartLine: offset ?? 1,
      error: `ERROR: ${readErrorMatch[1].trim()}`,
    };
  }

  const rawLines = raw.split("\n");
  const parsedLines: string[] = [];
  let inferredStartLine = offset ?? 0;
  let firstPrefixSeen = false;

  for (const line of rawLines) {
    if (READ_METADATA_RE.test(line.trim())) {
      continue;
    }

    if (!firstPrefixSeen && line.trim() === "") {
      continue;
    }

    const prefixMatch = line.match(LINE_PREFIX_RE);
    if (prefixMatch) {
      if (!firstPrefixSeen) {
        firstPrefixSeen = true;
        if (!offset && prefixMatch[1]) {
          inferredStartLine = parseInt(prefixMatch[1], 10);
        }
      }
      parsedLines.push(prefixMatch[2] ?? "");
    } else {
      // No prefix — pass through as-is
      parsedLines.push(line);
    }
  }

  if (inferredStartLine === 0) inferredStartLine = 1;

  return { lines: parsedLines, inferredStartLine };
}

/** Extract raw text string from various result formats */
function extractRawText(result: unknown): string | null {
  if (typeof result === "string") return result;

  if (Array.isArray(result)) {
    const texts: string[] = [];
    for (const item of result) {
      if (
        item &&
        typeof item === "object" &&
        "text" in item &&
        typeof (item as { text: unknown }).text === "string"
      ) {
        texts.push((item as { text: string }).text);
      }
    }
    if (texts.length > 0) return texts.join("\n");
  }

  if (result && typeof result === "object") {
    const obj = result as Record<string, unknown>;
    if (typeof obj.text === "string") return obj.text;
    if (typeof obj.content === "string") return obj.content;
    const contentText = extractMcpTextContent(obj.content);
    if (contentText != null) return contentText;
  }

  return null;
}

// ============================================================================
// Widget Props (standard interface for all widget components)
// ============================================================================

export interface ToolCallWidgetProps {
  /** The tool call data */
  toolCall: ToolCall;
  /** Compact mode */
  compact?: boolean;
  /** Additional className */
  className?: string;
}

// ============================================================================
// Step Line Types
// ============================================================================

export type StepLineVariant = "started" | "completed" | "added" | "skipped" | "failed";

export interface StepLineConfig {
  color: string;
  label: string;
  badgeVariant: BadgeVariant;
}

export const stepVariantConfig: Record<StepLineVariant, StepLineConfig> = {
  started: { color: colors.accent, label: "started", badgeVariant: "muted" },
  completed: { color: colors.success, label: "completed", badgeVariant: "success" },
  added: { color: colors.blue, label: "added", badgeVariant: "blue" },
  skipped: { color: colors.textMuted, label: "skipped", badgeVariant: "muted" },
  failed: { color: colors.error, label: "failed", badgeVariant: "error" },
};

/** Inline style for truncated title text (flex item, ellipsis overflow). */
export function truncatedTitleStyle(compact = false): CSSProperties {
  return {
    flex: 1,
    fontSize: compact ? 10.5 : 11,
    color: colors.textSecondary,
    overflow: "hidden",
    textOverflow: "ellipsis",
    whiteSpace: "nowrap",
  };
}

// ============================================================================
// Component Prop Types (used by shared.tsx components)
// ============================================================================

export interface WidgetCardProps {
  /** Header content (passed to WidgetHeader or custom) */
  header: ReactNode;
  /** Body content (shown when expanded/collapsed with gradient) */
  children: ReactNode;
  /** Start expanded */
  defaultExpanded?: boolean;
  /** Compact mode for nested/subagent rendering */
  compact?: boolean;
  /** Additional className */
  className?: string;
  /** If true, body is always fully visible (no collapse) — for <=3 result items */
  alwaysExpanded?: boolean;
}

export interface WidgetHeaderProps {
  icon?: ReactNode;
  title: string;
  badge?: ReactNode;
  compact?: boolean;
  /** Use monospace font for title (file paths, patterns) */
  mono?: boolean;
}

export interface GradientFadeProps {
  visible: boolean;
  bgColor?: string;
}

export interface CodePreviewProps {
  /** Lines of code to display */
  lines: string[];
  /** Starting line number (default 1) */
  startLine?: number;
  compact?: boolean;
}

export interface InlineIndicatorProps {
  icon?: ReactNode;
  text: string;
}

export interface StepLineProps {
  variant: StepLineVariant;
  title: string;
  note?: string | undefined;
  compact?: boolean;
}

export interface FilePathProps {
  path: string;
  /** Max characters before shortening */
  maxLength?: number;
}

export interface BadgeProps {
  variant: BadgeVariant;
  children: ReactNode;
  compact?: boolean;
}

export interface WidgetRowProps {
  compact?: boolean | undefined;
  children: ReactNode;
}
