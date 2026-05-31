import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { Virtuoso, type ListRange } from "react-virtuoso";
import { diffApi } from "@/api/diff";
import type { DiffPageRow, DiffRefKind, FileDiffPage, PrDiffAnnotation } from "@/api/diff";
import { Button } from "@/components/ui/button";
import {
  annotationsForLine,
  buildAnnotationIndex,
  renderDiffLine,
  renderHunkHeader,
} from "./diffRenderHelpers";

export const DIFF_PAGE_SIZE = 200;
const PAGE_WINDOW_RADIUS = 1;
const DIFF_ROW_ESTIMATED_HEIGHT = 20;
const PLACEHOLDER_ROWS = 12;

export interface PagedDiffViewProps {
  conversationId: string;
  filePath: string;
  refKind: DiffRefKind;
  annotations?: PrDiffAnnotation[] | undefined;
  pageSize?: number | undefined;
  scrollContainer?: boolean | undefined;
  defaultWrapLines?: boolean | undefined;
}

function refKindCacheKey(refKind: DiffRefKind): string {
  return refKind.kind === "commit" ? `${refKind.kind}:${refKind.sha}` : refKind.kind;
}

function pageOffsetForIndex(index: number, pageSize: number): number {
  return Math.floor(Math.max(0, index) / pageSize) * pageSize;
}

function pageOffsetsForRange(range: ListRange, pageSize: number): number[] {
  const start = pageOffsetForIndex(range.startIndex, pageSize);
  const end = pageOffsetForIndex(range.endIndex, pageSize);
  const offsets: number[] = [];
  for (let offset = start; offset <= end; offset += pageSize) {
    offsets.push(offset);
  }
  return offsets;
}

function rowAtIndex(
  pages: Map<number, FileDiffPage>,
  index: number,
  pageSize: number
): DiffPageRow | undefined {
  const offset = pageOffsetForIndex(index, pageSize);
  return pages.get(offset)?.rows[index - offset];
}

function prunePagesToOffsetRange(
  pages: Map<number, FileDiffPage>,
  firstKeptOffset: number,
  lastKeptOffset: number
): boolean {
  let changed = false;
  for (const offset of pages.keys()) {
    if (offset < firstKeptOffset || offset > lastKeptOffset) {
      pages.delete(offset);
      changed = true;
    }
  }
  return changed;
}

type LoadPageOptions = {
  generation?: number;
  keepOffsetRange?: {
    first: number;
    last: number;
  };
};

export function PagedDiffView({
  conversationId,
  filePath,
  refKind,
  annotations = [],
  pageSize = DIFF_PAGE_SIZE,
  scrollContainer = false,
  defaultWrapLines = true,
}: PagedDiffViewProps) {
  const pagesRef = useRef<Map<number, FileDiffPage>>(new Map());
  const loadingOffsetsRef = useRef<Set<number>>(new Set());
  const generationRef = useRef(0);
  const previousSentinelRef = useRef<HTMLDivElement | null>(null);
  const nextSentinelRef = useRef<HTMLDivElement | null>(null);
  const [pages, setPages] = useState<Map<number, FileDiffPage>>(() => new Map());
  const [totalRows, setTotalRows] = useState<number | null>(null);
  const [initialError, setInitialError] = useState<Error | null>(null);
  const [isInitialLoading, setIsInitialLoading] = useState(true);
  const [wrapLines, setWrapLines] = useState(defaultWrapLines);
  const annotationIndex = useMemo(() => buildAnnotationIndex(annotations), [annotations]);
  const cacheKey = refKindCacheKey(refKind);

  const loadPage = useCallback(
    async (requestedOffset: number, options: LoadPageOptions = {}) => {
      const generation = options.generation ?? generationRef.current;
      const offset = pageOffsetForIndex(requestedOffset, pageSize);
      if (pagesRef.current.has(offset)) {
        if (
          options.keepOffsetRange &&
          prunePagesToOffsetRange(
            pagesRef.current,
            options.keepOffsetRange.first,
            options.keepOffsetRange.last
          )
        ) {
          setPages(new Map(pagesRef.current));
        }
        return;
      }
      if (loadingOffsetsRef.current.has(offset)) {
        return;
      }

      loadingOffsetsRef.current.add(offset);
      if (offset === 0) {
        setIsInitialLoading(true);
      }

      try {
        const page = await diffApi.getAgentConversationWorkspaceFileDiffPage({
          conversationId,
          path: filePath,
          refKind,
          offset,
          limit: pageSize,
        });
        if (generation !== generationRef.current) {
          return;
        }
        pagesRef.current.set(page.offset, page);
        if (options.keepOffsetRange) {
          prunePagesToOffsetRange(
            pagesRef.current,
            options.keepOffsetRange.first,
            options.keepOffsetRange.last
          );
        }
        setPages(new Map(pagesRef.current));
        setTotalRows(page.totalRows);
        setInitialError(null);
      } catch (error) {
        if (generation === generationRef.current && offset === 0) {
          setInitialError(error instanceof Error ? error : new Error(String(error)));
        }
      } finally {
        if (generation === generationRef.current) {
          loadingOffsetsRef.current.delete(offset);
          if (offset === 0) {
            setIsInitialLoading(false);
          }
        }
      }
    },
    [conversationId, filePath, pageSize, refKind]
  );

  useEffect(() => {
    generationRef.current += 1;
    const generation = generationRef.current;
    pagesRef.current = new Map();
    loadingOffsetsRef.current = new Set();
    setPages(new Map());
    setTotalRows(null);
    setInitialError(null);
    setIsInitialLoading(true);
    void loadPage(0, { generation });
  }, [cacheKey, conversationId, filePath, loadPage]);

  const prunePagesAroundRange = useCallback(
    (range: ListRange) => {
      const firstVisibleOffset = pageOffsetForIndex(range.startIndex, pageSize);
      const lastVisibleOffset = pageOffsetForIndex(range.endIndex, pageSize);
      const firstKeptOffset = Math.max(
        0,
        firstVisibleOffset - PAGE_WINDOW_RADIUS * pageSize
      );
      const lastKeptOffset = lastVisibleOffset + PAGE_WINDOW_RADIUS * pageSize;
      let changed = false;
      for (const offset of pagesRef.current.keys()) {
        if (offset < firstKeptOffset || offset > lastKeptOffset) {
          pagesRef.current.delete(offset);
          changed = true;
        }
      }
      if (changed) {
        setPages(new Map(pagesRef.current));
      }
    },
    [pageSize]
  );

  const handleRangeChanged = useCallback(
    (range: ListRange) => {
      prunePagesAroundRange(range);
      for (const offset of pageOffsetsForRange(range, pageSize)) {
        void loadPage(offset);
      }
    },
    [loadPage, pageSize, prunePagesAroundRange]
  );

  const firstPage = pages.get(0);
  const rowCount = totalRows ?? firstPage?.totalRows ?? 0;
  const hasAnyPage = pages.size > 0;
  const sortedPageOffsets = useMemo(
    () => [...pages.keys()].sort((a, b) => a - b),
    [pages]
  );
  const firstLoadedOffset = sortedPageOffsets[0] ?? 0;
  const lastLoadedOffset = sortedPageOffsets[sortedPageOffsets.length - 1] ?? 0;
  const lastLoadedPage = pages.get(lastLoadedOffset);
  const previousOffset = firstLoadedOffset > 0
    ? Math.max(0, firstLoadedOffset - pageSize)
    : null;
  const nextOffset = lastLoadedPage?.nextOffset ?? null;

  useEffect(() => {
    if (scrollContainer || typeof IntersectionObserver === "undefined") {
      return;
    }

    const observer = new IntersectionObserver(
      (entries) => {
        for (const entry of entries) {
          if (!entry.isIntersecting) continue;
          if (entry.target === previousSentinelRef.current && previousOffset !== null) {
            void loadPage(previousOffset, {
              keepOffsetRange: {
                first: previousOffset,
                last: previousOffset + pageSize,
              },
            });
          }
          if (entry.target === nextSentinelRef.current && nextOffset !== null) {
            void loadPage(nextOffset, {
              keepOffsetRange: {
                first: Math.max(0, nextOffset - pageSize),
                last: nextOffset,
              },
            });
          }
        }
      },
      { root: null, rootMargin: "640px 0px 640px 0px" }
    );

    const previousSentinel = previousSentinelRef.current;
    const nextSentinel = nextSentinelRef.current;
    if (previousOffset !== null && previousSentinel) {
      observer.observe(previousSentinel);
    }
    if (nextOffset !== null && nextSentinel) {
      observer.observe(nextSentinel);
    }

    return () => observer.disconnect();
  }, [loadPage, nextOffset, pageSize, previousOffset, scrollContainer]);

  if (initialError && !hasAnyPage) {
    return (
      <div
        data-testid="paged-diff-error"
        className="flex flex-col items-center gap-2 py-6 text-xs"
        style={{ color: "var(--text-muted)" }}
      >
        <p>Could not load diff rows.</p>
        <button
          type="button"
          aria-label="Retry loading diff rows"
          className="rounded px-2 py-1 text-xs hover:bg-[var(--bg-hover)]"
          style={{ color: "var(--text-secondary)" }}
          onClick={() => void loadPage(0)}
        >
          Retry
        </button>
      </div>
    );
  }

  if (isInitialLoading && !firstPage) {
    return (
      <div
        data-testid="paged-diff-loading"
        className="space-y-px px-3 py-3"
        style={{ backgroundColor: "var(--bg-base)" }}
      >
        {Array.from({ length: PLACEHOLDER_ROWS }).map((_, index) => (
          <div
            key={index}
            className="h-4 rounded"
            style={{ backgroundColor: "var(--bg-subtle)" }}
          />
        ))}
      </div>
    );
  }

  if (firstPage?.isBinary) {
    return (
      <div
        className="flex items-center justify-center py-8"
        style={{ color: "var(--text-muted)" }}
      >
        <p className="text-sm">Binary file — diff not shown</p>
      </div>
    );
  }

  if (rowCount === 0) {
    return (
      <div
        className="flex items-center justify-center py-8"
        style={{ color: "var(--text-muted)" }}
      >
        <p className="text-sm">No changes</p>
      </div>
    );
  }

  const renderRow = (row: DiffPageRow, index: number) => {
    if (row.kind === "hunk_header") {
      return renderHunkHeader(row.header);
    }
    return renderDiffLine(
      row.line,
      index,
      wrapLines,
      "standard",
      annotationsForLine(annotationIndex, row.line)
    );
  };

  const topSpacerHeight = firstLoadedOffset * DIFF_ROW_ESTIMATED_HEIGHT;
  const lastLoadedRowEnd =
    lastLoadedPage !== undefined
      ? lastLoadedOffset + lastLoadedPage.rows.length
      : 0;
  const bottomSpacerHeight =
    Math.max(0, rowCount - lastLoadedRowEnd) * DIFF_ROW_ESTIMATED_HEIGHT;

  return (
    <div className={scrollContainer ? "h-full overflow-hidden" : "w-full overflow-hidden"}>
      <div
        className="font-mono text-[0.8125rem] leading-[20px]"
        data-testid="paged-diff-view"
        data-loaded-page-count={pages.size}
        data-scroll-container={String(scrollContainer)}
        data-total-rows={rowCount}
        data-wrap-lines={wrapLines}
        style={{ backgroundColor: "var(--bg-base)" }}
      >
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
        {scrollContainer ? (
          <Virtuoso
            totalCount={rowCount}
            data-testid="paged-diff-virtual-list"
            style={{
              height: "100%",
              overflowX: "auto",
            }}
            rangeChanged={handleRangeChanged}
            increaseViewportBy={{ top: 320, bottom: 640 }}
            computeItemKey={(index) => {
              const row = rowAtIndex(pages, index, pageSize);
              if (!row) return `placeholder-${index}`;
              return row.kind === "hunk_header"
                ? `hunk-${index}-${row.header}`
                : `line-${index}-${row.line.oldLineNum ?? "x"}-${row.line.newLineNum ?? "x"}`;
            }}
            itemContent={(index) => {
              const row = rowAtIndex(pages, index, pageSize);
              if (!row) {
                return (
                  <div
                    data-testid="paged-diff-placeholder-row"
                    className="h-5"
                    style={{ backgroundColor: "var(--bg-base)" }}
                  />
                );
              }
              return renderRow(row, index);
            }}
          />
        ) : (
          <div data-testid="paged-diff-inline-list" className="overflow-x-auto">
            {topSpacerHeight > 0 && (
              <div
                data-testid="paged-diff-top-spacer"
                aria-hidden="true"
                style={{ height: `${topSpacerHeight}px` }}
              />
            )}
            {previousOffset !== null && (
              <div
                ref={previousSentinelRef}
                data-testid="paged-diff-previous-sentinel"
                aria-hidden="true"
                className="h-px"
              />
            )}
            {sortedPageOffsets.flatMap((offset) =>
              (pages.get(offset)?.rows ?? []).map((row, index) => {
                const absoluteIndex = offset + index;
                return (
                  <div key={`${offset}-${index}`}>
                    {renderRow(row, absoluteIndex)}
                  </div>
                );
              })
            )}
            {nextOffset !== null && (
              <div
                ref={nextSentinelRef}
                data-testid="paged-diff-next-sentinel"
                aria-hidden="true"
                className="h-5"
                style={{ backgroundColor: "var(--bg-base)" }}
              />
            )}
            {bottomSpacerHeight > 0 && (
              <div
                data-testid="paged-diff-bottom-spacer"
                aria-hidden="true"
                style={{ height: `${bottomSpacerHeight}px` }}
              />
            )}
          </div>
        )}
      </div>
    </div>
  );
}
