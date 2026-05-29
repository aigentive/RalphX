/**
 * DiffToolCallView.utils - Diff computation and line rendering helpers
 *
 * Extracted from SimpleDiffView.tsx for reuse in DiffToolCallView.
 * Provides LCS-based diff computation and tool-call-specific extractors.
 */

import type { ToolCall } from "./ToolCallIndicator";
import { canonicalizeToolName } from "./tool-widgets/tool-name";
import { withAlpha } from "@/lib/theme-colors";
import type { DiffHunk, DiffLine, FileDiff } from "@/api/diff";

// ============================================================================
// Types
// ============================================================================

interface Match {
  oldIdx: number;
  newIdx: number;
}

export type DiffDisplayKind = "diff" | "final-content";

export interface DiffResult {
  filePath: string;
  displayKind: DiffDisplayKind;
  additions: number | null;
  deletions: number | null;
  previewDiff: FileDiff | null;
  fullDiff: FileDiff | null;
  oldContent: string | null;
  newContent: string | null;
  finalContent: string | null;
  baselineUnavailable: boolean;
  newFile: boolean;
}

const HUNK_CONTEXT_LINES = 3;
const SYNC_FULL_DIFF_LINE_LIMIT = 320;

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
  const oldLines = splitLinesForDiff(oldContent);
  const newLines = splitLinesForDiff(newContent);
  const result: DiffLine[] = [];

  const lcs = computeLCS(oldLines, newLines);

  let oldIdx = 0;
  let newIdx = 0;
  let oldLineNum = 1;
  let newLineNum = 1;

  for (const match of lcs) {
    while (oldIdx < match.oldIdx) {
      result.push({
        kind: "deletion",
        content: oldLines[oldIdx] ?? "",
        oldLineNum: oldLineNum++,
        newLineNum: null,
      });
      oldIdx++;
    }

    while (newIdx < match.newIdx) {
      result.push({
        kind: "addition",
        content: newLines[newIdx] ?? "",
        oldLineNum: null,
        newLineNum: newLineNum++,
      });
      newIdx++;
    }

    result.push({
      kind: "context",
      content: oldLines[oldIdx] ?? "",
      oldLineNum: oldLineNum++,
      newLineNum: newLineNum++,
    });
    oldIdx++;
    newIdx++;
  }

  while (oldIdx < oldLines.length) {
    result.push({
      kind: "deletion",
      content: oldLines[oldIdx] ?? "",
      oldLineNum: oldLineNum++,
      newLineNum: null,
    });
    oldIdx++;
  }

  while (newIdx < newLines.length) {
    result.push({
      kind: "addition",
      content: newLines[newIdx] ?? "",
      oldLineNum: null,
      newLineNum: newLineNum++,
    });
    newIdx++;
  }

  return result;
}

function lineCount(content: string): number {
  return content.length === 0 ? 0 : content.split("\n").length;
}

function splitLinesForDiff(content: string): string[] {
  return content.length === 0 ? [] : content.split("\n");
}

function getLanguageFromPath(filePath: string): string {
  const ext = filePath.split(".").pop()?.toLowerCase() ?? "";
  switch (ext) {
    case "ts":
    case "tsx":
      return "typescript";
    case "js":
    case "jsx":
      return "javascript";
    case "rs":
      return "rust";
    case "css":
      return "css";
    case "html":
      return "html";
    case "json":
      return "json";
    case "md":
      return "markdown";
    default:
      return "text";
  }
}

function formatHunkHeader(hunk: Pick<DiffHunk, "oldStart" | "oldLines" | "newStart" | "newLines">): string {
  return `@@ -${hunk.oldStart},${hunk.oldLines} +${hunk.newStart},${hunk.newLines} @@`;
}

function hunkStartLine(lines: DiffLine[], side: "old" | "new"): number {
  const field = side === "old" ? "oldLineNum" : "newLineNum";
  const first = lines.find((line) => line[field] != null)?.[field];
  if (first != null) return first;
  return 0;
}

function toHunk(lines: DiffLine[]): DiffHunk {
  const hunk = {
    oldStart: hunkStartLine(lines, "old"),
    oldLines: lines.filter((line) => line.kind !== "addition").length,
    newStart: hunkStartLine(lines, "new"),
    newLines: lines.filter((line) => line.kind !== "deletion").length,
    header: "",
    lines,
  };
  return { ...hunk, header: formatHunkHeader(hunk) };
}

function buildHunksFromLines(lines: DiffLine[], contextLines = HUNK_CONTEXT_LINES): DiffHunk[] {
  const changedIndexes = lines
    .map((line, index) => (line.kind === "context" ? -1 : index))
    .filter((index) => index >= 0);
  if (changedIndexes.length === 0) return [];

  const ranges: Array<{ start: number; end: number }> = [];
  for (const changedIndex of changedIndexes) {
    const start = Math.max(0, changedIndex - contextLines);
    const end = Math.min(lines.length - 1, changedIndex + contextLines);
    const previous = ranges[ranges.length - 1];
    if (previous && start <= previous.end + 1) {
      previous.end = Math.max(previous.end, end);
    } else {
      ranges.push({ start, end });
    }
  }

  return ranges.map((range) => toHunk(lines.slice(range.start, range.end + 1)));
}

function buildFileDiffFromLines(
  filePath: string,
  oldContent: string,
  newContent: string,
  lines: DiffLine[]
): FileDiff {
  return {
    filePath,
    language: getLanguageFromPath(filePath),
    hunks: buildHunksFromLines(lines),
    oldTotalLines: lineCount(oldContent),
    newTotalLines: lineCount(newContent),
    isBinary: false,
  };
}

export function computeFileDiff(filePath: string, oldContent: string, newContent: string): FileDiff {
  return buildFileDiffFromLines(filePath, oldContent, newContent, computeDiff(oldContent, newContent));
}

function computeFirstChangedRange(oldLines: string[], newLines: string[]) {
  let prefix = 0;
  while (
    prefix < oldLines.length &&
    prefix < newLines.length &&
    oldLines[prefix] === newLines[prefix]
  ) {
    prefix += 1;
  }

  if (prefix === oldLines.length && prefix === newLines.length) {
    return null;
  }

  return { prefix };
}

function computePreviewDiff(filePath: string, oldContent: string, newContent: string): FileDiff {
  const oldLines = splitLinesForDiff(oldContent);
  const newLines = splitLinesForDiff(newContent);
  const range = computeFirstChangedRange(oldLines, newLines);
  if (!range) {
    return {
      filePath,
      language: getLanguageFromPath(filePath),
      hunks: [],
      oldTotalLines: lineCount(oldContent),
      newTotalLines: lineCount(newContent),
      isBinary: false,
    };
  }

  const start = Math.max(0, range.prefix - HUNK_CONTEXT_LINES);
  const oldChangedEnd = range.prefix < oldLines.length ? range.prefix : range.prefix - 1;
  const newChangedEnd = range.prefix < newLines.length ? range.prefix : range.prefix - 1;
  const oldContextEnd = Math.min(
    oldLines.length - 1,
    Math.max(oldChangedEnd, range.prefix - 1) + HUNK_CONTEXT_LINES
  );
  const newContextEnd = Math.min(
    newLines.length - 1,
    Math.max(newChangedEnd, range.prefix - 1) + HUNK_CONTEXT_LINES
  );
  const lines: DiffLine[] = [];
  let oldLineNum = start + 1;
  let newLineNum = start + 1;

  for (let index = start; index < range.prefix; index += 1) {
    lines.push({
      kind: "context",
      content: oldLines[index] ?? "",
      oldLineNum: oldLineNum++,
      newLineNum: newLineNum++,
    });
  }

  for (let index = range.prefix; index <= oldChangedEnd; index += 1) {
    lines.push({
      kind: "deletion",
      content: oldLines[index] ?? "",
      oldLineNum: oldLineNum++,
      newLineNum: null,
    });
  }

  for (let index = range.prefix; index <= newChangedEnd; index += 1) {
    lines.push({
      kind: "addition",
      content: newLines[index] ?? "",
      oldLineNum: null,
      newLineNum: newLineNum++,
    });
  }

  const sharedContextEnd = Math.min(
    oldContextEnd - Math.max(oldChangedEnd, range.prefix - 1),
    newContextEnd - Math.max(newChangedEnd, range.prefix - 1)
  );
  for (let offset = 1; offset <= sharedContextEnd; offset += 1) {
    lines.push({
      kind: "context",
      content: oldLines[Math.max(oldChangedEnd, range.prefix - 1) + offset] ?? "",
      oldLineNum: oldLineNum++,
      newLineNum: newLineNum++,
    });
  }

  return {
    filePath,
    language: getLanguageFromPath(filePath),
    hunks: lines.length > 0 ? [toHunk(lines)] : [],
    oldTotalLines: lineCount(oldContent),
    newTotalLines: lineCount(newContent),
    isBinary: false,
  };
}

function countChanges(diff: FileDiff): { additions: number; deletions: number } {
  let additions = 0;
  let deletions = 0;
  for (const hunk of diff.hunks) {
    for (const line of hunk.lines) {
      if (line.kind === "addition") additions += 1;
      if (line.kind === "deletion") deletions += 1;
    }
  }
  return { additions, deletions };
}

function buildDiffResult(
  filePath: string,
  oldContent: string,
  newContent: string,
  options: { newFile?: boolean } = {}
): DiffResult {
  const totalLines = lineCount(oldContent) + lineCount(newContent);
  const fullDiff =
    totalLines <= SYNC_FULL_DIFF_LINE_LIMIT
      ? computeFileDiff(filePath, oldContent, newContent)
      : null;
  const previewDiff = fullDiff
    ? { ...fullDiff, hunks: fullDiff.hunks.slice(0, 1) }
    : computePreviewDiff(filePath, oldContent, newContent);
  const counts = fullDiff ? countChanges(fullDiff) : null;

  return {
    filePath,
    displayKind: "diff",
    additions: counts?.additions ?? null,
    deletions: counts?.deletions ?? null,
    previewDiff,
    fullDiff,
    oldContent,
    newContent,
    finalContent: null,
    baselineUnavailable: false,
    newFile: options.newFile === true,
  };
}

function buildPreviewDiffResult(
  filePath: string,
  previewDiff: FileDiff,
  options: { newFile?: boolean } = {}
): DiffResult {
  const counts = options.newFile
    ? { additions: previewDiff.newTotalLines, deletions: 0 }
    : null;

  return {
    filePath,
    displayKind: "diff",
    additions: counts?.additions ?? null,
    deletions: counts?.deletions ?? null,
    previewDiff,
    fullDiff: null,
    oldContent: null,
    newContent: null,
    finalContent: null,
    baselineUnavailable: false,
    newFile: options.newFile === true,
  };
}

function buildFinalContentResult(filePath: string, finalContent: string): DiffResult {
  return {
    filePath,
    displayKind: "final-content",
    additions: null,
    deletions: null,
    previewDiff: null,
    fullDiff: null,
    oldContent: null,
    newContent: null,
    finalContent,
    baselineUnavailable: true,
    newFile: false,
  };
}

// ============================================================================
// Line Rendering Helpers
// ============================================================================

export function getLineBackground(kind: DiffLine["kind"]): string {
  switch (kind) {
    case "addition":
      return "var(--status-success-muted)";
    case "deletion":
      return "var(--status-error-muted)";
    default:
      return "transparent";
  }
}

export function getLineNumColor(kind: DiffLine["kind"]): string {
  switch (kind) {
    case "addition":
      return withAlpha("var(--status-success)", 60);
    case "deletion":
      return withAlpha("var(--status-error)", 60);
    default:
      return "var(--text-muted)";
  }
}

export function getLinePrefix(kind: DiffLine["kind"]): string {
  switch (kind) {
    case "addition":
      return "+";
    case "deletion":
      return "-";
    default:
      return " ";
  }
}

export function getPrefixColor(kind: DiffLine["kind"]): string {
  switch (kind) {
    case "addition":
      return "var(--status-success)";
    case "deletion":
      return "var(--status-error)";
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
  const filePath = typeof a.file_path === "string" ? a.file_path : "";
  const oldString = typeof a.old_string === "string" ? a.old_string : null;
  const newString = typeof a.new_string === "string" ? a.new_string : null;

  if (!filePath) return null;
  if (oldString == null || newString == null) {
    return toolCall.diffPreview
      ? buildPreviewDiffResult(filePath, toolCall.diffPreview)
      : null;
  }

  return buildDiffResult(filePath, oldString, newString);
}

/**
 * Extract diff data from a Write tool call.
 * Write arguments contain content and file_path.
 * If diffContext.oldContent exists, compute a real diff. If the backend has
 * confirmed the old file did not exist, render the write as a new-file diff.
 */
export function extractWriteDiff(toolCall: ToolCall): DiffResult | null {
  const args = toolCall.arguments;
  if (!args || typeof args !== "object") return null;

  const a = args as Record<string, unknown>;
  const filePath = typeof a.file_path === "string" ? a.file_path : "";
  const content = typeof a.content === "string" ? a.content : null;

  if (!filePath) return null;
  const isNewFile = toolCall.diffContext?.oldFileExists === false;
  if (content == null) {
    return toolCall.diffPreview
      ? buildPreviewDiffResult(filePath, toolCall.diffPreview, { newFile: isNewFile })
      : null;
  }

  const oldContent = toolCall.diffContext?.oldContent;

  if (oldContent != null) {
    return buildDiffResult(filePath, oldContent, content);
  }

  if (isNewFile) {
    return buildDiffResult(filePath, "", content, { newFile: true });
  }

  return buildFinalContentResult(filePath, content);
}
