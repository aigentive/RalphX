import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { FileDiffPage } from "@/api/diff";

const mockGetDiffPage = vi.fn();
let latestSetVirtuosoRange:
  | ((range: { startIndex: number; endIndex: number }) => void)
  | undefined;

vi.mock("@/api/diff", () => ({
  diffApi: {
    getAgentConversationWorkspaceFileDiffPage: (...args: unknown[]) =>
      mockGetDiffPage(...args),
  },
}));

vi.mock("react-virtuoso", async () => {
  const React = await vi.importActual<typeof import("react")>("react");

  type VirtuosoMockProps = {
    totalCount?: number;
    itemContent?: (index: number) => React.ReactNode;
    rangeChanged?: (range: { startIndex: number; endIndex: number }) => void;
    computeItemKey?: (index: number) => React.Key;
    style?: React.CSSProperties;
    customScrollParent?: HTMLElement;
    useWindowScroll?: boolean;
    defaultItemHeight?: number;
    components?: unknown;
    increaseViewportBy?: number | { top?: number; bottom?: number };
    scrollSeekConfiguration?: unknown;
    "data-testid"?: string;
  };

  function Virtuoso(props: VirtuosoMockProps) {
    const { rangeChanged } = props;
    const initialEnd = Math.max(0, Math.min((props.totalCount ?? 0) - 1, 23));
    const [range, setRange] = React.useState({
      startIndex: 0,
      endIndex: initialEnd,
    });
    const rangeChangedRef = React.useRef(rangeChanged);
    rangeChangedRef.current = rangeChanged;

    React.useEffect(() => {
      rangeChanged?.(range);
    }, [rangeChanged, range]);

    React.useEffect(() => {
      latestSetVirtuosoRange = (nextRange) => {
        setRange(nextRange);
        rangeChangedRef.current?.(nextRange);
      };
      return () => {
        latestSetVirtuosoRange = undefined;
      };
    }, []);

    const totalCount = props.totalCount ?? 0;
    const startIndex = Math.max(0, Math.min(range.startIndex, totalCount));
    const endIndex = Math.max(
      startIndex - 1,
      Math.min(range.endIndex, totalCount - 1)
    );
    const increaseViewportBy = props.increaseViewportBy;
    const increaseTop =
      typeof increaseViewportBy === "number"
        ? increaseViewportBy
        : increaseViewportBy?.top ?? 0;
    const increaseBottom =
      typeof increaseViewportBy === "number"
        ? increaseViewportBy
        : increaseViewportBy?.bottom ?? 0;

    return (
      <div
        data-testid={props["data-testid"] ?? "mock-virtuoso"}
        data-count={totalCount}
        data-custom-scroll-parent={props.customScrollParent ? "true" : "false"}
        data-default-item-height={props.defaultItemHeight ?? ""}
        data-has-scroll-seek={String(Boolean(props.scrollSeekConfiguration))}
        data-increase-viewport-bottom={increaseBottom}
        data-increase-viewport-top={increaseTop}
        data-rendered-end={endIndex}
        data-rendered-start={startIndex}
        data-use-window-scroll={String(Boolean(props.useWindowScroll))}
        style={props.style}
      >
        {Array.from({ length: Math.max(0, endIndex - startIndex + 1) }).map(
          (_, rangeIndex) => {
            const index = startIndex + rangeIndex;
            return (
              <div key={props.computeItemKey?.(index) ?? index}>
                {props.itemContent?.(index)}
              </div>
            );
          }
        )}
      </div>
    );
  }

  return { Virtuoso };
});

import { PagedDiffView } from "./PagedDiffView";

function makePage(offset: number, limit: number, totalRows = 260): FileDiffPage {
  const rowCount = Math.max(0, Math.min(limit, totalRows - offset));
  return {
    filePath: "src/Huge.tsx",
    language: "typescript",
    rows: Array.from({ length: rowCount }).map((_, index) => {
      const rowIndex = offset + index;
      if (rowIndex === 0) {
        return {
          kind: "hunk_header" as const,
          header: "@@ -1,260 +1,260 @@",
          oldStart: 1,
          oldLines: 260,
          newStart: 1,
          newLines: 260,
        };
      }
      return {
        kind: "line" as const,
        line: {
          kind: "addition" as const,
          content: `line ${rowIndex}`,
          oldLineNum: null,
          newLineNum: rowIndex,
        },
      };
    }),
    offset,
    limit,
    nextOffset: offset + rowCount < totalRows ? offset + rowCount : null,
    totalRows,
    oldTotalLines: 0,
    newTotalLines: totalRows,
    isBinary: false,
  };
}

function createDeferredPage() {
  let resolve!: (page: FileDiffPage) => void;
  let reject!: (error: Error) => void;
  const promise = new Promise<FileDiffPage>((promiseResolve, promiseReject) => {
    resolve = promiseResolve;
    reject = promiseReject;
  });
  return { promise, reject, resolve };
}

async function setVirtuosoRange(startIndex: number, endIndex: number) {
  await act(async () => {
    latestSetVirtuosoRange?.({ startIndex, endIndex });
  });
}

function countPageFetches(offset: number): number {
  return mockGetDiffPage.mock.calls.filter(([args]) => {
    const pageArgs = args as { offset?: number };
    return pageArgs.offset === offset;
  }).length;
}

describe("PagedDiffView", () => {
  beforeEach(() => {
    latestSetVirtuosoRange = undefined;
    mockGetDiffPage.mockReset();
    mockGetDiffPage.mockImplementation(({ offset, limit }) =>
      Promise.resolve(makePage(offset as number, limit as number))
    );
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("announces the initial paged loading state", () => {
    mockGetDiffPage.mockImplementation(() => new Promise(() => {}));

    render(
      <PagedDiffView
        conversationId="conv-loading"
        filePath="src/Loading.tsx"
        refKind={{ kind: "head" }}
      />
    );

    expect(screen.getByTestId("paged-diff-loading")).toHaveAttribute(
      "role",
      "status"
    );
    expect(screen.getByTestId("paged-diff-loading")).toHaveAttribute(
      "aria-busy",
      "true"
    );
    expect(screen.getByTestId("paged-diff-loading")).toHaveTextContent(
      "Loading diff rows"
    );
  });

  it("renders inline rows through Virtuoso attached to the outer scroll parent", async () => {
    render(
      <div data-testid="outer-scroll" style={{ overflowY: "auto" }}>
        <PagedDiffView
          conversationId="conv-1"
          filePath="src/Huge.tsx"
          refKind={{ kind: "head" }}
          pageSize={100}
        />
      </div>
    );

    await waitFor(() =>
      expect(mockGetDiffPage).toHaveBeenCalledWith({
        conversationId: "conv-1",
        path: "src/Huge.tsx",
        refKind: { kind: "head" },
        offset: 0,
        limit: 100,
      })
    );
    expect(await screen.findByTestId("paged-diff-view")).toHaveAttribute(
      "data-total-rows",
      "260"
    );
    expect(screen.getByTestId("paged-diff-view")).toHaveAttribute(
      "data-scroll-container",
      "false"
    );
    expect(screen.getByTestId("paged-diff-view")).toHaveAttribute(
      "data-loaded-page-count",
      "2"
    );
    expect(screen.getByTestId("paged-diff-view")).toHaveAttribute(
      "data-mounted-page-count",
      "1"
    );
    expect(screen.getByTestId("paged-diff-inline-list")).toBeInTheDocument();
    expect(screen.getByTestId("paged-diff-virtual-list")).toHaveAttribute(
      "data-custom-scroll-parent",
      "true"
    );
    expect(screen.getByTestId("paged-diff-virtual-list")).toHaveAttribute(
      "data-use-window-scroll",
      "false"
    );
    expect(screen.getByTestId("paged-diff-virtual-list")).toHaveAttribute(
      "data-has-scroll-seek",
      "true"
    );
    expect(screen.getByText("@@ -1,260 +1,260 @@")).toBeInTheDocument();
    expect(screen.getByText("line 1")).toBeInTheDocument();
    expect(screen.queryByText("line 120")).toBeNull();
  });

  it("renders workspace review hunk annotations below matching paged hunk headers", async () => {
    render(
      <PagedDiffView
        conversationId="conv-1"
        filePath="src/Huge.tsx"
        refKind={{ kind: "head" }}
        pageSize={100}
        hunkAnnotations={[
          {
            id: "workspace-review-hunk-1",
            conversationId: "conv-1",
            projectId: "project-1",
            artifactId: "artifact-1",
            artifactVersion: 1,
            targetScope: "selected_source",
            headSha: "head-sha",
            diffFingerprint: "fingerprint-1",
            path: "src/Huge.tsx",
            diffSource: "selected_source",
            hunkHeader: "@@ -1,260 +1,260 @@",
            oldStart: 1,
            oldLines: 260,
            newStart: 1,
            newLines: 260,
            title: "Review summary",
            message: "This hunk wires the paged renderer.",
            level: "notice",
            createdByRunId: "run-1",
            createdAt: "2026-07-01T00:00:00Z",
          },
        ]}
      />
    );

    expect(await screen.findByTestId("diff-hunk-annotation-row")).toBeInTheDocument();
    expect(screen.getByText("Workspace review")).toBeInTheDocument();
    expect(screen.getByText("Review summary")).toBeInTheDocument();
    expect(screen.getByText("This hunk wires the paged renderer.")).toBeInTheDocument();
  });

  it("waits for an explicit inline scroll parent instead of falling back to window scroll", async () => {
    render(
      <PagedDiffView
        conversationId="conv-1"
        filePath="src/Huge.tsx"
        refKind={{ kind: "head" }}
        pageSize={100}
        inlineScrollParent={null}
        initialTotalRows={260}
      />
    );

    expect(await screen.findByTestId("paged-diff-scroll-parent-pending")).toHaveStyle({
      height: `${260 * 20}px`,
    });
    expect(screen.queryByTestId("paged-diff-virtual-list")).toBeNull();
  });

  it("renders full-height placeholder slots from a row-count hint before the first page resolves", async () => {
    const delayedPage = createDeferredPage();
    mockGetDiffPage.mockImplementation(() => delayedPage.promise);

    render(
      <PagedDiffView
        conversationId="conv-1"
        filePath="src/Huge.tsx"
        refKind={{ kind: "head" }}
        pageSize={100}
        inlineScrollParent={document.createElement("div")}
        initialTotalRows={260}
      />
    );

    expect(await screen.findByTestId("paged-diff-view")).toHaveAttribute(
      "data-total-rows",
      "260"
    );
    expect(screen.queryByTestId("paged-diff-loading")).toBeNull();
    expect(screen.getByTestId("paged-diff-virtual-list")).toHaveAttribute(
      "data-count",
      "260"
    );
    expect(await screen.findAllByTestId("paged-diff-placeholder-row")).toHaveLength(24);
  });

  it("uses window scrolling when no outer scroll parent is present", async () => {
    render(
      <PagedDiffView
        conversationId="conv-1"
        filePath="src/Huge.tsx"
        refKind={{ kind: "head" }}
        pageSize={100}
      />
    );

    await screen.findByText("line 1");

    expect(screen.getByTestId("paged-diff-virtual-list")).toHaveAttribute(
      "data-custom-scroll-parent",
      "false"
    );
    expect(screen.getByTestId("paged-diff-virtual-list")).toHaveAttribute(
      "data-use-window-scroll",
      "true"
    );
  });

  it("loads inline pages from the visible virtual range while keeping off-range rows unmounted", async () => {
    mockGetDiffPage.mockImplementation(({ offset, limit }) =>
      Promise.resolve(makePage(offset as number, limit as number, 500))
    );

    render(
      <PagedDiffView
        conversationId="conv-1"
        filePath="src/Huge.tsx"
        refKind={{ kind: "head" }}
        pageSize={100}
      />
    );

    await screen.findByText("line 1");

    await setVirtuosoRange(220, 240);

    await waitFor(() =>
      expect(mockGetDiffPage).toHaveBeenCalledWith({
        conversationId: "conv-1",
        path: "src/Huge.tsx",
        refKind: { kind: "head" },
        offset: 200,
        limit: 100,
      })
    );
    expect(await screen.findByText("line 220")).toBeInTheDocument();
    expect(countPageFetches(100)).toBe(1);
    expect(countPageFetches(300)).toBe(1);
    expect(screen.queryByText("line 1")).toBeNull();
    expect(screen.getByTestId("paged-diff-view")).toHaveAttribute(
      "data-loaded-page-count",
      "4"
    );
    expect(screen.getByTestId("paged-diff-view")).toHaveAttribute(
      "data-mounted-page-count",
      "1"
    );

    await setVirtuosoRange(420, 440);

    expect(await screen.findByText("line 420")).toBeInTheDocument();
    expect(screen.queryByText("line 220")).toBeNull();
    expect(screen.getByTestId("paged-diff-view")).toHaveAttribute(
      "data-loaded-page-count",
      "5"
    );
    expect(screen.getByTestId("paged-diff-view")).toHaveAttribute(
      "data-mounted-page-count",
      "1"
    );
  });

  it("renders placeholder slots for unloaded visible rows and replaces them after the page resolves", async () => {
    const delayedPage = createDeferredPage();
    mockGetDiffPage.mockImplementation(({ offset, limit }) => {
      if (offset === 100) {
        return delayedPage.promise;
      }
      return Promise.resolve(makePage(offset as number, limit as number, 260));
    });

    render(
      <PagedDiffView
        conversationId="conv-1"
        filePath="src/Huge.tsx"
        refKind={{ kind: "head" }}
        pageSize={100}
      />
    );

    await screen.findByText("line 1");

    await setVirtuosoRange(120, 122);

    expect(await screen.findAllByTestId("paged-diff-placeholder-row")).toHaveLength(3);
    expect(screen.queryByText("line 120")).toBeNull();

    await act(async () => {
      delayedPage.resolve(makePage(100, 100, 260));
      await delayedPage.promise;
    });

    expect(await screen.findByText("line 120")).toBeInTheDocument();
    expect(screen.queryAllByTestId("paged-diff-placeholder-row")).toHaveLength(0);
  });

  it("keeps fetched inline pages cached when their rows leave the virtual range", async () => {
    mockGetDiffPage.mockImplementation(({ offset, limit }) =>
      Promise.resolve(makePage(offset as number, limit as number, 500))
    );

    render(
      <PagedDiffView
        conversationId="conv-1"
        filePath="src/Huge.tsx"
        refKind={{ kind: "head" }}
        pageSize={100}
      />
    );

    await screen.findByText("line 1");

    await setVirtuosoRange(120, 140);
    expect(await screen.findByText("line 120")).toBeInTheDocument();
    expect(countPageFetches(100)).toBe(1);

    await setVirtuosoRange(420, 440);
    expect(await screen.findByText("line 420")).toBeInTheDocument();
    expect(screen.queryByText("line 120")).toBeNull();

    await setVirtuosoRange(120, 140);
    expect(await screen.findByText("line 120")).toBeInTheDocument();
    expect(countPageFetches(100)).toBe(1);
  });

  it("renders terminal binary and empty page states without mounting rows", async () => {
    mockGetDiffPage.mockResolvedValueOnce({
      ...makePage(0, 100, 0),
      rows: [],
      isBinary: true,
    });

    const { unmount } = render(
      <PagedDiffView
        conversationId="conv-1"
        filePath="src/Huge.tsx"
        refKind={{ kind: "head" }}
        pageSize={100}
      />
    );

    expect(await screen.findByText(/binary file/i)).toBeInTheDocument();
    expect(screen.queryByTestId("paged-diff-view")).not.toBeInTheDocument();

    unmount();
    mockGetDiffPage.mockResolvedValueOnce({
      ...makePage(0, 100, 0),
      rows: [],
      totalRows: 0,
      nextOffset: null,
    });

    render(
      <PagedDiffView
        conversationId="conv-1"
        filePath="src/Huge.tsx"
        refKind={{ kind: "head" }}
        pageSize={100}
      />
    );

    expect(await screen.findByText(/no changes/i)).toBeInTheDocument();
    expect(screen.queryByTestId("paged-diff-view")).not.toBeInTheDocument();
  });

  it("retries the initial page after an error", async () => {
    const user = userEvent.setup();
    mockGetDiffPage
      .mockRejectedValueOnce(new Error("network"))
      .mockResolvedValueOnce(makePage(0, 100, 260));

    render(
      <PagedDiffView
        conversationId="conv-1"
        filePath="src/Huge.tsx"
        refKind={{ kind: "head" }}
        pageSize={100}
      />
    );

    expect(await screen.findByTestId("paged-diff-error")).toBeInTheDocument();

    await user.click(screen.getByRole("button", { name: /retry loading diff rows/i }));

    expect(await screen.findByText("line 1")).toBeInTheDocument();
    expect(countPageFetches(0)).toBe(2);
    expect(countPageFetches(100)).toBe(1);
  });

  it("loads visible pages and prunes distant loaded pages in contained scroll mode", async () => {
    mockGetDiffPage.mockImplementation(({ offset, limit }) =>
      Promise.resolve(makePage(offset as number, limit as number, 500))
    );

    render(
      <PagedDiffView
        conversationId="conv-1"
        filePath="src/Huge.tsx"
        refKind={{ kind: "cumulative_head" }}
        pageSize={100}
        scrollContainer={true}
      />
    );

    await screen.findByTestId("paged-diff-view");
    expect(screen.getByTestId("paged-diff-view")).toHaveAttribute(
      "data-scroll-container",
      "true"
    );
    expect(screen.getByTestId("paged-diff-virtual-list")).toBeInTheDocument();
    expect(screen.getByTestId("paged-diff-virtual-list")).toHaveAttribute(
      "data-increase-viewport-top",
      "320"
    );

    await setVirtuosoRange(220, 240);
    await waitFor(() =>
      expect(mockGetDiffPage).toHaveBeenCalledWith({
        conversationId: "conv-1",
        path: "src/Huge.tsx",
        refKind: { kind: "cumulative_head" },
        offset: 200,
        limit: 100,
      })
    );

    await setVirtuosoRange(450, 470);
    await waitFor(() =>
      expect(mockGetDiffPage).toHaveBeenCalledWith({
        conversationId: "conv-1",
        path: "src/Huge.tsx",
        refKind: { kind: "cumulative_head" },
        offset: 400,
        limit: 100,
      })
    );
    await waitFor(() => {
      const loadedPageCount = Number(
        screen.getByTestId("paged-diff-view").getAttribute("data-loaded-page-count")
      );
      expect(loadedPageCount).toBeLessThanOrEqual(3);
    });
  });
});
