/**
 * ConflictDiffViewer - Chunk-based conflict diff using SimpleDiffView
 *
 * Renders ours vs theirs content as a unified diff with:
 * - Deletions (red) = lines only in ours (current branch)
 * - Additions (blue) = lines only in theirs (incoming branch)
 * - Context = shared between both
 */

import { useMemo } from "react";
import type { DiffHunk, DiffLine } from "@/api/diff";
import type { ConflictDiff } from "@/hooks/useConflictDiff";
import { SimpleDiffView } from "./SimpleDiffView";

// ── Client-side diff for conflict content ──────────────────────────────────

type EditKind = "context" | "deletion" | "addition";
interface Edit {
  kind: EditKind;
  oldIdx: number | null;
  newIdx: number | null;
}

/** LCS-based line diff: O(m*n) — acceptable for conflict-file sizes. */
function diffLines(oldLines: string[], newLines: string[]): Edit[] {
  const m = oldLines.length;
  const n = newLines.length;
  // dp[i][j] = LCS length for oldLines[0..i-1] vs newLines[0..j-1]
  const dp: number[][] = Array.from({ length: m + 1 }, () => new Array<number>(n + 1).fill(0));
  for (let i = 1; i <= m; i++) {
    const row = dp[i]!;
    const prevRow = dp[i - 1]!;
    for (let j = 1; j <= n; j++) {
      if (oldLines[i - 1] === newLines[j - 1]) {
        row[j] = (prevRow[j - 1] ?? 0) + 1;
      } else {
        row[j] = Math.max(prevRow[j] ?? 0, row[j - 1] ?? 0);
      }
    }
  }

  const edits: Edit[] = [];
  let i = m, j = n;
  while (i > 0 || j > 0) {
    if (i > 0 && j > 0 && oldLines[i - 1] === newLines[j - 1]) {
      edits.unshift({ kind: "context", oldIdx: i - 1, newIdx: j - 1 });
      i--;
      j--;
    } else if (j > 0 && (i === 0 || (dp[i]?.[j - 1] ?? 0) >= (dp[i - 1]?.[j] ?? 0))) {
      edits.unshift({ kind: "addition", oldIdx: null, newIdx: j - 1 });
      j--;
    } else {
      edits.unshift({ kind: "deletion", oldIdx: i - 1, newIdx: null });
      i--;
    }
  }
  return edits;
}

const CONTEXT_LINES = 3;

/**
 * Build DiffHunk[] from two content strings (for client-side conflict diffs).
 * Uses LCS diff with 3 context lines around each changed region.
 */
function buildHunksFromContent(
  oldContent: string,
  newContent: string
): { hunks: DiffHunk[]; oldTotalLines: number; newTotalLines: number } {
  const oldLines = oldContent ? oldContent.split("\n") : [];
  const newLines = newContent ? newContent.split("\n") : [];

  if (oldLines.length === 0 && newLines.length === 0) {
    return { hunks: [], oldTotalLines: 0, newTotalLines: 0 };
  }

  const edits = diffLines(oldLines, newLines);

  // Mark which edit indices fall within CONTEXT_LINES of a change
  const inHunk = new Array<boolean>(edits.length).fill(false);
  for (let i = 0; i < edits.length; i++) {
    if (edits[i]?.kind !== "context") {
      for (
        let c = Math.max(0, i - CONTEXT_LINES);
        c <= Math.min(edits.length - 1, i + CONTEXT_LINES);
        c++
      ) {
        inHunk[c] = true;
      }
    }
  }

  const hunks: DiffHunk[] = [];
  let oldLine = 1;
  let newLine = 1;
  let hunkOldStart = 1;
  let hunkNewStart = 1;
  let hunkOldLines = 0;
  let hunkNewLines = 0;
  let hunkLines: DiffLine[] = [];
  let inCurrentHunk = false;

  function flushHunk() {
    if (hunkLines.length > 0) {
      hunks.push({
        oldStart: hunkOldStart,
        oldLines: hunkOldLines,
        newStart: hunkNewStart,
        newLines: hunkNewLines,
        header: `@@ -${hunkOldStart},${hunkOldLines} +${hunkNewStart},${hunkNewLines} @@`,
        lines: hunkLines,
      });
    }
    hunkLines = [];
    hunkOldLines = 0;
    hunkNewLines = 0;
    inCurrentHunk = false;
  }

  for (let i = 0; i < edits.length; i++) {
    const edit = edits[i]!;
    if (inHunk[i]) {
      if (!inCurrentHunk) {
        hunkOldStart = oldLine;
        hunkNewStart = newLine;
        inCurrentHunk = true;
      }
      const content =
        edit.kind === "addition"
          ? (newLines[edit.newIdx ?? 0] ?? "")
          : (oldLines[edit.oldIdx ?? 0] ?? "");

      hunkLines.push({
        kind: edit.kind,
        content,
        oldLineNum: edit.kind !== "addition" ? oldLine : null,
        newLineNum: edit.kind !== "deletion" ? newLine : null,
      });

      if (edit.kind !== "addition") {
        hunkOldLines++;
        oldLine++;
      }
      if (edit.kind !== "deletion") {
        hunkNewLines++;
        newLine++;
      }
    } else {
      if (inCurrentHunk) flushHunk();
      // Advance line counters for lines outside any hunk
      if (edit.kind !== "addition") oldLine++;
      if (edit.kind !== "deletion") newLine++;
    }
  }
  flushHunk();

  return { hunks, oldTotalLines: oldLines.length, newTotalLines: newLines.length };
}

interface ConflictDiffViewerProps {
  /** Conflict diff data from useConflictDiff hook */
  conflictDiff: ConflictDiff;
}

/**
 * Get file extension for language badge display
 */
function getFileExtension(filePath: string): string {
  const parts = filePath.split(".");
  if (parts.length > 1) {
    return parts[parts.length - 1] ?? "";
  }
  return "";
}

export function ConflictDiffViewer({ conflictDiff }: ConflictDiffViewerProps) {
  const { filePath, oursContent, theirsContent, language } = conflictDiff;

  const displayLanguage = language || getFileExtension(filePath);
  const { hunks, oldTotalLines, newTotalLines } = useMemo(
    () => buildHunksFromContent(oursContent ?? "", theirsContent ?? ""),
    [oursContent, theirsContent]
  );

  return (
    <div className="h-full flex flex-col">
      <div
        className="font-mono text-[0.8125rem] leading-[20px]"
        style={{ backgroundColor: "var(--bg-base)" }}
      >
        {/* Header with file path and language badge */}
        <div
          className="flex items-center justify-between px-3 py-2 border-b"
          style={{ borderColor: "var(--overlay-weak)" }}
        >
          <span
            className="text-sm truncate"
            style={{ color: "var(--text-secondary)" }}
          >
            {filePath}
          </span>
          {displayLanguage && (
            <span
              className="text-[0.6875rem] px-2 py-0.5 rounded ml-2 shrink-0"
              style={{
                backgroundColor: "var(--overlay-weak)",
                color: "var(--text-muted)",
              }}
            >
              {displayLanguage}
            </span>
          )}
        </div>

        {/* Conflict legend */}
        <div
          className="flex items-center gap-4 px-3 py-1.5 text-[0.6875rem]"
          style={{
            backgroundColor: "var(--overlay-faint)",
            borderBottom: "1px solid var(--overlay-faint)",
          }}
        >
          <span className="flex items-center gap-1.5">
            <span
              className="w-3 h-3 rounded"
              style={{ backgroundColor: "var(--status-error-muted)" }}
            />
            <span style={{ color: "var(--status-error)" }}>-</span>
            <span style={{ color: "var(--text-muted)" }}>Ours (current)</span>
          </span>
          <span className="flex items-center gap-1.5">
            <span
              className="w-3 h-3 rounded"
              style={{ backgroundColor: "var(--status-info-muted)" }}
            />
            <span style={{ color: "var(--status-info)" }}>+</span>
            <span style={{ color: "var(--text-muted)" }}>Theirs (incoming)</span>
          </span>
        </div>
      </div>

      {/* Diff content via SimpleDiffView */}
      <div className="flex-1 min-h-0">
        <SimpleDiffView
          hunks={hunks}
          oldTotalLines={oldTotalLines}
          newTotalLines={newTotalLines}
          language={displayLanguage}
          variant="conflict"
        />
      </div>
    </div>
  );
}
