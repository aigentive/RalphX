/**
 * DiffToolCallView.utils - Diff computation and line rendering helpers
 *
 * Extracted from SimpleDiffView.tsx for reuse in DiffToolCallView.
 * Provides LCS-based diff computation and tool-call-specific extractors.
 */

import type { ToolCall } from "./ToolCallIndicator";
import { canonicalizeToolName } from "./tool-widgets/tool-name";
import { withAlpha } from "@/lib/theme-colors";

// ============================================================================
// Types
// ============================================================================

export interface DiffLine {
  type: "context" | "addition" | "deletion" | "header";
  content: string;
  oldLineNum: number | null;
  newLineNum: number | null;
}

interface Match {
  oldIdx: number;
  newIdx: number;
}

export interface DiffResult {
  lines: DiffLine[];
  filePath: string;
  additions: number;
  deletions: number;
}

// ============================================================================
// Core Diff Algorithm
// ============================================================================

/**
 * Compute Longest Common Subsequence indices
 */
function computeLCS(oldLines: string[], newLines: string[]): Match[] {
  const m = oldLines.length;
  const n = newLines.length;

  const dp: number[][] = Array(m + 1)
    .fill(null)
    .map(() => Array(n + 1).fill(0));

  for (let i = 1; i <= m; i++) {
    for (let j = 1; j <= n; j++) {
      if (oldLines[i - 1] === newLines[j - 1]) {
        dp[i]![j] = (dp[i - 1]?.[j - 1] ?? 0) + 1;
      } else {
        dp[i]![j] = Math.max(dp[i - 1]?.[j] ?? 0, dp[i]?.[j - 1] ?? 0);
      }
    }
  }

  const matches: Match[] = [];
  let i = m;
  let j = n;

  while (i > 0 && j > 0) {
    if (oldLines[i - 1] === newLines[j - 1]) {
      matches.unshift({ oldIdx: i - 1, newIdx: j - 1 });
      i--;
      j--;
    } else if ((dp[i - 1]?.[j] ?? 0) > (dp[i]?.[j - 1] ?? 0)) {
      i--;
    } else {
      j--;
    }
  }

  return matches;
}

/**
 * Compute unified diff lines from old and new content strings
 */
export function computeDiff(oldContent: string, newContent: string): DiffLine[] {
  const oldLines = oldContent.split("\n");
  const newLines = newContent.split("\n");
  const result: DiffLine[] = [];

  const lcs = computeLCS(oldLines, newLines);

  let oldIdx = 0;
  let newIdx = 0;
  let oldLineNum = 1;
  let newLineNum = 1;

  for (const match of lcs) {
    while (oldIdx < match.oldIdx) {
      result.push({
        type: "deletion",
        content: oldLines[oldIdx] ?? "",
        oldLineNum: oldLineNum++,
        newLineNum: null,
      });
      oldIdx++;
    }

    while (newIdx < match.newIdx) {
      result.push({
        type: "addition",
        content: newLines[newIdx] ?? "",
        oldLineNum: null,
        newLineNum: newLineNum++,
      });
      newIdx++;
    }

    result.push({
      type: "context",
      content: oldLines[oldIdx] ?? "",
      oldLineNum: oldLineNum++,
      newLineNum: newLineNum++,
    });
    oldIdx++;
    newIdx++;
  }

  while (oldIdx < oldLines.length) {
    result.push({
      type: "deletion",
      content: oldLines[oldIdx] ?? "",
      oldLineNum: oldLineNum++,
      newLineNum: null,
    });
    oldIdx++;
  }

  while (newIdx < newLines.length) {
    result.push({
      type: "addition",
      content: newLines[newIdx] ?? "",
      oldLineNum: null,
      newLineNum: newLineNum++,
    });
    newIdx++;
  }

  return result;
}

// ============================================================================
// Line Rendering Helpers
// ============================================================================

export function getLineBackground(type: DiffLine["type"]): string {
  switch (type) {
    case "addition":
      return "var(--status-success-muted)";
    case "deletion":
      return "var(--status-error-muted)";
    default:
      return "transparent";
  }
}

export function getLineNumColor(type: DiffLine["type"]): string {
  switch (type) {
    case "addition":
      return withAlpha("var(--status-success)", 60);
    case "deletion":
      return withAlpha("var(--status-error)", 60);
    default:
      return "var(--text-muted)";
  }
}

export function getLinePrefix(type: DiffLine["type"]): string {
  switch (type) {
    case "addition":
      return "+";
    case "deletion":
      return "-";
    default:
      return " ";
  }
}

export function getPrefixColor(type: DiffLine["type"]): string {
  switch (type) {
    case "addition":
      return "var(--status-success)";
    case "deletion":
      return "var(--status-error)";
    case "header":
      return withAlpha("var(--text-primary)", 45);
    default:
      return "transparent";
  }
}

// ============================================================================
// Tool Call Extractors
// ============================================================================

/**
 * Check if a tool name is an Edit or Write tool call that should render as diff
 */
export function isDiffToolCall(name: string): boolean {
  const canonical = canonicalizeToolName(name);
  return canonical === "edit" || canonical === "write";
}

/**
 * Check if a tool name is a Task or Agent subagent tool call
 */
export function isTaskToolCall(name: string): boolean {
  const canonical = canonicalizeToolName(name);
  return canonical === "task" || canonical === "agent" || canonical === "delegate_start";
}

function normalizePathSeparators(path: string): string {
  return path.replace(/\\/g, "/");
}

function normalizePathForCompare(path: string): string {
  const normalized = normalizePathSeparators(path).replace(/\/+$/, "");
  return normalized || "/";
}

function isAbsolutePath(path: string): boolean {
  const normalized = normalizePathSeparators(path);
  return normalized.startsWith("/") || /^[A-Za-z]:\//.test(normalized);
}

export function getWorkspaceRelativeDiffPath(
  filePath: string,
  workspaceRootPath: string | null | undefined
): string | null {
  if (!filePath || !workspaceRootPath) return null;

  const normalizedFilePath = normalizePathForCompare(filePath);
  const normalizedRootPath = normalizePathForCompare(workspaceRootPath);

  if (!isAbsolutePath(normalizedFilePath) || !isAbsolutePath(normalizedRootPath)) {
    return null;
  }

  if (normalizedFilePath === normalizedRootPath) {
    return ".";
  }

  const rootPrefix = `${normalizedRootPath}/`;
  if (!normalizedFilePath.startsWith(rootPrefix)) {
    return null;
  }

  return normalizedFilePath.slice(rootPrefix.length);
}

export function getDiffFilePathDisplay(
  filePath: string,
  workspaceRootPath: string | null | undefined
): string {
  return getWorkspaceRelativeDiffPath(filePath, workspaceRootPath) ?? filePath;
}

/**
 * Extract diff data from an Edit tool call.
 * Edit arguments contain old_string and new_string.
 */
export function extractEditDiff(toolCall: ToolCall): DiffResult | null {
  const args = toolCall.arguments;
  if (!args || typeof args !== "object") return null;

  const a = args as Record<string, unknown>;
  const oldString = typeof a.old_string === "string" ? a.old_string : "";
  const newString = typeof a.new_string === "string" ? a.new_string : "";
  const filePath = typeof a.file_path === "string" ? a.file_path : "";

  if (!filePath) return null;

  const lines = computeDiff(oldString, newString);
  const additions = lines.filter((l) => l.type === "addition").length;
  const deletions = lines.filter((l) => l.type === "deletion").length;

  return { lines, filePath, additions, deletions };
}

/**
 * Extract diff data from a Write tool call.
 * Write arguments contain content and file_path.
 * If diffContext.oldContent exists, compute proper diff. Otherwise all lines are additions.
 */
export function extractWriteDiff(toolCall: ToolCall): DiffResult | null {
  const args = toolCall.arguments;
  if (!args || typeof args !== "object") return null;

  const a = args as Record<string, unknown>;
  const content = typeof a.content === "string" ? a.content : "";
  const filePath = typeof a.file_path === "string" ? a.file_path : "";

  if (!filePath) return null;

  const oldContent = toolCall.diffContext?.oldContent;

  if (oldContent != null) {
    // Overwrite existing file — compute proper diff
    const lines = computeDiff(oldContent, content);
    const additions = lines.filter((l) => l.type === "addition").length;
    const deletions = lines.filter((l) => l.type === "deletion").length;
    return { lines, filePath, additions, deletions };
  }

  // New file — all lines are additions
  const contentLines = content.split("\n");
  const lines: DiffLine[] = contentLines.map((line, i) => ({
    type: "addition" as const,
    content: line,
    oldLineNum: null,
    newLineNum: i + 1,
  }));

  return { lines, filePath, additions: lines.length, deletions: 0 };
}
