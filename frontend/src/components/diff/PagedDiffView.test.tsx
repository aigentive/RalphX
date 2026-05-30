import { act, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { FileDiffPage } from "@/api/diff";

const mockGetDiffPage = vi.fn();
let latestRangeChanged:
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

describe("PagedDiffView", () => {
  beforeEach(() => {
    latestRangeChanged = undefined;
    mockGetDiffPage.mockReset();
    mockGetDiffPage.mockImplementation(({ offset, limit }) =>
      Promise.resolve(makePage(offset as number, limit as number))
    );
  });

  it("fetches and renders only the first page initially", async () => {
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
    expect(screen.getByText("@@ -1,260 +1,260 @@")).toBeInTheDocument();
    expect(screen.getByText("line 1")).toBeInTheDocument();
    expect(screen.queryByText("line 120")).toBeNull();
  });

  it("loads visible pages and prunes distant loaded pages", async () => {
    render(
      <PagedDiffView
        conversationId="conv-1"
        filePath="src/Huge.tsx"
        refKind={{ kind: "cumulative_head" }}
        pageSize={100}
      />
    );

    await screen.findByTestId("paged-diff-view");

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
