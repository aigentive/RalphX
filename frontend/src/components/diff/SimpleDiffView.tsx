/**
 * SimpleDiffView — Hunk-based diff renderer
 *
 * Renders server-provided DiffHunk[] with line numbers and optional
 * lazy range fetch for "Show N unchanged lines" gaps.
 *
 * Performance: hunks render synchronously. Range fetches are lazy (on click).
 */

import { useState, useCallback, useMemo, useRef } from "react";
import * as ScrollAreaPrimitive from "@radix-ui/react-scroll-area";
import { Virtuoso } from "react-virtuoso";
import { Button } from "@/components/ui/button";
import { diffApi } from "@/api/diff";
import type {
  DiffHunk,
  DiffLine,
  DiffRefKind,
  PrDiffAnnotation,
  RangeLine,
  WorkspaceReviewHunkAnnotation,
} from "@/api/diff";
import {
  annotationsForLine,
  buildAnnotationIndex,
  buildHunkAnnotationIndex,
  hunkAnnotationsForHunk,
  renderDiffLine,
  renderHunkHeader,
  renderHunkAnnotationRows,
  type AnnotationIndex,
  type DiffRenderVariant,
} from "./diffRenderHelpers";

export interface SimpleDiffViewProps {
  hunks: DiffHunk[];
  oldTotalLines: number;
  newTotalLines: number;
  isBinary?: boolean;
  language?: string | undefined;
  variant?: "standard" | "conflict";
  /**
   * When provided together with filePath and refKind, enables lazy range
   * fetch for "Show N unchanged lines" gap expanders.
   */
  conversationId?: string | undefined;
  filePath?: string | undefined;
  refKind?: DiffRefKind | undefined;
  /** Own the vertical scroll container. Inline virtualized callers disable this. */
  scrollContainer?: boolean | undefined;
  /** GitHub review/check annotations already filtered to this file. */
  annotations?: PrDiffAnnotation[] | undefined;
  /** Workspace Review hunk notes already filtered to this file and ref/source. */
  hunkAnnotations?: WorkspaceReviewHunkAnnotation[] | undefined;
  /** Compact density for embedded chat widgets; default keeps repository diff views unchanged. */
  density?: "standard" | "compact" | undefined;
  /** Initial line wrapping state. */
  defaultWrapLines?: boolean | undefined;
  /** Show the line wrapping toggle. */
  showWrapToggle?: boolean | undefined;
  /** Show expandable unchanged-line gap rows between hunks. */
  showContextGaps?: boolean | undefined;
  /** Disable per-row sticky line-number gutters for WebKit-sensitive embedded diffs. */
  stickyGutter?: boolean | undefined;
}

type GapState = "loading" | "error" | RangeLine[];

type Variant = DiffRenderVariant;
export const DIFF_ROW_VIRTUALIZATION_THRESHOLD = 1_000;
const INLINE_VIRTUAL_DIFF_HEIGHT = "min(70vh, 720px)";

type GapRowBase = {
  gapKey: string;
  gapCount: number;
  fromNewLine: number;
  toNewLine: number;
  fromOldLine: number;
};

type DiffVirtualRow =
  | {
      type: "hunk-header";
      key: string;
      header: string;
      oldStart: number;
      oldLines: number;
      newStart: number;
      newLines: number;
    }
  | { type: "line"; key: string; line: DiffLine }
  | { type: "range-line"; key: string; line: DiffLine }
  | ({ type: "gap-collapsed" | "gap-loading" | "gap-error" | "gap-hide"; key: string } & GapRowBase);
type GapVirtualRow = Extract<
  DiffVirtualRow,
  { type: "gap-collapsed" | "gap-loading" | "gap-error" | "gap-hide" }
>;

function annotationsForNewLineRange(
  index: AnnotationIndex,
  fromLine: number,
  toLine: number
): PrDiffAnnotation[] {
  const annotations = new Map<string, PrDiffAnnotation>();
  for (let line = fromLine; line <= toLine; line += 1) {
    for (const annotation of index.get(`new:${line}`) ?? []) {
      annotations.set(annotation.id, annotation);
    }
  }
  return [...annotations.values()];
}

/** Render a fetched context range line as a DiffLine-like row. */
function renderRangeLine(
  rl: RangeLine,
  oldLineNum: number,
  wrapLines: boolean,
  variant: Variant,
  annotations: PrDiffAnnotation[],
  stickyGutter: boolean,
) {
  const line: DiffLine = {
    kind: "context",
    content: rl.content,
    oldLineNum,
    newLineNum: rl.lineNum,
  };
  return renderDiffLine(line, rl.lineNum, wrapLines, variant, annotations, {
    stickyGutter,
  });
}

function pushGapRows(
  rows: DiffVirtualRow[],
  gap: GapRowBase,
  gapCache: Map<string, GapState>,
  collapsedGaps: Set<string>
) {
  if (gap.gapCount <= 0) return;

  const state = gapCache.get(gap.gapKey);
  const isCollapsed = collapsedGaps.has(gap.gapKey);

  if (state === "error") {
    rows.push({ type: "gap-error", key: `gap-${gap.gapKey}-error`, ...gap });
    return;
  }

  if (state === "loading") {
    rows.push({ type: "gap-loading", key: `gap-${gap.gapKey}-loading`, ...gap });
    return;
  }

  if (Array.isArray(state) && !isCollapsed) {
    state.forEach((rangeLine, index) => {
      rows.push({
        type: "range-line",
        key: `gap-${gap.gapKey}-line-${rangeLine.lineNum}-${index}`,
        line: {
          kind: "context",
          content: rangeLine.content,
          oldLineNum: gap.fromOldLine + index,
          newLineNum: rangeLine.lineNum,
        },
      });
    });
    rows.push({ type: "gap-hide", key: `gap-${gap.gapKey}-hide`, ...gap });
    return;
  }

  rows.push({ type: "gap-collapsed", key: `gap-${gap.gapKey}-collapsed`, ...gap });
}

function buildDiffVirtualRows(
  hunks: DiffHunk[],
  newTotalLines: number,
  showContextGaps: boolean,
  gapCache: Map<string, GapState>,
  collapsedGaps: Set<string>
): DiffVirtualRow[] {
  const rows: DiffVirtualRow[] = [];

  hunks.forEach((hunk, hunkIdx) => {
    const prevHunk = hunks[hunkIdx - 1];
    if (showContextGaps) {
      if (hunkIdx === 0) {
        pushGapRows(
          rows,
          {
            gapKey: "pre",
            gapCount: hunk.newStart - 1,
            fromNewLine: 1,
            toNewLine: hunk.newStart - 1,
            fromOldLine: 1,
          },
          gapCache,
          collapsedGaps
        );
      } else if (prevHunk) {
        const fromNewLine = prevHunk.newStart + prevHunk.newLines;
        pushGapRows(
          rows,
          {
            gapKey: String(hunkIdx),
            gapCount: hunk.newStart - fromNewLine,
            fromNewLine,
            toNewLine: hunk.newStart - 1,
            fromOldLine: prevHunk.oldStart + prevHunk.oldLines,
          },
          gapCache,
          collapsedGaps
        );
      }
    }

    rows.push({
      type: "hunk-header",
      key: `hunk-${hunkIdx}-header`,
      header: hunk.header,
      oldStart: hunk.oldStart,
      oldLines: hunk.oldLines,
      newStart: hunk.newStart,
      newLines: hunk.newLines,
    });
    hunk.lines.forEach((line, lineIdx) => {
      rows.push({
        type: "line",
        key: `hunk-${hunkIdx}-line-${lineIdx}-${line.oldLineNum ?? "x"}-${line.newLineNum ?? "x"}`,
        line,
      });
    });
  });

  if (showContextGaps) {
    const lastHunk = hunks[hunks.length - 1];
    if (lastHunk) {
      const fromNewLine = lastHunk.newStart + lastHunk.newLines;
      pushGapRows(
        rows,
        {
          gapKey: "post",
          gapCount: newTotalLines - lastHunk.newStart - lastHunk.newLines + 1,
          fromNewLine,
          toNewLine: newTotalLines,
          fromOldLine: lastHunk.oldStart + lastHunk.oldLines,
        },
        gapCache,
        collapsedGaps
      );
    }
  }

  return rows;
}

// ============================================================================
// Main component
// ============================================================================

export function SimpleDiffView({
  hunks,
  oldTotalLines: _oldTotalLines,
  newTotalLines,
  isBinary = false,
  variant = "standard",
  conversationId,
  filePath,
  refKind,
  scrollContainer = true,
  annotations = [],
  density = "standard",
  defaultWrapLines = true,
  showWrapToggle = true,
  showContextGaps = true,
  stickyGutter = true,
  hunkAnnotations = [],
}: SimpleDiffViewProps) {
  const [wrapLines, setWrapLines] = useState(defaultWrapLines);
  // gapCache state drives rendering; gapCacheRef mirrors it so callbacks
  // always read the latest value without a stale closure.
  const [gapCache, setGapCache] = useState<Map<string, GapState>>(() => new Map());
  const gapCacheRef = useRef(gapCache);
  // Track collapsed state: a gap that was expanded and then collapsed shows "Show N" again
  // but the cached data is still available (no re-fetch needed).
  const [collapsedGaps, setCollapsedGaps] = useState<Set<string>>(() => new Set());

  const canFetch =
    conversationId !== undefined &&
    filePath !== undefined &&
    refKind !== undefined;
  const annotationIndex = useMemo(() => buildAnnotationIndex(annotations), [annotations]);
  const hunkAnnotationIndex = useMemo(
    () => buildHunkAnnotationIndex(hunkAnnotations),
    [hunkAnnotations]
  );
  const bodyTextClass =
    density === "compact"
      ? "font-mono text-[0.6875rem] leading-[18px]"
      : "font-mono text-[0.8125rem] leading-[20px]";

  function setGapData(key: string, value: GapState) {
    // Update ref immediately so callbacks see fresh data; update state to re-render.
    gapCacheRef.current.set(key, value);
    setGapCache((prev) => {
      const next = new Map(prev);
      next.set(key, value);
      return next;
    });
  }

  function clearGapData(key: string) {
    gapCacheRef.current.delete(key);
    setGapCache((prev) => {
      const next = new Map(prev);
      next.delete(key);
      return next;
    });
  }

  const expandGap = useCallback(
    (gapKey: string, fromLine: number, toLine: number) => {
      if (!canFetch) return;

      // Read from ref (not state) — avoids stale closure in async callbacks.
      const cached = gapCacheRef.current.get(gapKey);

      // Cache hit (already have data) — just uncollapse
      if (Array.isArray(cached)) {
        setCollapsedGaps((prev) => {
          const next = new Set(prev);
          next.delete(gapKey);
          return next;
        });
        return;
      }

      // Already in flight
      if (cached === "loading") return;

      // Start fetch
      setGapData(gapKey, "loading");

      void diffApi
        .getAgentConversationWorkspaceFileContentRange({
          conversationId: conversationId!,
          side: "new",
          path: filePath!,
          refKind: refKind!,
          from: fromLine,
          to: toLine,
        })
        .then((lines) => {
          setGapData(gapKey, lines);
          setCollapsedGaps((prev) => {
            const next = new Set(prev);
            next.delete(gapKey);
            return next;
          });
        })
        .catch(() => {
          setGapData(gapKey, "error");
        });
    },
    [canFetch, conversationId, filePath, refKind]
  );

  const retryGap = useCallback(
    (gapKey: string, fromLine: number, toLine: number) => {
      clearGapData(gapKey);
      expandGap(gapKey, fromLine, toLine);
    },
    [expandGap]
  );

  const collapseGap = useCallback((gapKey: string) => {
    setCollapsedGaps((prev) => new Set(prev).add(gapKey));
  }, []);

  const diffLineCount = useMemo(
    () => hunks.reduce((sum, hunk) => sum + hunk.lines.length, 0),
    [hunks]
  );
  const shouldVirtualizeRows = diffLineCount >= DIFF_ROW_VIRTUALIZATION_THRESHOLD;
  const virtualRows = useMemo(
    () =>
      shouldVirtualizeRows
        ? buildDiffVirtualRows(
            hunks,
            newTotalLines,
            showContextGaps,
            gapCache,
            collapsedGaps
          )
        : [],
    [collapsedGaps, gapCache, hunks, newTotalLines, shouldVirtualizeRows, showContextGaps]
  );

  // ── Early exits ────────────────────────────────────────────────────────

  if (isBinary) {
    return (
      <div
        className="flex items-center justify-center h-full"
        style={{ color: "var(--text-muted)" }}
      >
        <p className="text-sm">Binary file — diff not shown</p>
      </div>
    );
  }

  if (hunks.length === 0) {
    return (
      <div
        className="flex items-center justify-center h-full"
        style={{ color: "var(--text-muted)" }}
      >
        <p className="text-sm">No changes</p>
      </div>
    );
  }

  // ── Gap computation helper ─────────────────────────────────────────────

  /** Render the gap region (context lines not in any hunk). */
  function renderGap(
    gapKey: string,
    gapCount: number,
    fromNewLine: number,
    toNewLine: number,
    fromOldLine: number
  ) {
    if (gapCount <= 0) return null;

    const state = gapCache.get(gapKey);
    const isCollapsed = collapsedGaps.has(gapKey);
    const hasData = Array.isArray(state);
    const hiddenAnnotations = annotationsForNewLineRange(
      annotationIndex,
      fromNewLine,
      toNewLine
    );

    return (
      <div
        key={`gap-${gapKey}`}
        data-testid="diff-gap"
      >
        {/* Error state */}
        {state === "error" && (
          <div
            data-testid="gap-error"
            className="flex items-center gap-2 px-3 py-1.5 text-[0.6875rem]"
            style={{
              borderBottom: "1px solid var(--overlay-faint)",
              color: "var(--text-muted)",
            }}
          >
            <span>Could not load context lines.</span>
            <button
              type="button"
              aria-label="Retry loading lines"
              className="underline hover:no-underline"
              style={{ color: "var(--text-secondary)" }}
              onClick={() => retryGap(gapKey, fromNewLine, toNewLine)}
            >
              Retry
            </button>
          </div>
        )}

        {/* Loading state */}
        {state === "loading" && (
          <div
            data-testid="gap-loading"
            className="px-3 py-1.5 text-[0.6875rem]"
            style={{
              borderBottom: "1px solid var(--overlay-faint)",
              color: "var(--text-muted)",
            }}
          >
            Loading…
          </div>
        )}

        {/* Expanded fetched content */}
        {hasData && !isCollapsed && (
          <>
            {(state as RangeLine[]).map((rl, i) =>
              renderRangeLine(
                rl,
                fromOldLine + i,
                wrapLines,
                variant,
                annotationsForLine(annotationIndex, {
                  kind: "context",
                  content: rl.content,
                  oldLineNum: fromOldLine + i,
                  newLineNum: rl.lineNum,
                }),
                stickyGutter,
              )
            )}
            <div
              data-testid="diff-gap-control"
              style={{ borderBottom: "1px solid var(--overlay-faint)" }}
            >
              <button
                type="button"
                aria-label="Hide unchanged lines"
                className="block px-3 py-1.5 text-[0.6875rem] hover:underline"
                style={{ color: "var(--text-muted)" }}
                onClick={() => collapseGap(gapKey)}
              >
                Hide unchanged lines
              </button>
            </div>
          </>
        )}

        {/* Collapsed or not-yet-fetched */}
        {(!hasData || isCollapsed) && state !== "loading" && state !== "error" && (
          <div
            data-testid="diff-gap-control"
            style={{ borderBottom: "1px solid var(--overlay-faint)" }}
          >
            {hiddenAnnotations.length > 0 && (
              <div
                className="px-3 pt-1.5 text-[0.6875rem] font-medium"
                data-testid="diff-hidden-annotations"
                style={{ color: "var(--status-warning)" }}
              >
                {hiddenAnnotations.length} GitHub annotation
                {hiddenAnnotations.length === 1 ? "" : "s"} in hidden context
              </div>
            )}
            {canFetch ? (
              <button
                type="button"
                aria-label={
                  hiddenAnnotations.length > 0
                    ? `Show ${hiddenAnnotations.length} hidden annotations in ${gapCount} unchanged lines`
                    : `Show ${gapCount} unchanged lines`
                }
                className="block px-3 py-1.5 text-[0.6875rem] hover:underline"
                style={{ color: "var(--text-muted)" }}
                onClick={() => expandGap(gapKey, fromNewLine, toNewLine)}
              >
                {hiddenAnnotations.length > 0
                  ? `Show annotations in ${gapCount} unchanged lines`
                  : `Show ${gapCount} unchanged lines`}
              </button>
            ) : (
              <span
                className="block px-3 py-1.5 text-[0.6875rem]"
                style={{ color: "var(--text-muted)" }}
              >
                {gapCount} unchanged lines
              </span>
            )}
          </div>
        )}
      </div>
    );
  }

  function renderVirtualGapRow(row: GapVirtualRow) {
    if (row.type === "gap-error") {
      return (
        <div data-testid="diff-gap">
          <div
            data-testid="gap-error"
            className="flex items-center gap-2 px-3 py-1.5 text-[0.6875rem]"
            style={{
              borderBottom: "1px solid var(--overlay-faint)",
              color: "var(--text-muted)",
            }}
          >
            <span>Could not load context lines.</span>
            <button
              type="button"
              aria-label="Retry loading lines"
              className="underline hover:no-underline"
              style={{ color: "var(--text-secondary)" }}
              onClick={() => retryGap(row.gapKey, row.fromNewLine, row.toNewLine)}
            >
              Retry
            </button>
          </div>
        </div>
      );
    }

    if (row.type === "gap-loading") {
      return (
        <div data-testid="diff-gap">
          <div
            data-testid="gap-loading"
            className="px-3 py-1.5 text-[0.6875rem]"
            style={{
              borderBottom: "1px solid var(--overlay-faint)",
              color: "var(--text-muted)",
            }}
          >
            Loading…
          </div>
        </div>
      );
    }

    if (row.type === "gap-hide") {
      return (
        <div data-testid="diff-gap">
          <div
            data-testid="diff-gap-control"
            style={{ borderBottom: "1px solid var(--overlay-faint)" }}
          >
            <button
              type="button"
              aria-label="Hide unchanged lines"
              className="block px-3 py-1.5 text-[0.6875rem] hover:underline"
              style={{ color: "var(--text-muted)" }}
              onClick={() => collapseGap(row.gapKey)}
            >
              Hide unchanged lines
            </button>
          </div>
        </div>
      );
    }

    const hiddenAnnotations = annotationsForNewLineRange(
      annotationIndex,
      row.fromNewLine,
      row.toNewLine
    );

    return (
      <div data-testid="diff-gap">
        <div
          data-testid="diff-gap-control"
          style={{ borderBottom: "1px solid var(--overlay-faint)" }}
        >
          {hiddenAnnotations.length > 0 && (
            <div
              className="px-3 pt-1.5 text-[0.6875rem] font-medium"
              data-testid="diff-hidden-annotations"
              style={{ color: "var(--status-warning)" }}
            >
              {hiddenAnnotations.length} GitHub annotation
              {hiddenAnnotations.length === 1 ? "" : "s"} in hidden context
            </div>
          )}
          {canFetch ? (
            <button
              type="button"
              aria-label={
                hiddenAnnotations.length > 0
                  ? `Show ${hiddenAnnotations.length} hidden annotations in ${row.gapCount} unchanged lines`
                  : `Show ${row.gapCount} unchanged lines`
              }
              className="block px-3 py-1.5 text-[0.6875rem] hover:underline"
              style={{ color: "var(--text-muted)" }}
              onClick={() => expandGap(row.gapKey, row.fromNewLine, row.toNewLine)}
            >
              {hiddenAnnotations.length > 0
                ? `Show annotations in ${row.gapCount} unchanged lines`
                : `Show ${row.gapCount} unchanged lines`}
            </button>
          ) : (
            <span
              className="block px-3 py-1.5 text-[0.6875rem]"
              style={{ color: "var(--text-muted)" }}
            >
              {row.gapCount} unchanged lines
            </span>
          )}
        </div>
      </div>
    );
  }

  function renderVirtualRow(_index: number, row: DiffVirtualRow) {
    if (row.type === "hunk-header") {
      const matchedHunkAnnotations = hunkAnnotationsForHunk(hunkAnnotationIndex, {
        header: row.header,
        oldStart: row.oldStart,
        oldLines: row.oldLines,
        newStart: row.newStart,
        newLines: row.newLines,
      });
      return (
        <>
          {renderHunkHeader(row.header)}
          {renderHunkAnnotationRows(matchedHunkAnnotations, wrapLines, variant)}
        </>
      );
    }
    if (row.type === "line" || row.type === "range-line") {
      return renderDiffLine(
        row.line,
        _index,
        wrapLines,
        variant,
        annotationsForLine(annotationIndex, row.line),
        { stickyGutter },
      );
    }
    return renderVirtualGapRow(row);
  }

  // ── Main render ────────────────────────────────────────────────────────

  if (shouldVirtualizeRows) {
    return (
      <div className={scrollContainer ? "h-full overflow-hidden" : "w-full overflow-hidden"}>
        <div
          className={
            scrollContainer
              ? `${bodyTextClass} flex h-full min-h-0 flex-col`
              : bodyTextClass
          }
          data-density={density}
          data-wrap-lines={wrapLines}
          data-testid="simple-diff-virtualized"
          style={{ backgroundColor: "var(--bg-base)" }}
        >
          {showWrapToggle && (
            <div
              className="px-3 py-2 border-b"
              style={{ borderColor: "var(--overlay-weak)" }}
            >
              <Button
                variant="ghost"
                className="h-7 px-2 text-[0.6875rem]"
                onClick={() => setWrapLines((prev) => !prev)}
              >
                {wrapLines ? "Disable wrap" : "Wrap lines"}
              </Button>
            </div>
          )}
          <Virtuoso
            data={virtualRows}
            data-testid="simple-diff-virtual-list"
            className={scrollContainer ? "min-h-0 flex-1" : undefined}
            style={{
              height: scrollContainer ? "100%" : INLINE_VIRTUAL_DIFF_HEIGHT,
              overflowX: "auto",
            }}
            computeItemKey={(_index, row) => row.key}
            increaseViewportBy={{ top: 320, bottom: 640 }}
            itemContent={renderVirtualRow}
          />
        </div>
      </div>
    );
  }

  return (
    <div className={scrollContainer ? "h-full overflow-y-auto" : "w-full overflow-visible"}>
      <div
        className={bodyTextClass}
        data-density={density}
        data-wrap-lines={wrapLines}
        style={{ backgroundColor: "var(--bg-base)" }}
      >
        {showWrapToggle && (
          <div
            className="px-3 py-2 border-b"
            style={{ borderColor: "var(--overlay-weak)" }}
          >
            <Button
              variant="ghost"
              className="h-7 px-2 text-[0.6875rem]"
              onClick={() => setWrapLines((prev) => !prev)}
            >
              {wrapLines ? "Disable wrap" : "Wrap lines"}
            </Button>
          </div>
        )}

        {hunks.map((hunk, hunkIdx) => {
          const prevHunk = hunks[hunkIdx - 1];

          // Gap before this hunk
          let gapKey: string;
          let gapFromNew: number;
          let gapToNew: number;
          let gapFromOld: number;
          let gapCount: number;

          if (hunkIdx === 0) {
            // Leading gap (before first hunk)
            gapKey = "pre";
            gapFromNew = 1;
            gapToNew = hunk.newStart - 1;
            gapFromOld = 1;
            gapCount = hunk.newStart - 1;
          } else {
            // Gap between prevHunk and this hunk
            gapKey = String(hunkIdx);
            gapFromNew = (prevHunk!.newStart + prevHunk!.newLines);
            gapToNew = hunk.newStart - 1;
            gapFromOld = (prevHunk!.oldStart + prevHunk!.oldLines);
            gapCount = hunk.newStart - (prevHunk!.newStart + prevHunk!.newLines);
          }

          return (
            <div key={`hunk-${hunkIdx}`}>
              {showContextGaps
                ? renderGap(gapKey, gapCount, gapFromNew, gapToNew, gapFromOld)
                : null}

              <div
                className="border-b"
                style={{ borderColor: "var(--overlay-faint)" }}
              >
                {renderHunkHeader(hunk.header)}
                {renderHunkAnnotationRows(
                  hunkAnnotationsForHunk(hunkAnnotationIndex, hunk),
                  wrapLines,
                  variant,
                )}
                <ScrollAreaPrimitive.Root className="w-full overflow-hidden">
                  <ScrollAreaPrimitive.Viewport className="w-full overflow-x-auto">
                    <div style={{ minWidth: wrapLines ? "auto" : "max-content" }}>
                      {hunk.lines.map((line, lineIdx) =>
                        renderDiffLine(
                          line,
                          lineIdx,
                          wrapLines,
                          variant,
                          annotationsForLine(annotationIndex, line),
                          { stickyGutter },
                        )
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
            </div>
          );
        })}

        {/* Trailing gap (after last hunk) */}
        {showContextGaps
          ? (() => {
              const lastHunk = hunks[hunks.length - 1]!;
              const trailingFromNew = lastHunk.newStart + lastHunk.newLines;
              const trailingToNew = newTotalLines;
              const trailingFromOld = lastHunk.oldStart + lastHunk.oldLines;
              const trailingCount = newTotalLines - lastHunk.newStart - lastHunk.newLines + 1;
              return renderGap(
                "post",
                trailingCount,
                trailingFromNew,
                trailingToNew,
                trailingFromOld
              );
            })()
          : null}
      </div>
    </div>
  );
}
