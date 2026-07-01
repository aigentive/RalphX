import { act, render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { FileDiffPage } from "@/api/diff";

const mockGetDiffPage = vi.fn();
let latestRangeChanged:
  | ((range: { startIndex: number; endIndex: number }) => void)
  | undefined;
const intersectionObservers: MockIntersectionObserver[] = [];
const resizeObservers: MockResizeObserver[] = [];
let measuredPageHeights = new Map<number, number>();

class MockIntersectionObserver {
  readonly callback: IntersectionObserverCallback;
  readonly elements = new Set<Element>();

  constructor(callback: IntersectionObserverCallback) {
    this.callback = callback;
    intersectionObservers.push(this);
  }

  observe = (element: Element) => {
    this.elements.add(element);
  };

  unobserve = (element: Element) => {
    this.elements.delete(element);
  };

  disconnect = () => {
    this.elements.clear();
  };

  takeRecords = () => [];

  trigger(testId: string) {
    for (const element of this.elements) {
      if (element.getAttribute("data-testid") === testId) {
        this.callback(
          [{ isIntersecting: true, target: element } as IntersectionObserverEntry],
          this as unknown as IntersectionObserver,
        );
      }
    }
  }
}

class MockResizeObserver {
  readonly callback: ResizeObserverCallback;
  readonly elements = new Set<Element>();

  constructor(callback: ResizeObserverCallback) {
    this.callback = callback;
    resizeObservers.push(this);
  }

  observe = (element: Element) => {
    this.elements.add(element);
    const offset = Number((element as HTMLElement).dataset.pageOffset);
    const height = measuredPageHeights.get(offset) ?? 0;
    if (height > 0) {
      this.callback(
        [
          {
            target: element,
            contentRect: { height },
          } as ResizeObserverEntry,
        ],
        this as unknown as ResizeObserver,
      );
    }
  };

  unobserve = (element: Element) => {
    this.elements.delete(element);
  };

  disconnect = () => {
    this.elements.clear();
  };
}

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
    "data-testid"?: string;
  };

  function Virtuoso(props: VirtuosoMockProps) {
    latestRangeChanged = props.rangeChanged;
    React.useEffect(() => {
      props.rangeChanged?.({ startIndex: 0, endIndex: 23 });
    }, [props]);
    const totalCount = props.totalCount ?? 0;
    return (
      <div
        data-testid={props["data-testid"] ?? "mock-virtuoso"}
        data-count={totalCount}
        style={props.style}
      >
        {Array.from({ length: Math.min(totalCount, 24) }).map((_, index) => (
          <div key={props.computeItemKey?.(index) ?? index}>
            {props.itemContent?.(index)}
          </div>
        ))}
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
        return { kind: "hunk_header" as const, header: "@@ -1,260 +1,260 @@" };
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

async function triggerObservedSentinel(testId: string) {
  await screen.findByTestId(testId);
  await waitFor(() => {
    expect(
      intersectionObservers.some((observer) =>
        Array.from(observer.elements).some(
          (element) => element.getAttribute("data-testid") === testId
        )
      )
    ).toBe(true);
  });

  await act(async () => {
    for (const observer of intersectionObservers) {
      observer.trigger(testId);
    }
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
    latestRangeChanged = undefined;
    intersectionObservers.length = 0;
    resizeObservers.length = 0;
    measuredPageHeights = new Map();
    vi.stubGlobal("IntersectionObserver", MockIntersectionObserver);
    vi.stubGlobal("ResizeObserver", MockResizeObserver);
    mockGetDiffPage.mockReset();
    mockGetDiffPage.mockImplementation(({ offset, limit }) =>
      Promise.resolve(makePage(offset as number, limit as number))
    );
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("fetches and renders only the first page initially without owning a vertical scroller", async () => {
    render(
      <PagedDiffView
        conversationId="conv-1"
        filePath="src/Huge.tsx"
        refKind={{ kind: "head" }}
        pageSize={100}
      />
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
      "1"
    );
    expect(screen.getByTestId("paged-diff-view")).toHaveAttribute(
      "data-mounted-page-count",
      "1"
    );
    expect(screen.queryByTestId("paged-diff-virtual-list")).not.toBeInTheDocument();
    expect(screen.getByTestId("paged-diff-inline-list")).toBeInTheDocument();
    expect(screen.getByText("@@ -1,260 +1,260 @@")).toBeInTheDocument();
    expect(screen.getByText("line 1")).toBeInTheDocument();
    expect(screen.queryByText("line 120")).toBeNull();
  });

  it("loads the next inline page when the bottom sentinel reaches the outer scroll", async () => {
    render(
      <PagedDiffView
        conversationId="conv-1"
        filePath="src/Huge.tsx"
        refKind={{ kind: "head" }}
        pageSize={100}
      />
    );

    await screen.findByTestId("paged-diff-view");

    await triggerObservedSentinel("paged-diff-next-sentinel");

    await waitFor(() =>
      expect(mockGetDiffPage).toHaveBeenCalledWith({
        conversationId: "conv-1",
        path: "src/Huge.tsx",
        refKind: { kind: "head" },
        offset: 100,
        limit: 100,
      })
    );
    expect(await screen.findByText("line 120")).toBeInTheDocument();
    expect(screen.getByTestId("paged-diff-view")).toHaveAttribute(
      "data-loaded-page-count",
      "2"
    );
    expect(screen.getByTestId("paged-diff-view")).toHaveAttribute(
      "data-mounted-page-count",
      "2"
    );
  });

  it("keeps only the inline mounted page window rendered as deeper pages load", async () => {
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

    await triggerObservedSentinel("paged-diff-next-sentinel");
    expect(await screen.findByText("line 120")).toBeInTheDocument();

    await triggerObservedSentinel("paged-diff-next-sentinel");
    expect(await screen.findByText("line 220")).toBeInTheDocument();

    await triggerObservedSentinel("paged-diff-next-sentinel");
    expect(await screen.findByText("line 320")).toBeInTheDocument();

    await waitFor(() => {
      expect(screen.getByTestId("paged-diff-view")).toHaveAttribute(
        "data-loaded-page-count",
        "4"
      );
      expect(screen.getByTestId("paged-diff-view")).toHaveAttribute(
        "data-mounted-page-count",
        "2"
      );
    });
    expect(screen.queryByText("line 1")).toBeNull();
    expect(screen.queryByText("line 120")).toBeNull();
    expect(screen.getByText("line 220")).toBeInTheDocument();
    expect(screen.getByText("line 320")).toBeInTheDocument();
    expect(screen.getByTestId("paged-diff-top-spacer")).toHaveStyle({
      height: "4000px",
    });
    expect(screen.getByTestId("paged-diff-bottom-spacer")).toHaveStyle({
      height: "2000px",
    });
  });

  it("uses measured cached page heights for inline spacers when pages unmount", async () => {
    measuredPageHeights = new Map([
      [0, 2600],
      [100, 5200],
      [200, 4400],
      [300, 4600],
    ]);
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

    await triggerObservedSentinel("paged-diff-next-sentinel");
    expect(await screen.findByText("line 120")).toBeInTheDocument();

    await triggerObservedSentinel("paged-diff-next-sentinel");
    expect(await screen.findByText("line 220")).toBeInTheDocument();

    await waitFor(() =>
      expect(screen.getByTestId("paged-diff-top-spacer")).toHaveStyle({
        height: "2600px",
      })
    );

    await triggerObservedSentinel("paged-diff-next-sentinel");
    expect(await screen.findByText("line 320")).toBeInTheDocument();

    await waitFor(() =>
      expect(screen.getByTestId("paged-diff-top-spacer")).toHaveStyle({
        height: "7800px",
      })
    );
  });

  it("can page backward from cached inline pages outside the mounted window", async () => {
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

    await triggerObservedSentinel("paged-diff-next-sentinel");
    expect(await screen.findByText("line 120")).toBeInTheDocument();

    await triggerObservedSentinel("paged-diff-next-sentinel");
    expect(await screen.findByText("line 220")).toBeInTheDocument();

    await triggerObservedSentinel("paged-diff-next-sentinel");
    expect(await screen.findByText("line 320")).toBeInTheDocument();
    expect(screen.queryByText("line 120")).toBeNull();
    expect(screen.getByTestId("paged-diff-view")).toHaveAttribute(
      "data-loaded-page-count",
      "4"
    );
    expect(countPageFetches(100)).toBe(1);

    await triggerObservedSentinel("paged-diff-previous-sentinel");

    expect(await screen.findByText("line 120")).toBeInTheDocument();
    expect(countPageFetches(100)).toBe(1);
    expect(screen.getByTestId("paged-diff-view")).toHaveAttribute(
      "data-loaded-page-count",
      "4"
    );
    expect(screen.getByTestId("paged-diff-view")).toHaveAttribute(
      "data-mounted-page-count",
      "3"
    );
    expect(screen.getByText("line 220")).toBeInTheDocument();
    expect(screen.queryByText("line 320")).toBeNull();
  });

  it("keeps the current page visually anchored when mounting cached pages above", async () => {
    measuredPageHeights = new Map([
      [0, 2600],
      [100, 5200],
      [200, 4400],
      [300, 4600],
    ]);
    const pageTopQueues = new Map<number, number[]>();
    const rectSpy = vi
      .spyOn(Element.prototype, "getBoundingClientRect")
      .mockImplementation(function getMockRect() {
        const offset = Number((this as HTMLElement).dataset.pageOffset);
        const queue = pageTopQueues.get(offset);
        const top =
          queue && queue.length > 1
            ? queue.shift()!
            : queue?.[0] ?? 0;
        const height = measuredPageHeights.get(offset) ?? 0;
        return {
          x: 0,
          y: top,
          top,
          right: 0,
          bottom: top + height,
          left: 0,
          width: 0,
          height,
          toJSON: () => ({}),
        } as DOMRect;
      });
    const scrollBySpy = vi
      .spyOn(window, "scrollBy")
      .mockImplementation(() => undefined);
    mockGetDiffPage.mockImplementation(({ offset, limit }) =>
      Promise.resolve(makePage(offset as number, limit as number, 500))
    );

    try {
      render(
        <PagedDiffView
          conversationId="conv-1"
          filePath="src/Huge.tsx"
          refKind={{ kind: "head" }}
          pageSize={100}
        />
      );

      await screen.findByText("line 1");
      await triggerObservedSentinel("paged-diff-next-sentinel");
      expect(await screen.findByText("line 120")).toBeInTheDocument();
      await triggerObservedSentinel("paged-diff-next-sentinel");
      expect(await screen.findByText("line 220")).toBeInTheDocument();
      await triggerObservedSentinel("paged-diff-next-sentinel");
      expect(await screen.findByText("line 320")).toBeInTheDocument();

      pageTopQueues.set(200, [100, 5300]);
      await triggerObservedSentinel("paged-diff-previous-sentinel");

      await waitFor(() => expect(scrollBySpy).toHaveBeenCalledWith(0, 5200));
    } finally {
      scrollBySpy.mockRestore();
      rectSpy.mockRestore();
    }
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
    expect(mockGetDiffPage).toHaveBeenCalledTimes(2);
  });

  it("loads visible pages and prunes distant loaded pages in contained scroll mode", async () => {
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

    await act(async () => {
      latestRangeChanged?.({ startIndex: 220, endIndex: 240 });
    });
    await waitFor(() =>
      expect(mockGetDiffPage).toHaveBeenCalledWith({
        conversationId: "conv-1",
        path: "src/Huge.tsx",
        refKind: { kind: "cumulative_head" },
        offset: 200,
        limit: 100,
      })
    );

    await act(async () => {
      latestRangeChanged?.({ startIndex: 450, endIndex: 470 });
    });
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
      expect(loadedPageCount).toBeLessThanOrEqual(2);
    });
  });
});
