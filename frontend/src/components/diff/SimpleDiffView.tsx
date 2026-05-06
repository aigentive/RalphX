/**
 * SimpleDiffView - A reliable inline diff renderer
 *
 * Renders unified diff content with line numbers and syntax highlighting.
 * Fallback for when @git-diff-view/react has issues.
 */

import { useMemo, useState } from "react";
import * as ScrollAreaPrimitive from "@radix-ui/react-scroll-area";
import { Button } from "@/components/ui/button";
import { withAlpha } from "@/lib/theme-colors";

interface SimpleDiffViewProps {
  oldContent: string;
  newContent: string;
  language?: string | undefined;
  variant?: "standard" | "conflict";
}

interface DiffLine {
  type: "context" | "addition" | "deletion" | "header";
  content: string;
  oldLineNum: number | null;
  newLineNum: number | null;
}

interface DiffHunk {
  id: string;
  start: number;
  end: number;
  header: string;
  lines: DiffLine[];
}

const CONTEXT_LINES = 3;
const LARGE_DIFF_LINE_LIMIT = 4000;

/**
 * Simple diff algorithm - generates unified diff lines
 */
function computeDiff(oldContent: string, newContent: string): DiffLine[] {
  const oldLines = oldContent.split("\n");
  const newLines = newContent.split("\n");
  const result: DiffLine[] = [];

  // Simple LCS-based diff
  const lcs = computeLCS(oldLines, newLines);

  let oldIdx = 0;
  let newIdx = 0;
  let oldLineNum = 1;
  let newLineNum = 1;

  for (const match of lcs) {
    // Add deletions (lines in old but not in new before this match)
    while (oldIdx < match.oldIdx) {
      result.push({
        type: "deletion",
        content: oldLines[oldIdx] ?? "",
        oldLineNum: oldLineNum++,
        newLineNum: null,
      });
      oldIdx++;
    }

    // Add additions (lines in new but not in old before this match)
    while (newIdx < match.newIdx) {
      result.push({
        type: "addition",
        content: newLines[newIdx] ?? "",
        oldLineNum: null,
        newLineNum: newLineNum++,
      });
      newIdx++;
    }

    // Add context line (matching)
    result.push({
      type: "context",
      content: oldLines[oldIdx] ?? "",
      oldLineNum: oldLineNum++,
      newLineNum: newLineNum++,
    });
    oldIdx++;
    newIdx++;
  }

  // Add remaining deletions
  while (oldIdx < oldLines.length) {
    result.push({
      type: "deletion",
      content: oldLines[oldIdx] ?? "",
      oldLineNum: oldLineNum++,
      newLineNum: null,
    });
    oldIdx++;
  }

  // Add remaining additions
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

interface Match {
  oldIdx: number;
  newIdx: number;
}

/**
 * Compute Longest Common Subsequence indices
 */
function computeLCS(oldLines: string[], newLines: string[]): Match[] {
  const m = oldLines.length;
  const n = newLines.length;

  // Build DP table
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

  // Backtrack to find matches
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

type Variant = "standard" | "conflict";

/**
 * Get line background color based on type and variant
 */
function getLineBackground(type: DiffLine["type"], variant: Variant): string {
  switch (type) {
    case "addition":
      return variant === "conflict"
        ? "var(--status-info-muted)"
        : "var(--status-success-muted)";
    case "deletion":
      return "var(--status-error-muted)";
    default:
      return "transparent";
  }
}

/**
 * Get line number color based on type and variant
 */
function getLineNumColor(type: DiffLine["type"], variant: Variant): string {
  switch (type) {
    case "addition":
      return variant === "conflict"
        ? withAlpha("var(--status-info)", 60)
        : withAlpha("var(--status-success)", 60);
    case "deletion":
      return withAlpha("var(--status-error)", 60);
    default:
      return "var(--text-muted)";
  }
}

/**
 * Get prefix character for line type
 */
function getLinePrefix(type: DiffLine["type"]): string {
  switch (type) {
    case "addition":
      return "+";
    case "deletion":
      return "-";
    default:
      return " ";
  }
}

/**
 * Get prefix color based on type and variant
 */
function getPrefixColor(type: DiffLine["type"], variant: Variant): string {
  switch (type) {
    case "addition":
      return variant === "conflict" ? "var(--status-info)" : "var(--status-success)";
    case "deletion":
      return "var(--status-error)";
    case "header":
      return withAlpha("var(--text-primary)", 45);
    default:
      return "transparent";
  }
}

function hasChanges(lines: DiffLine[]): boolean {
  return lines.some((line) => line.type === "addition" || line.type === "deletion");
}

function buildHunks(lines: DiffLine[]): DiffHunk[] {
  if (!hasChanges(lines)) {
    return [
      {
        id: "all",
        start: 0,
        end: Math.max(0, lines.length - 1),
        header: "@@ 0,0 +0,0 @@",
        lines,
      },
    ];
  }

  const include = new Array(lines.length).fill(false);
  lines.forEach((line, index) => {
    if (line.type === "addition" || line.type === "deletion") {
      const start = Math.max(0, index - CONTEXT_LINES);
      const end = Math.min(lines.length - 1, index + CONTEXT_LINES);
      for (let i = start; i <= end; i++) {
        include[i] = true;
      }
    }
  });

  const hunks: DiffHunk[] = [];
  let i = 0;
  while (i < lines.length) {
    if (!include[i]) {
      i++;
      continue;
    }

    const start = i;
    while (i < lines.length && include[i]) {
      i++;
    }
    const end = i - 1;
    const hunkLines = lines.slice(start, end + 1);

    const firstLine = hunkLines.find(
      (line) => line.oldLineNum !== null || line.newLineNum !== null
    );
    const oldStart = firstLine?.oldLineNum ?? 0;
    const newStart = firstLine?.newLineNum ?? 0;
    const oldCount = hunkLines.filter((line) => line.oldLineNum !== null).length;
    const newCount = hunkLines.filter((line) => line.newLineNum !== null).length;
    const header = `@@ -${oldStart},${oldCount} +${newStart},${newCount} @@`;

    hunks.push({
      id: `${start}-${end}`,
      start,
      end,
      header,
      lines: hunkLines,
    });
  }

  return hunks;
}

function renderHeader(content: string) {
  return (
    <div
      className="px-3 py-1 text-[0.6875rem] font-mono"
      style={{
        backgroundColor: "var(--overlay-weak)",
        color: withAlpha("var(--text-primary)", 60),
        borderTop: "1px solid var(--overlay-weak)",
        borderBottom: "1px solid var(--overlay-weak)",
      }}
    >
      {content}
    </div>
  );
}

function renderLine(line: DiffLine, index: number, wrapLines: boolean, variant: Variant) {

  return (
    <div
      key={index}
      className="flex"
      style={{
        backgroundColor: getLineBackground(line.type, variant),
        minHeight: "20px",
      }}
    >
      <div
        className="w-12 shrink-0 text-right pr-2 select-none z-10"
        style={{
          position: "sticky",
          left: 0,
          color: getLineNumColor(line.type, variant),
          backgroundColor: "var(--bg-surface)",
        }}
      >
        {line.oldLineNum ?? ""}
      </div>

      <div
        className="w-12 shrink-0 text-right pr-2 select-none border-r z-10"
        style={{
          position: "sticky",
          left: 48,
          color: getLineNumColor(line.type, variant),
          backgroundColor: "var(--bg-surface)",
          borderColor: "var(--border-subtle)",
        }}
      >
        {line.newLineNum ?? ""}
      </div>

      <div
        className="w-6 shrink-0 text-center select-none font-bold z-10"
        style={{
          position: "sticky",
          left: 96,
          color: getPrefixColor(line.type, variant),
          backgroundColor: "var(--bg-surface)",
        }}
      >
        {getLinePrefix(line.type)}
      </div>

      <div
        className={`flex-1 pr-4 min-w-0 ${
          wrapLines ? "whitespace-pre-wrap break-all" : "whitespace-pre"
        }`}
        style={{
          color:
            line.type === "deletion"
              ? "var(--text-muted)"
              : "var(--text-secondary)",
        }}
      >
        {line.content || " "}
      </div>
    </div>
  );
}

export function SimpleDiffView({ oldContent, newContent, variant = "standard" }: SimpleDiffViewProps) {
  const [renderLargeDiff, setRenderLargeDiff] = useState(false);
  const [expandedGaps, setExpandedGaps] = useState<Set<string>>(() => new Set());
  const [wrapLines, setWrapLines] = useState(true);
  const totalLines = useMemo(() => {
    const oldLines = oldContent.split("\n").length;
    const newLines = newContent.split("\n").length;
    return oldLines + newLines;
  }, [oldContent, newContent]);
  const isLargeDiff = totalLines > LARGE_DIFF_LINE_LIMIT;
  const allowRender = !isLargeDiff || renderLargeDiff;

  const diffLines = useMemo(() => {
    if (!allowRender) return [];
    return computeDiff(oldContent, newContent);
  }, [allowRender, oldContent, newContent]);
  const hunks = useMemo(
    () => (allowRender && hasChanges(diffLines) ? buildHunks(diffLines) : []),
    [allowRender, diffLines]
  );

  const toggleGap = (gapId: string) => {
    setExpandedGaps((prev) => {
      const next = new Set(prev);
      if (next.has(gapId)) {
        next.delete(gapId);
      } else {
        next.add(gapId);
      }
      return next;
    });
  };

  if (!allowRender) {
    return (
      <div
        className="flex flex-col items-center justify-center h-full gap-3 px-6"
        style={{ color: "var(--text-muted)" }}
      >
        <div className="text-sm">Diff too large to render quickly</div>
        <div className="text-xs text-text-primary/50">
          {totalLines.toLocaleString()} total lines in this file.
        </div>
        <Button
          variant="ghost"
          className="h-8 px-3 text-xs"
          onClick={() => setRenderLargeDiff(true)}
        >
          Render anyway
        </Button>
      </div>
    );
  }

  if (diffLines.length === 0 || !hasChanges(diffLines)) {
    return (
      <div
        className="flex items-center justify-center h-full"
        style={{ color: "var(--text-muted)" }}
      >
        <p className="text-sm">No changes</p>
      </div>
    );
  }

  return (
    <div className="h-full overflow-y-auto">
      <div
        className="font-mono text-[0.8125rem] leading-[20px]"
        style={{ backgroundColor: "var(--bg-base)" }}
      >
        <div className="px-3 py-2 border-b" style={{ borderColor: "var(--overlay-weak)" }}>
          <Button
            variant="ghost"
            className="h-7 px-2 text-[0.6875rem]"
            onClick={() => setWrapLines((prev) => !prev)}
          >
            {wrapLines ? "Disable wrap" : "Wrap lines"}
          </Button>
        </div>
        {hunks.map((hunk, index) => {
          const prev = hunks[index - 1];
          const gapStart = prev ? prev.end + 1 : 0;
          const gapEnd = hunk.start - 1;
          const gapId = `${gapStart}-${gapEnd}`;
          const gapHasLines = gapEnd >= gapStart;
          const gapLines = gapHasLines ? diffLines.slice(gapStart, gapEnd + 1) : [];
          const isExpanded = expandedGaps.has(gapId);

          return (
            <div key={hunk.id} className="border-b" style={{ borderColor: "var(--overlay-faint)" }}>
              {gapHasLines && (
                <div className="px-3 py-2">
                  {isExpanded ? (
                    <>
                      {gapLines.map((line, gapIndex) =>
                        renderLine(
                          {
                            ...line,
                            type: "context",
                          },
                          gapStart + gapIndex,
                          wrapLines,
                          variant
                        )
                      )}
                      <button
                        type="button"
                        className="mt-2 text-[0.6875rem] text-text-primary/50 hover:text-text-primary/70"
                        onClick={() => toggleGap(gapId)}
                      >
                        Hide unchanged lines
                      </button>
                    </>
                  ) : (
                    <button
                      type="button"
                      className="text-[0.6875rem] text-text-primary/50 hover:text-text-primary/70"
                      onClick={() => toggleGap(gapId)}
                    >
                      Show {gapLines.length} unchanged lines
                    </button>
                  )}
                </div>
              )}

              {renderHeader(hunk.header)}
              <ScrollAreaPrimitive.Root className="w-full overflow-hidden">
                <ScrollAreaPrimitive.Viewport className="w-full overflow-x-auto">
                  <div style={{ minWidth: wrapLines ? "auto" : "max-content" }}>
                    {hunk.lines.map((line, lineIndex) =>
                      renderLine(line, hunk.start + lineIndex, wrapLines, variant)
                    )}
                  </div>
                </ScrollAreaPrimitive.Viewport>
                <ScrollAreaPrimitive.ScrollAreaScrollbar
                  orientation="horizontal"
                  className="h-2.5 flex-col border-t border-t-transparent p-[1px]"
                >
                  <ScrollAreaPrimitive.ScrollAreaThumb className="relative flex-1 rounded-full bg-border" />
                </ScrollAreaPrimitive.ScrollAreaScrollbar>
              </ScrollAreaPrimitive.Root>
            </div>
          );
        })}
      </div>
    </div>
  );
}
