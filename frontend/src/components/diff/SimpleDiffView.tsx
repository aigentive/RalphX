/**
 * SimpleDiffView — Hunk-based diff renderer
 *
 * Renders server-provided DiffHunk[] with line numbers and optional
 * lazy range fetch for "Show N unchanged lines" gaps.
 *
 * Performance: hunks render synchronously. Range fetches are lazy (on click).
 */

import { useState, useCallback, useRef } from "react";
import * as ScrollAreaPrimitive from "@radix-ui/react-scroll-area";
import { Button } from "@/components/ui/button";
import { withAlpha } from "@/lib/theme-colors";
import { diffApi } from "@/api/diff";
import type { DiffHunk, DiffLine, DiffRefKind, RangeLine } from "@/api/diff";

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
}

type GapState = "loading" | "error" | RangeLine[];

type Variant = "standard" | "conflict";

// ============================================================================
// Rendering helpers (pure)
// ============================================================================

function getLineBackground(kind: DiffLine["kind"], variant: Variant): string {
  switch (kind) {
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

function getLineNumColor(kind: DiffLine["kind"], variant: Variant): string {
  switch (kind) {
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

function getLinePrefix(kind: DiffLine["kind"]): string {
  switch (kind) {
    case "addition":
      return "+";
    case "deletion":
      return "-";
    default:
      return " ";
  }
}

function getPrefixColor(kind: DiffLine["kind"], variant: Variant): string {
  switch (kind) {
    case "addition":
      return variant === "conflict" ? "var(--status-info)" : "var(--status-success)";
    case "deletion":
      return "var(--status-error)";
    default:
      return "transparent";
  }
}

// ============================================================================
// Line renderer
// ============================================================================

function renderDiffLine(
  line: DiffLine,
  index: number,
  wrapLines: boolean,
  variant: Variant
) {
  return (
    <div
      key={index}
      className="flex"
      style={{
        backgroundColor: getLineBackground(line.kind, variant),
        minHeight: "20px",
      }}
    >
      <div
        className="w-12 shrink-0 text-right pr-2 select-none z-10"
        style={{
          position: "sticky",
          left: 0,
          color: getLineNumColor(line.kind, variant),
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
          color: getLineNumColor(line.kind, variant),
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
          color: getPrefixColor(line.kind, variant),
          backgroundColor: "var(--bg-surface)",
        }}
      >
        {getLinePrefix(line.kind)}
      </div>

      <div
        className={`flex-1 pr-4 min-w-0 ${
          wrapLines ? "whitespace-pre-wrap break-all" : "whitespace-pre"
        }`}
        style={{
          color:
            line.kind === "deletion"
              ? "var(--text-muted)"
              : "var(--text-secondary)",
        }}
      >
        {line.content || " "}
      </div>
    </div>
  );
}

/** Render a fetched context range line as a DiffLine-like row. */
function renderRangeLine(
  rl: RangeLine,
  oldLineNum: number,
  wrapLines: boolean,
  variant: Variant
) {
  const line: DiffLine = {
    kind: "context",
    content: rl.content,
    oldLineNum,
    newLineNum: rl.lineNum,
  };
  return renderDiffLine(line, rl.lineNum, wrapLines, variant);
}

function renderHunkHeader(header: string) {
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
      {header}
    </div>
  );
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
}: SimpleDiffViewProps) {
  const [wrapLines, setWrapLines] = useState(true);
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

    return (
      <div
        key={`gap-${gapKey}`}
        className="px-3 py-1.5"
        style={{ borderBottom: "1px solid var(--overlay-faint)" }}
      >
        {/* Error state */}
        {state === "error" && (
          <div
            data-testid="gap-error"
            className="flex items-center gap-2 text-[0.6875rem]"
            style={{ color: "var(--text-muted)" }}
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
            className="text-[0.6875rem]"
            style={{ color: "var(--text-muted)" }}
          >
            Loading…
          </div>
        )}

        {/* Expanded fetched content */}
        {hasData && !isCollapsed && (
          <>
            {(state as RangeLine[]).map((rl, i) =>
              renderRangeLine(rl, fromOldLine + i, wrapLines, variant)
            )}
            <button
              type="button"
              aria-label="Hide unchanged lines"
              className="mt-1 text-[0.6875rem] hover:underline"
              style={{ color: "var(--text-muted)" }}
              onClick={() => collapseGap(gapKey)}
            >
              Hide unchanged lines
            </button>
          </>
        )}

        {/* Collapsed or not-yet-fetched */}
        {(!hasData || isCollapsed) && state !== "loading" && state !== "error" && (
          <>
            {canFetch ? (
              <button
                type="button"
                aria-label={`Show ${gapCount} unchanged lines`}
                className="text-[0.6875rem] hover:underline"
                style={{ color: "var(--text-muted)" }}
                onClick={() => expandGap(gapKey, fromNewLine, toNewLine)}
              >
                Show {gapCount} unchanged lines
              </button>
            ) : (
              <span
                className="text-[0.6875rem]"
                style={{ color: "var(--text-muted)" }}
              >
                {gapCount} unchanged lines
              </span>
            )}
          </>
        )}
      </div>
    );
  }

  // ── Main render ────────────────────────────────────────────────────────

  return (
    <div className="h-full overflow-y-auto">
      <div
        className="font-mono text-[0.8125rem] leading-[20px]"
        style={{ backgroundColor: "var(--bg-base)" }}
      >
        {/* Wrap toggle */}
        <div className="px-3 py-2 border-b" style={{ borderColor: "var(--overlay-weak)" }}>
          <Button
            variant="ghost"
            className="h-7 px-2 text-[0.6875rem]"
            onClick={() => setWrapLines((prev) => !prev)}
          >
            {wrapLines ? "Disable wrap" : "Wrap lines"}
          </Button>
        </div>

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
              {renderGap(gapKey, gapCount, gapFromNew, gapToNew, gapFromOld)}

              <div
                className="border-b"
                style={{ borderColor: "var(--overlay-faint)" }}
              >
                {renderHunkHeader(hunk.header)}
                <ScrollAreaPrimitive.Root className="w-full overflow-hidden">
                  <ScrollAreaPrimitive.Viewport className="w-full overflow-x-auto">
                    <div style={{ minWidth: wrapLines ? "auto" : "max-content" }}>
                      {hunk.lines.map((line, lineIdx) =>
                        renderDiffLine(line, lineIdx, wrapLines, variant)
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
        {(() => {
          const lastHunk = hunks[hunks.length - 1]!;
          const trailingFromNew = lastHunk.newStart + lastHunk.newLines;
          const trailingToNew = newTotalLines;
          const trailingFromOld = lastHunk.oldStart + lastHunk.oldLines;
          const trailingCount = newTotalLines - lastHunk.newStart - lastHunk.newLines + 1;
          return renderGap("post", trailingCount, trailingFromNew, trailingToNew, trailingFromOld);
        })()}
      </div>
    </div>
  );
}
