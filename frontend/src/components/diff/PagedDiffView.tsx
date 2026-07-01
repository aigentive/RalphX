import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from "react";
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

type LoadPageOptions = {
  generation?: number;
};

type ScrollContainer = HTMLElement | Window;

interface PendingScrollAnchor {
  offset: number;
  top: number;
  scrollContainer: ScrollContainer;
}

interface MeasuredPageBlockProps {
  offset: number;
  children: ReactNode;
  onHeightChange: (offset: number, height: number) => void;
}

function MeasuredPageBlock({
  offset,
  children,
  onHeightChange,
}: MeasuredPageBlockProps) {
  const pageRef = useRef<HTMLDivElement | null>(null);

  useLayoutEffect(() => {
    const element = pageRef.current;
    if (!element) {
      return undefined;
    }

    const reportHeight = (height: number) => {
      if (height > 0) {
        onHeightChange(offset, height);
      }
    };
    reportHeight(element.getBoundingClientRect().height);

    if (typeof ResizeObserver === "undefined") {
      return undefined;
    }

    const observer = new ResizeObserver((entries) => {
      const entry = entries[0];
      reportHeight(entry?.contentRect.height ?? element.getBoundingClientRect().height);
    });
    observer.observe(element);
    return () => observer.disconnect();
  }, [offset, onHeightChange]);

  return (
    <div ref={pageRef} data-testid="paged-diff-page" data-page-offset={offset}>
      {children}
    </div>
  );
}

function findScrollContainer(element: HTMLElement): ScrollContainer {
  let current = element.parentElement;
  while (current) {
    const overflowY = window.getComputedStyle(current).overflowY;
    const canScroll =
      overflowY === "auto" ||
      overflowY === "scroll" ||
      overflowY === "overlay";
    if (canScroll && current.scrollHeight > current.clientHeight) {
      return current;
    }
    current = current.parentElement;
  }
  return window;
}

function scrollByDelta(scrollContainer: ScrollContainer, delta: number) {
  if (Math.abs(delta) < 1) {
    return;
  }
  if (isWindowScrollContainer(scrollContainer)) {
    window.scrollBy(0, delta);
    return;
  }
  scrollContainer.scrollTop += delta;
}

function isWindowScrollContainer(
  scrollContainer: ScrollContainer
): scrollContainer is Window {
  return scrollContainer === window;
}

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
  const inlineListRef = useRef<HTMLDivElement | null>(null);
  const firstMountedOffsetRef = useRef(0);
  const pendingScrollAnchorRef = useRef<PendingScrollAnchor | null>(null);
  const [pages, setPages] = useState<Map<number, FileDiffPage>>(() => new Map());
  const [totalRows, setTotalRows] = useState<number | null>(null);
  const [initialError, setInitialError] = useState<Error | null>(null);
  const [isInitialLoading, setIsInitialLoading] = useState(true);
  const [mountedCenterOffset, setMountedCenterOffset] = useState(0);
  const [pageHeights, setPageHeights] = useState<Map<number, number>>(
    () => new Map()
  );
  const [wrapLines, setWrapLines] = useState(defaultWrapLines);
  const annotationIndex = useMemo(() => buildAnnotationIndex(annotations), [annotations]);
  const cacheKey = refKindCacheKey(refKind);

  const captureInlineScrollAnchor = useCallback(
    (anchorOffset: number) => {
      if (scrollContainer) {
        return;
      }
      const anchorElement = inlineListRef.current?.querySelector<HTMLElement>(
        `[data-page-offset="${anchorOffset}"]`
      );
      if (!anchorElement) {
        return;
      }
      pendingScrollAnchorRef.current = {
        offset: anchorOffset,
        top: anchorElement.getBoundingClientRect().top,
        scrollContainer: findScrollContainer(anchorElement),
      };
    },
    [scrollContainer]
  );

  const loadPage = useCallback(
    async (requestedOffset: number, options: LoadPageOptions = {}) => {
      const generation = options.generation ?? generationRef.current;
      const offset = pageOffsetForIndex(requestedOffset, pageSize);
      if (pagesRef.current.has(offset)) {
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
        if (!scrollContainer && offset < firstMountedOffsetRef.current) {
          captureInlineScrollAnchor(firstMountedOffsetRef.current);
        }
        pagesRef.current.set(page.offset, page);
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
    [captureInlineScrollAnchor, conversationId, filePath, pageSize, refKind, scrollContainer]
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
    setMountedCenterOffset(0);
    setPageHeights(new Map());
    void loadPage(0, { generation });
  }, [cacheKey, conversationId, filePath, loadPage]);

  const recordPageHeight = useCallback((offset: number, height: number) => {
    setPageHeights((previous) => {
      const previousHeight = previous.get(offset);
      if (
        previousHeight !== undefined &&
        Math.abs(previousHeight - height) < 1
      ) {
        return previous;
      }
      const next = new Map(previous);
      next.set(offset, height);
      return next;
    });
  }, []);

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
  const mountedWindowFirstOffset = Math.max(
    0,
    mountedCenterOffset - PAGE_WINDOW_RADIUS * pageSize
  );
  const mountedWindowLastOffset =
    mountedCenterOffset + PAGE_WINDOW_RADIUS * pageSize;
  const mountedPageOffsets = useMemo(
    () =>
      scrollContainer
        ? sortedPageOffsets
        : sortedPageOffsets.filter(
            (offset) =>
              offset >= mountedWindowFirstOffset && offset <= mountedWindowLastOffset
          ),
    [mountedWindowFirstOffset, mountedWindowLastOffset, scrollContainer, sortedPageOffsets]
  );
  const firstLoadedOffset = sortedPageOffsets[0] ?? 0;
  const lastLoadedOffset = sortedPageOffsets[sortedPageOffsets.length - 1] ?? 0;
  const firstMountedOffset = mountedPageOffsets[0] ?? firstLoadedOffset;
  const lastMountedOffset =
    mountedPageOffsets[mountedPageOffsets.length - 1] ?? lastLoadedOffset;
  const lastMountedPage = pages.get(lastMountedOffset);
  const previousOffset = firstMountedOffset > 0
    ? Math.max(0, firstMountedOffset - pageSize)
    : null;
  const nextOffset = lastMountedPage?.nextOffset ?? null;

  useLayoutEffect(() => {
    firstMountedOffsetRef.current = firstMountedOffset;
  }, [firstMountedOffset]);

  useEffect(() => {
    if (scrollContainer || typeof IntersectionObserver === "undefined") {
      return;
    }

    const observer = new IntersectionObserver(
      (entries) => {
        for (const entry of entries) {
          if (!entry.isIntersecting) continue;
          if (entry.target === previousSentinelRef.current && previousOffset !== null) {
            captureInlineScrollAnchor(firstMountedOffset);
            setMountedCenterOffset(previousOffset);
            void loadPage(previousOffset);
          }
          if (entry.target === nextSentinelRef.current && nextOffset !== null) {
            setMountedCenterOffset(nextOffset);
            void loadPage(nextOffset);
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
  }, [
    captureInlineScrollAnchor,
    firstMountedOffset,
    loadPage,
    nextOffset,
    pageSize,
    previousOffset,
    scrollContainer,
  ]);

  useLayoutEffect(() => {
    const anchor = pendingScrollAnchorRef.current;
    if (!anchor) {
      return;
    }
    pendingScrollAnchorRef.current = null;
    const anchorElement = inlineListRef.current?.querySelector<HTMLElement>(
      `[data-page-offset="${anchor.offset}"]`
    );
    if (!anchorElement) {
      return;
    }
    const nextTop = anchorElement.getBoundingClientRect().top;
    scrollByDelta(anchor.scrollContainer, nextTop - anchor.top);
  });

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
      annotationsForLine(annotationIndex, row.line),
      { stickyGutter: false },
    );
  };

  const lastMountedRowEnd =
    lastMountedPage !== undefined
      ? lastMountedOffset + lastMountedPage.rows.length
      : 0;
  const spacerHeightForRowRange = (startIndex: number, endIndex: number) => {
    const start = Math.max(0, Math.min(startIndex, rowCount));
    const end = Math.max(start, Math.min(endIndex, rowCount));
    let height = 0;
    let cursor = start;
    while (cursor < end) {
      const pageOffset = pageOffsetForIndex(cursor, pageSize);
      const pageStart = pageOffset;
      const pageEnd = Math.min(rowCount, pageOffset + pageSize);
      const rangeEnd = Math.min(end, pageEnd);
      const coversWholePage = cursor === pageStart && rangeEnd === pageEnd;
      const measuredHeight = coversWholePage
        ? pageHeights.get(pageOffset)
        : undefined;
      height +=
        measuredHeight ?? (rangeEnd - cursor) * DIFF_ROW_ESTIMATED_HEIGHT;
      cursor = rangeEnd;
    }
    return height;
  };

  const topSpacerHeight = spacerHeightForRowRange(0, firstMountedOffset);
  const bottomSpacerHeight = spacerHeightForRowRange(lastMountedRowEnd, rowCount);

  return (
    <div className={scrollContainer ? "h-full overflow-hidden" : "w-full overflow-hidden"}>
      <div
        className="font-mono text-[0.8125rem] leading-[20px]"
        data-testid="paged-diff-view"
        data-loaded-page-count={pages.size}
        data-mounted-page-count={scrollContainer ? pages.size : mountedPageOffsets.length}
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
          <div
            ref={inlineListRef}
            data-testid="paged-diff-inline-list"
            className="overflow-x-auto"
          >
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
            {mountedPageOffsets.map((offset) => (
              <MeasuredPageBlock
                key={offset}
                offset={offset}
                onHeightChange={recordPageHeight}
              >
                {(pages.get(offset)?.rows ?? []).map((row, index) => {
                  const absoluteIndex = offset + index;
                  return (
                    <div key={`${offset}-${index}`}>
                      {renderRow(row, absoluteIndex)}
                    </div>
                  );
                })}
              </MeasuredPageBlock>
            ))}
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
