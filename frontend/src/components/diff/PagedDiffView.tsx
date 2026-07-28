import {
  useCallback,
  useEffect,
  useLayoutEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import {
  Virtuoso,
  type Components,
  type ListRange,
  type ScrollSeekConfiguration,
} from "react-virtuoso";
import { diffApi } from "@/api/diff";
import type {
  DiffPageRow,
  DiffRefKind,
  FileDiffPage,
  PrDiffAnnotation,
  WorkspaceReviewHunkAnnotation,
} from "@/api/diff";
import { Button } from "@/components/ui/button";
import {
  annotationsForLine,
  buildAnnotationIndex,
  buildHunkAnnotationIndex,
  hunkAnnotationsForHunk,
  renderDiffLine,
  renderHunkHeader,
  renderHunkAnnotationRows,
} from "./diffRenderHelpers";

export const DIFF_PAGE_SIZE = 200;
const PAGE_WINDOW_RADIUS = 1;
const PAGE_FETCH_RADIUS = 1;
const DIFF_ROW_ESTIMATED_HEIGHT = 20;
const CONTAINED_VIEWPORT_INCREASE = { top: 320, bottom: 640 };
const INLINE_VIEWPORT_INCREASE = { top: 800, bottom: 1200 };
const PLACEHOLDER_ROWS = 12;
const SCROLL_SEEK_ENTER_VELOCITY = 900;
const SCROLL_SEEK_EXIT_VELOCITY = 120;

export interface PagedDiffViewProps {
  conversationId: string;
  filePath: string;
  refKind: DiffRefKind;
  annotations?: PrDiffAnnotation[] | undefined;
  hunkAnnotations?: WorkspaceReviewHunkAnnotation[] | undefined;
  pageSize?: number | undefined;
  scrollContainer?: boolean | undefined;
  inlineScrollParent?: ScrollContainer | null | undefined;
  defaultWrapLines?: boolean | undefined;
  initialTotalRows?: number | undefined;
  initialIsBinary?: boolean | undefined;
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

function findScrollContainer(element: HTMLElement): ScrollContainer {
  let current = element.parentElement;
  while (current) {
    const overflowY = window.getComputedStyle(current).overflowY;
    const explicitOverflowY = current.style.overflowY;
    const canScroll =
      overflowY === "auto" ||
      overflowY === "scroll" ||
      overflowY === "overlay";
    const hasExplicitScrollY =
      explicitOverflowY === "auto" ||
      explicitOverflowY === "scroll" ||
      explicitOverflowY === "overlay";
    const hasScrollableContent = current.scrollHeight > current.clientHeight;
    if (canScroll && (hasExplicitScrollY || hasScrollableContent)) {
      return current;
    }
    current = current.parentElement;
  }
  return window;
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
  hunkAnnotations = [],
  pageSize = DIFF_PAGE_SIZE,
  scrollContainer = false,
  inlineScrollParent,
  defaultWrapLines = true,
  initialTotalRows,
  initialIsBinary,
}: PagedDiffViewProps) {
  const pagesRef = useRef<Map<number, FileDiffPage>>(new Map());
  const loadingOffsetsRef = useRef<Set<number>>(new Set());
  const generationRef = useRef(0);
  const inlineListRef = useRef<HTMLDivElement | null>(null);
  const initialTotalRowsRef = useRef(initialTotalRows);
  const [pages, setPages] = useState<Map<number, FileDiffPage>>(() => new Map());
  const [totalRows, setTotalRows] = useState<number | null>(
    () => initialTotalRows ?? null
  );
  const [initialError, setInitialError] = useState<Error | null>(null);
  const [isInitialLoading, setIsInitialLoading] = useState(true);
  const [inlineScrollContainer, setInlineScrollContainer] =
    useState<ScrollContainer | null>(null);
  const [renderedRange, setRenderedRange] = useState<ListRange | null>(null);
  const [wrapLines, setWrapLines] = useState(defaultWrapLines);
  const annotationIndex = useMemo(() => buildAnnotationIndex(annotations), [annotations]);
  const hunkAnnotationIndex = useMemo(
    () => buildHunkAnnotationIndex(hunkAnnotations),
    [hunkAnnotations]
  );
  const cacheKey = refKindCacheKey(refKind);
  const firstPage = pages.get(0);
  const rowCount = totalRows ?? firstPage?.totalRows ?? 0;
  const isBinary = firstPage?.isBinary ?? initialIsBinary === true;
  const hasAnyPage = pages.size > 0;
  const hasExplicitInlineScrollParent = inlineScrollParent !== undefined;
  const resolvedInlineScrollContainer = hasExplicitInlineScrollParent
    ? inlineScrollParent
    : inlineScrollContainer;
  const virtuosoComponents = useMemo<Components>(
    () => ({
      ScrollSeekPlaceholder: ({ height }) => (
        <div
          data-testid="paged-diff-scroll-seek-placeholder-row"
          style={{
            height: Math.max(height, DIFF_ROW_ESTIMATED_HEIGHT),
            backgroundColor: "var(--bg-base)",
          }}
        />
      ),
    }),
    [],
  );
  const scrollSeekConfiguration = useMemo<ScrollSeekConfiguration>(
    () => ({
      enter: (velocity) => Math.abs(velocity) > SCROLL_SEEK_ENTER_VELOCITY,
      exit: (velocity) => Math.abs(velocity) < SCROLL_SEEK_EXIT_VELOCITY,
    }),
    [],
  );

  useEffect(() => {
    initialTotalRowsRef.current = initialTotalRows;
    if (initialTotalRows !== undefined) {
      setTotalRows((current) => current ?? initialTotalRows);
    }
  }, [initialTotalRows]);

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
    [conversationId, filePath, pageSize, refKind]
  );

  useEffect(() => {
    generationRef.current += 1;
    const generation = generationRef.current;
    pagesRef.current = new Map();
    loadingOffsetsRef.current = new Set();
    setPages(new Map());
    setTotalRows(initialTotalRowsRef.current ?? null);
    setInitialError(null);
    setIsInitialLoading(true);
    if (!hasExplicitInlineScrollParent) {
      setInlineScrollContainer(null);
    }
    setRenderedRange(null);
    void loadPage(0, { generation });
  }, [
    cacheKey,
    conversationId,
    filePath,
    hasExplicitInlineScrollParent,
    loadPage,
  ]);

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
      setRenderedRange((current) =>
        current?.startIndex === range.startIndex &&
        current.endIndex === range.endIndex
          ? current
          : range
      );
      if (scrollContainer) {
        prunePagesAroundRange(range);
      }
      if (rowCount === 0) {
        return;
      }
      const expandedRange = {
        startIndex: Math.max(0, range.startIndex - PAGE_FETCH_RADIUS * pageSize),
        endIndex: Math.min(
          rowCount - 1,
          range.endIndex + PAGE_FETCH_RADIUS * pageSize
        ),
      };
      for (const offset of pageOffsetsForRange(expandedRange, pageSize)) {
        void loadPage(offset);
      }
    },
    [loadPage, pageSize, prunePagesAroundRange, rowCount, scrollContainer]
  );

  useLayoutEffect(() => {
    if (scrollContainer || hasExplicitInlineScrollParent) {
      setInlineScrollContainer(null);
      return;
    }
    const element = inlineListRef.current;
    if (!element) {
      return;
    }
    const nextScrollContainer = findScrollContainer(element);
    setInlineScrollContainer((current) =>
      current === nextScrollContainer ? current : nextScrollContainer
    );
  }, [hasExplicitInlineScrollParent, rowCount, scrollContainer]);

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

  if (isBinary) {
    return (
      <div
        className="flex items-center justify-center py-8"
        style={{ color: "var(--text-muted)" }}
      >
        <p className="text-sm">Binary file — diff not shown</p>
      </div>
    );
  }

  if (isInitialLoading && !firstPage && totalRows === null) {
    return (
      <div
        data-testid="paged-diff-loading"
        role="status"
        aria-busy="true"
        className="space-y-px px-3 py-3"
        style={{ backgroundColor: "var(--bg-base)" }}
      >
        <span className="sr-only">Loading diff rows</span>
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
          {renderHunkAnnotationRows(matchedHunkAnnotations, wrapLines, "standard")}
        </>
      );
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

  const mountedPageCount = renderedRange
    ? pageOffsetsForRange(
        {
          startIndex: Math.max(0, Math.min(renderedRange.startIndex, rowCount - 1)),
          endIndex: Math.max(0, Math.min(renderedRange.endIndex, rowCount - 1)),
        },
        pageSize
      ).length
    : 0;
  const inlineCustomScrollParent =
    !scrollContainer &&
    resolvedInlineScrollContainer &&
    !isWindowScrollContainer(resolvedInlineScrollContainer)
      ? resolvedInlineScrollContainer
      : undefined;
  const useInlineWindowScroll =
    !scrollContainer &&
    !hasExplicitInlineScrollParent &&
    (resolvedInlineScrollContainer === null ||
      isWindowScrollContainer(resolvedInlineScrollContainer));
  const isInlineScrollParentPending =
    !scrollContainer &&
    hasExplicitInlineScrollParent &&
    resolvedInlineScrollContainer === null;
  const virtualRows = (
    <Virtuoso
      totalCount={rowCount}
      components={virtuosoComponents}
      data-testid="paged-diff-virtual-list"
      style={
        scrollContainer
          ? {
              height: "100%",
              overflowX: "auto",
            }
          : {
              overflowX: "visible",
            }
      }
      {...(inlineCustomScrollParent
        ? { customScrollParent: inlineCustomScrollParent }
        : {})}
      {...(useInlineWindowScroll ? { useWindowScroll: true } : {})}
      defaultItemHeight={DIFF_ROW_ESTIMATED_HEIGHT}
      rangeChanged={handleRangeChanged}
      scrollSeekConfiguration={scrollSeekConfiguration}
      increaseViewportBy={
        scrollContainer ? CONTAINED_VIEWPORT_INCREASE : INLINE_VIEWPORT_INCREASE
      }
      computeItemKey={(index) => index}
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
  );

  return (
    <div className={scrollContainer ? "h-full overflow-hidden" : "w-full overflow-hidden"}>
      <div
        className="font-mono text-[0.8125rem] leading-[20px]"
        data-testid="paged-diff-view"
        data-loaded-page-count={pages.size}
        data-mounted-page-count={mountedPageCount}
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
          virtualRows
        ) : (
          <div
            ref={inlineListRef}
            data-testid="paged-diff-inline-list"
            className="overflow-x-auto"
          >
            {isInlineScrollParentPending ? (
              <div
                data-testid="paged-diff-scroll-parent-pending"
                style={{
                  height:
                    rowCount * DIFF_ROW_ESTIMATED_HEIGHT,
                  backgroundColor: "var(--bg-base)",
                }}
              />
            ) : (
              virtualRows
            )}
          </div>
        )}
      </div>
    </div>
  );
}
