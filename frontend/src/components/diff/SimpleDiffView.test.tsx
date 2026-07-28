/**
 * SimpleDiffView tests — hunk-based rendering + lazy range fetch.
 *
 * Tests prove:
 *  (a) Correct API called with correct args
 *  (b) DOM-level rendering (not just prop passing)
 *  (c) Range fetch deduplication (cache hit)
 *  (d) Binary file placeholder
 *  (e) Error + retry flow
 */

import { describe, it, expect, vi, beforeEach, afterEach } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type {
  DiffHunk,
  DiffLine,
  PrDiffAnnotation,
  WorkspaceReviewHunkAnnotation,
} from "@/api/diff";

// ── Mock diffApi ─────────────────────────────────────────────────────────
const mockGetRange = vi.fn();
const mockOpenUrl = vi.fn();
vi.mock("@/api/diff", () => ({
  diffApi: {
    getAgentConversationWorkspaceFileContentRange: (...args: unknown[]) =>
      mockGetRange(...args),
  },
}));
vi.mock("@tauri-apps/plugin-opener", () => ({
  openUrl: (...args: unknown[]) => mockOpenUrl(...args),
}));
vi.mock("react-virtuoso", async () => {
  const React = await vi.importActual<typeof import("react")>("react");

  type VirtuosoMockProps = {
    data?: unknown[];
    itemContent?: (index: number, item: unknown) => React.ReactNode;
    computeItemKey?: (index: number, item: unknown) => React.Key;
    className?: string;
    style?: React.CSSProperties;
    "data-testid"?: string;
  };

  function Virtuoso(props: VirtuosoMockProps) {
    const data = props.data ?? [];
    return (
      <div
        data-testid={props["data-testid"] ?? "mock-virtuoso"}
        data-count={data.length}
        className={props.className}
        style={props.style}
      >
        {data.slice(0, 24).map((item, index) => (
          <div key={props.computeItemKey?.(index, item) ?? index}>
            {props.itemContent?.(index, item)}
          </div>
        ))}
      </div>
    );
  }

  return { Virtuoso };
});

import { SimpleDiffView } from "./SimpleDiffView";

// ── Fixtures ──────────────────────────────────────────────────────────────

function makeDiffLine(overrides: Partial<DiffLine> = {}): DiffLine {
  return {
    kind: "context",
    content: "some code",
    oldLineNum: 1,
    newLineNum: 1,
    ...overrides,
  };
}

function makeHunk(overrides: Partial<DiffHunk> = {}): DiffHunk {
  return {
    oldStart: 1,
    oldLines: 3,
    newStart: 1,
    newLines: 3,
    header: "@@ -1,3 +1,3 @@",
    lines: [
      makeDiffLine({ kind: "context", content: "ctx line 1", oldLineNum: 1, newLineNum: 1 }),
      makeDiffLine({ kind: "deletion", content: "old line", oldLineNum: 2, newLineNum: null }),
      makeDiffLine({ kind: "addition", content: "new line", oldLineNum: null, newLineNum: 2 }),
      makeDiffLine({ kind: "context", content: "ctx line 3", oldLineNum: 3, newLineNum: 3 }),
    ],
    ...overrides,
  };
}

const defaultHunks: DiffHunk[] = [makeHunk()];

function makeLargeHunkWithLeadingGap(): DiffHunk {
  return makeHunk({
    oldStart: 4,
    oldLines: 1_000,
    newStart: 4,
    newLines: 1_000,
    header: "@@ -4,1000 +4,1000 @@",
    lines: Array.from({ length: 1_000 }, (_value, index) =>
      makeDiffLine({
        content: `virtual diff line ${index}`,
        oldLineNum: index + 4,
        newLineNum: index + 4,
      })
    ),
  });
}

function makeAnnotation(overrides: Partial<PrDiffAnnotation> = {}): PrDiffAnnotation {
  return {
    id: "annotation-1",
    source: "check_run",
    path: "src/foo.ts",
    side: "right",
    startLine: 2,
    endLine: 2,
    startColumn: null,
    endColumn: null,
    level: "failure",
    status: "failure",
    title: "CodeQL warning",
    message: "Validate externally influenced paths.",
    author: null,
    checkName: "CodeQL",
    url: null,
    isOutdated: false,
    createdAt: null,
    ...overrides,
  };
}

function makeHunkAnnotation(
  overrides: Partial<WorkspaceReviewHunkAnnotation> = {},
): WorkspaceReviewHunkAnnotation {
  return {
    id: "workspace-review-hunk-1",
    conversationId: "conv-1",
    projectId: "project-1",
    artifactId: "artifact-1",
    artifactVersion: 1,
    targetScope: "selected_source",
    headSha: "head-sha",
    diffFingerprint: "fingerprint-1",
    path: "src/foo.ts",
    diffSource: "selected_source",
    hunkHeader: "@@ -1,3 +1,3 @@",
    oldStart: 1,
    oldLines: 3,
    newStart: 1,
    newLines: 3,
    title: "Review summary",
    message: "This hunk updates the renderer.",
    level: "notice",
    createdByRunId: "run-1",
    createdAt: "2026-07-01T00:00:00Z",
    ...overrides,
  };
}

describe("SimpleDiffView", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  afterEach(() => {
    vi.restoreAllMocks();
  });

  // ── Binary file ──────────────────────────────────────────────────────

  describe("binary file", () => {
    it("renders binary placeholder when isBinary=true", () => {
      render(
        <SimpleDiffView
          hunks={[]}
          oldTotalLines={0}
          newTotalLines={0}
          isBinary={true}
        />
      );
      expect(screen.getByText(/binary file/i)).toBeInTheDocument();
    });

    it("does not render diff content for binary files", () => {
      render(
        <SimpleDiffView
          hunks={defaultHunks}
          oldTotalLines={3}
          newTotalLines={3}
          isBinary={true}
        />
      );
      expect(screen.queryByText("ctx line 1")).not.toBeInTheDocument();
    });
  });

  // ── No changes ───────────────────────────────────────────────────────

  describe("empty hunks", () => {
    it("renders 'No changes' when hunks is empty and not binary", () => {
      render(
        <SimpleDiffView
          hunks={[]}
          oldTotalLines={10}
          newTotalLines={10}
          isBinary={false}
        />
      );
      expect(screen.getByText(/no changes/i)).toBeInTheDocument();
    });
  });

  // ── Hunk rendering ───────────────────────────────────────────────────

  describe("hunk rendering", () => {
    it("uses compact typography when embedded in dense surfaces", () => {
      const { container } = render(
        <SimpleDiffView
          hunks={defaultHunks}
          oldTotalLines={3}
          newTotalLines={3}
          density="compact"
        />
      );

      const body = container.querySelector('[data-density="compact"]');
      expect(body).toHaveClass("text-[0.6875rem]", "leading-[18px]");
      expect(body).not.toHaveClass("text-[0.8125rem]", "leading-[20px]");
    });

    it("renders hunk header", () => {
      render(
        <SimpleDiffView
          hunks={defaultHunks}
          oldTotalLines={3}
          newTotalLines={3}
        />
      );
      expect(screen.getByText("@@ -1,3 +1,3 @@")).toBeInTheDocument();
    });

    it("renders workspace review hunk annotations below matching hunk headers", () => {
      render(
        <SimpleDiffView
          hunks={defaultHunks}
          oldTotalLines={3}
          newTotalLines={3}
          hunkAnnotations={[makeHunkAnnotation()]}
        />
      );

      expect(screen.getByTestId("diff-hunk-annotation-row")).toBeInTheDocument();
      expect(screen.getByText("Workspace review")).toBeInTheDocument();
      expect(screen.getByText("Review summary")).toBeInTheDocument();
      expect(screen.getByText("This hunk updates the renderer.")).toBeInTheDocument();
    });

    it("toggles line wrapping when the wrap control is visible", async () => {
      const user = userEvent.setup();
      const { container } = render(
        <SimpleDiffView
          hunks={defaultHunks}
          oldTotalLines={3}
          newTotalLines={3}
          defaultWrapLines={true}
        />
      );

      expect(container.querySelector("[data-wrap-lines='true']")).toBeInTheDocument();

      await user.click(screen.getByRole("button", { name: /disable wrap/i }));

      expect(container.querySelector("[data-wrap-lines='false']")).toBeInTheDocument();
      expect(screen.getByRole("button", { name: /wrap lines/i })).toBeInTheDocument();
    });

    it("renders context line content", () => {
      render(
        <SimpleDiffView
          hunks={defaultHunks}
          oldTotalLines={3}
          newTotalLines={3}
        />
      );
      expect(screen.getByText("ctx line 1")).toBeInTheDocument();
    });

    it("renders addition line content", () => {
      render(
        <SimpleDiffView
          hunks={defaultHunks}
          oldTotalLines={3}
          newTotalLines={3}
        />
      );
      expect(screen.getByText("new line")).toBeInTheDocument();
    });

    it("renders deletion line content", () => {
      render(
        <SimpleDiffView
          hunks={defaultHunks}
          oldTotalLines={3}
          newTotalLines={3}
        />
      );
      expect(screen.getByText("old line")).toBeInTheDocument();
    });

    it("renders multiple hunks", () => {
      const hunks: DiffHunk[] = [
        makeHunk({ oldStart: 1, newStart: 1, header: "@@ -1,3 +1,3 @@" }),
        makeHunk({
          oldStart: 20,
          newStart: 20,
          header: "@@ -20,3 +20,3 @@",
          lines: [
            makeDiffLine({ kind: "addition", content: "far away line", oldLineNum: null, newLineNum: 20 }),
          ],
        }),
      ];
      render(
        <SimpleDiffView hunks={hunks} oldTotalLines={25} newTotalLines={25} />
      );
      expect(screen.getByText("@@ -1,3 +1,3 @@")).toBeInTheDocument();
      expect(screen.getByText("@@ -20,3 +20,3 @@")).toBeInTheDocument();
      expect(screen.getByText("far away line")).toBeInTheDocument();
    });

    it("virtualizes large diffs instead of mounting every line", () => {
      const lines = Array.from({ length: 1_200 }, (_value, index) =>
        makeDiffLine({
          content: `large diff line ${index}`,
          oldLineNum: index + 1,
          newLineNum: index + 1,
        })
      );
      render(
        <SimpleDiffView
          hunks={[
            makeHunk({
              oldLines: lines.length,
              newLines: lines.length,
              header: "@@ -1,1200 +1,1200 @@",
              lines,
            }),
          ]}
          oldTotalLines={1_200}
          newTotalLines={1_200}
        />
      );

      expect(screen.getByTestId("simple-diff-virtualized")).toBeInTheDocument();
      expect(screen.getByTestId("simple-diff-virtual-list")).toHaveAttribute(
        "data-count",
        "1201",
      );
      expect(screen.getByText("large diff line 0")).toBeInTheDocument();
      expect(screen.queryByText("large diff line 1199")).not.toBeInTheDocument();
    });

    it("renders and expands virtualized leading context gaps", async () => {
      let resolveRange: (v: { lineNum: number; content: string }[]) => void = () => undefined;
      mockGetRange.mockReturnValue(new Promise((resolve) => { resolveRange = resolve; }));
      const user = userEvent.setup();

      render(
        <SimpleDiffView
          hunks={[makeLargeHunkWithLeadingGap()]}
          oldTotalLines={1_003}
          newTotalLines={1_003}
          annotations={[makeAnnotation({ startLine: 2, endLine: 2 })]}
          conversationId="conv-1"
          filePath="src/foo.ts"
          refKind={{ kind: "head" }}
        />
      );

      expect(screen.getByTestId("simple-diff-virtualized")).toBeInTheDocument();
      expect(screen.getByTestId("diff-hidden-annotations")).toHaveTextContent(
        "1 GitHub annotation in hidden context"
      );

      await user.click(
        screen.getByRole("button", {
          name: /show 1 hidden annotations in 3 unchanged lines/i,
        })
      );

      expect(screen.getByTestId("gap-loading")).toBeInTheDocument();
      resolveRange([{ lineNum: 2, content: "virtual fetched context" }]);

      await waitFor(() =>
        expect(screen.getByText("virtual fetched context")).toBeInTheDocument()
      );
      await user.click(screen.getByRole("button", { name: /hide unchanged lines/i }));
      await user.click(
        screen.getByRole("button", {
          name: /show 1 hidden annotations in 3 unchanged lines/i,
        })
      );

      expect(mockGetRange).toHaveBeenCalledOnce();
      expect(screen.getByText("virtual fetched context")).toBeInTheDocument();
    });

    it("retries virtualized context gaps after a range fetch error", async () => {
      mockGetRange
        .mockRejectedValueOnce(new Error("network error"))
        .mockResolvedValueOnce([{ lineNum: 2, content: "virtual retried context" }]);
      const user = userEvent.setup();

      render(
        <SimpleDiffView
          hunks={[makeLargeHunkWithLeadingGap()]}
          oldTotalLines={1_003}
          newTotalLines={1_003}
          conversationId="conv-1"
          filePath="src/foo.ts"
          refKind={{ kind: "head" }}
        />
      );

      await user.click(screen.getByRole("button", { name: /show 3 unchanged lines/i }));
      await waitFor(() => expect(screen.getByTestId("gap-error")).toBeInTheDocument());

      await user.click(screen.getByRole("button", { name: /retry/i }));

      await waitFor(() =>
        expect(screen.getByText("virtual retried context")).toBeInTheDocument()
      );
      expect(mockGetRange).toHaveBeenCalledTimes(2);
    });

    it("renders virtualized context gap labels without fetch controls in embedded mode", () => {
      render(
        <SimpleDiffView
          hunks={[makeLargeHunkWithLeadingGap()]}
          oldTotalLines={1_003}
          newTotalLines={1_003}
          scrollContainer={false}
          showWrapToggle={false}
        />
      );

      expect(screen.getByText(/3 unchanged lines/i)).toBeInTheDocument();
      expect(screen.queryByRole("button", { name: /show 3 unchanged lines/i })).toBeNull();
      expect(screen.queryByRole("button", { name: /wrap/i })).toBeNull();
      expect(screen.getByTestId("simple-diff-virtual-list")).not.toHaveClass(
        "min-h-0",
        "flex-1"
      );
    });

    it("renders GitHub annotations on matching diff lines", () => {
      render(
        <SimpleDiffView
          hunks={defaultHunks}
          oldTotalLines={3}
          newTotalLines={3}
          annotations={[makeAnnotation()]}
        />
      );

      expect(screen.getByTestId("diff-annotation-row")).toBeInTheDocument();
      expect(screen.getByText("CodeQL:")).toBeInTheDocument();
      expect(screen.getByText("CodeQL warning")).toBeInTheDocument();
    });

    it("opens annotation URLs in GitHub", async () => {
      mockOpenUrl.mockResolvedValue(undefined);
      const user = userEvent.setup();
      render(
        <SimpleDiffView
          hunks={defaultHunks}
          oldTotalLines={3}
          newTotalLines={3}
          annotations={[
            makeAnnotation({
              url: "https://github.com/owner/repo/pull/1#annotation",
            }),
          ]}
        />
      );

      await user.click(screen.getByRole("button", { name: /open annotation in github/i }));

      expect(mockOpenUrl).toHaveBeenCalledWith(
        "https://github.com/owner/repo/pull/1#annotation"
      );
    });

    it("renders code scanning annotation detail and outdated state without a GitHub action", () => {
      render(
        <SimpleDiffView
          hunks={defaultHunks}
          oldTotalLines={3}
          newTotalLines={3}
          annotations={[
            makeAnnotation({
              source: "code_scanning",
              checkName: null,
              level: "medium",
              title: "Filesystem path injection",
              message: "Validate externally influenced paths before use.",
              isOutdated: true,
              url: null,
            }),
          ]}
        />
      );

      expect(screen.getByText("Code scanning")).toBeInTheDocument();
      expect(screen.getByText("Filesystem path injection")).toBeInTheDocument();
      expect(
        screen.getByText("Validate externally influenced paths before use.")
      ).toBeInTheDocument();
      expect(screen.getByText("outdated")).toBeInTheDocument();
      expect(
        screen.queryByRole("button", { name: /open annotation in github/i })
      ).not.toBeInTheDocument();
    });

    it("renders old-side and custom-source annotations on matching lines", () => {
      render(
        <SimpleDiffView
          hunks={defaultHunks}
          oldTotalLines={3}
          newTotalLines={3}
          annotations={[
            makeAnnotation({
              id: "review-comment:2",
              source: "review_comment",
              side: "LEFT",
              startLine: 2,
              level: "notice",
              title: null,
              message: "Review note on the removed line.",
              checkName: null,
            }),
            makeAnnotation({
              id: "third-party:1",
              source: "third_party_check",
              side: "right",
              startLine: 2,
              level: "note",
              title: null,
              message: "Custom checker note.",
              checkName: null,
            }),
          ]}
        />
      );

      expect(screen.getByText("Review")).toBeInTheDocument();
      expect(screen.getByText("Review note on the removed line.")).toBeInTheDocument();
      expect(screen.getByText("third party check")).toBeInTheDocument();
      expect(screen.getByText("Custom checker note.")).toBeInTheDocument();
    });
  });

  // ── Gap expanders ────────────────────────────────────────────────────

  describe("gap expanders (no range fetch capability)", () => {
    it("shows 'unchanged lines' label between hunks when no conversationId", () => {
      const hunks: DiffHunk[] = [
        makeHunk({ oldStart: 1, oldLines: 3, newStart: 1, newLines: 3 }),
        makeHunk({
          oldStart: 10,
          oldLines: 3,
          newStart: 10,
          newLines: 3,
          header: "@@ -10,3 +10,3 @@",
          lines: [
            makeDiffLine({ kind: "addition", content: "later", oldLineNum: null, newLineNum: 10 }),
          ],
        }),
      ];
      render(<SimpleDiffView hunks={hunks} oldTotalLines={15} newTotalLines={15} />);
      // Gap between hunk 1 end (line 3) and hunk 2 start (line 10): 6 lines
      expect(screen.getByText(/6 unchanged lines/i)).toBeInTheDocument();
    });

    it("can hide wrapping controls and gap rows for embedded surfaces", () => {
      const hunks: DiffHunk[] = [
        makeHunk({ oldStart: 1, oldLines: 3, newStart: 1, newLines: 3 }),
        makeHunk({
          oldStart: 10,
          oldLines: 3,
          newStart: 10,
          newLines: 3,
          header: "@@ -10,3 +10,3 @@",
          lines: [
            makeDiffLine({ kind: "addition", content: "later", oldLineNum: null, newLineNum: 10 }),
          ],
        }),
      ];
      const { container } = render(
        <SimpleDiffView
          hunks={hunks}
          oldTotalLines={15}
          newTotalLines={15}
          defaultWrapLines={false}
          showWrapToggle={false}
          showContextGaps={false}
        />
      );

      expect(screen.queryByRole("button", { name: /wrap/i })).not.toBeInTheDocument();
      expect(screen.queryByText(/unchanged lines/i)).not.toBeInTheDocument();
      expect(container.querySelector("[data-wrap-lines='false']")).toBeInTheDocument();
    });

    it("shows leading gap label when first hunk doesn't start at line 1", () => {
      const hunks: DiffHunk[] = [
        makeHunk({ oldStart: 5, oldLines: 3, newStart: 5, newLines: 3 }),
      ];
      render(<SimpleDiffView hunks={hunks} oldTotalLines={10} newTotalLines={10} />);
      // Leading gap: lines 1-4 = 4 lines
      expect(screen.getByText(/4 unchanged lines/i)).toBeInTheDocument();
    });

    it("shows trailing gap label when last hunk doesn't reach end", () => {
      const hunks: DiffHunk[] = [
        makeHunk({ oldStart: 1, oldLines: 3, newStart: 1, newLines: 3 }),
      ];
      render(<SimpleDiffView hunks={hunks} oldTotalLines={10} newTotalLines={10} />);
      // Trailing gap: lines 4-10 = 7 lines
      expect(screen.getByText(/7 unchanged lines/i)).toBeInTheDocument();
    });

    it("does not show gap label when hunks cover all lines", () => {
      const hunk = makeHunk({ oldStart: 1, oldLines: 3, newStart: 1, newLines: 3 });
      render(<SimpleDiffView hunks={[hunk]} oldTotalLines={3} newTotalLines={3} />);
      expect(screen.queryByText(/unchanged lines/i)).not.toBeInTheDocument();
    });
  });

  // ── Range fetch ──────────────────────────────────────────────────────

  describe("range fetch (with conversationId)", () => {
    const rangeProps = {
      conversationId: "conv-1",
      filePath: "src/foo.ts",
      refKind: { kind: "head" } as const,
    };

    it("shows 'Show N unchanged lines' button between hunks when conversationId provided", () => {
      const hunks: DiffHunk[] = [
        makeHunk({ oldStart: 1, oldLines: 3, newStart: 1, newLines: 3 }),
        makeHunk({
          oldStart: 10,
          oldLines: 3,
          newStart: 10,
          newLines: 3,
          header: "@@ -10,3 +10,3 @@",
          lines: [makeDiffLine({ kind: "addition", content: "later", oldLineNum: null, newLineNum: 10 })],
        }),
      ];
      render(
        <SimpleDiffView
          hunks={hunks}
          oldTotalLines={15}
          newTotalLines={15}
          {...rangeProps}
        />
      );
      expect(screen.getByRole("button", { name: /show 6 unchanged lines/i })).toBeInTheDocument();
    });

    it("shows hidden annotation affordance inside collapsed gaps", () => {
      const hunks: DiffHunk[] = [
        makeHunk({ oldStart: 1, oldLines: 3, newStart: 1, newLines: 3 }),
        makeHunk({
          oldStart: 10,
          oldLines: 3,
          newStart: 10,
          newLines: 3,
          header: "@@ -10,3 +10,3 @@",
          lines: [makeDiffLine({ kind: "addition", content: "later", oldLineNum: null, newLineNum: 10 })],
        }),
      ];
      render(
        <SimpleDiffView
          hunks={hunks}
          oldTotalLines={15}
          newTotalLines={15}
          annotations={[makeAnnotation({ startLine: 6, endLine: 6 })]}
          {...rangeProps}
        />
      );

      expect(screen.getByTestId("diff-hidden-annotations")).toHaveTextContent(
        "1 GitHub annotation in hidden context"
      );
      expect(
        screen.getByRole("button", {
          name: /show 1 hidden annotations in 6 unchanged lines/i,
        })
      ).toBeInTheDocument();
    });

    it("fires range fetch with correct args on button click", async () => {
      mockGetRange.mockResolvedValue([]);
      const user = userEvent.setup();
      const hunks: DiffHunk[] = [
        makeHunk({ oldStart: 1, oldLines: 3, newStart: 1, newLines: 3 }),
        makeHunk({
          oldStart: 10,
          oldLines: 3,
          newStart: 10,
          newLines: 3,
          header: "@@ -10,3 +10,3 @@",
          lines: [makeDiffLine({ kind: "addition", content: "later", oldLineNum: null, newLineNum: 10 })],
        }),
      ];

      render(
        <SimpleDiffView
          hunks={hunks}
          oldTotalLines={15}
          newTotalLines={15}
          {...rangeProps}
        />
      );

      await user.click(screen.getByRole("button", { name: /show 6 unchanged lines/i }));

      expect(mockGetRange).toHaveBeenCalledWith({
        conversationId: "conv-1",
        side: "new",
        path: "src/foo.ts",
        refKind: { kind: "head" },
        from: 4,  // hunk1.newStart + hunk1.newLines = 1 + 3 = 4
        to: 9,    // hunk2.newStart - 1 = 10 - 1 = 9
      });
    });

    it("shows loading state while range fetch is in flight", async () => {
      let resolveRange: (v: never[]) => void = () => undefined;
      mockGetRange.mockReturnValue(new Promise((res) => { resolveRange = res; }));
      const user = userEvent.setup();
      const hunks: DiffHunk[] = [
        makeHunk({ oldStart: 1, oldLines: 3, newStart: 1, newLines: 3 }),
        makeHunk({
          oldStart: 10,
          oldLines: 3,
          newStart: 10,
          newLines: 3,
          header: "@@ -10,3 +10,3 @@",
          lines: [makeDiffLine({ kind: "addition", content: "later", oldLineNum: null, newLineNum: 10 })],
        }),
      ];

      render(
        <SimpleDiffView
          hunks={hunks}
          oldTotalLines={15}
          newTotalLines={15}
          {...rangeProps}
        />
      );

      await user.click(screen.getByRole("button", { name: /show 6 unchanged lines/i }));
      expect(screen.getByTestId("gap-loading")).toBeInTheDocument();
      resolveRange([]);
    });

    it("renders fetched lines after range fetch completes", async () => {
      mockGetRange.mockResolvedValue([
        { lineNum: 4, content: "fetched line A" },
        { lineNum: 5, content: "fetched line B" },
      ]);
      const user = userEvent.setup();
      const hunks: DiffHunk[] = [
        makeHunk({ oldStart: 1, oldLines: 3, newStart: 1, newLines: 3 }),
        makeHunk({
          oldStart: 10,
          oldLines: 3,
          newStart: 10,
          newLines: 3,
          header: "@@ -10,3 +10,3 @@",
          lines: [makeDiffLine({ kind: "addition", content: "later", oldLineNum: null, newLineNum: 10 })],
        }),
      ];

      render(
        <SimpleDiffView
          hunks={hunks}
          oldTotalLines={15}
          newTotalLines={15}
          {...rangeProps}
        />
      );

      await user.click(screen.getByRole("button", { name: /show 6 unchanged lines/i }));
      await waitFor(() => expect(screen.getByText("fetched line A")).toBeInTheDocument());
      expect(screen.getByText("fetched line B")).toBeInTheDocument();
    });

    it("keeps expanded range rows outside the padded gap control", async () => {
      mockGetRange.mockResolvedValue([
        { lineNum: 4, content: "aligned fetched line" },
      ]);
      const user = userEvent.setup();
      const hunks: DiffHunk[] = [
        makeHunk({ oldStart: 1, oldLines: 3, newStart: 1, newLines: 3 }),
        makeHunk({
          oldStart: 10,
          oldLines: 3,
          newStart: 10,
          newLines: 3,
          header: "@@ -10,3 +10,3 @@",
          lines: [makeDiffLine({ kind: "addition", content: "later", oldLineNum: null, newLineNum: 10 })],
        }),
      ];

      render(
        <SimpleDiffView
          hunks={hunks}
          oldTotalLines={15}
          newTotalLines={15}
          {...rangeProps}
        />
      );

      await user.click(screen.getByRole("button", { name: /show 6 unchanged lines/i }));
      await waitFor(() => expect(screen.getByText("aligned fetched line")).toBeInTheDocument());

      const fetchedLine = screen.getByText("aligned fetched line");
      const hideButton = screen.getByRole("button", { name: /hide unchanged lines/i });
      const gapControl = hideButton.closest("[data-testid='diff-gap-control']");
      expect(fetchedLine.closest("[data-testid='diff-gap-control']")).toBeNull();
      expect(gapControl).not.toBeNull();
      expect(gapControl).not.toHaveClass("px-3");
      expect(hideButton).toHaveClass("px-3", "py-1.5");
    });

    it("does NOT re-fetch when the same gap is clicked again (cache hit)", async () => {
      mockGetRange.mockResolvedValue([
        { lineNum: 4, content: "cached line" },
      ]);
      const user = userEvent.setup();
      const hunks: DiffHunk[] = [
        makeHunk({ oldStart: 1, oldLines: 3, newStart: 1, newLines: 3 }),
        makeHunk({
          oldStart: 10,
          oldLines: 3,
          newStart: 10,
          newLines: 3,
          header: "@@ -10,3 +10,3 @@",
          lines: [makeDiffLine({ kind: "addition", content: "later", oldLineNum: null, newLineNum: 10 })],
        }),
      ];

      render(
        <SimpleDiffView
          hunks={hunks}
          oldTotalLines={15}
          newTotalLines={15}
          {...rangeProps}
        />
      );

      // First click — fetch fires
      await user.click(screen.getByRole("button", { name: /show 6 unchanged lines/i }));
      await waitFor(() => expect(screen.getByText("cached line")).toBeInTheDocument());

      // Content is shown, "Show N" button is replaced by "Hide" button
      // Collapse it to get "Show" button back
      await user.click(screen.getByRole("button", { name: /hide unchanged lines/i }));

      // Second click — must NOT re-fetch
      await user.click(screen.getByRole("button", { name: /show 6 unchanged lines/i }));

      expect(mockGetRange).toHaveBeenCalledOnce();
      await waitFor(() => expect(screen.getByText("cached line")).toBeInTheDocument());
    });

    it("shows inline error and retry button on range fetch failure", async () => {
      mockGetRange.mockRejectedValue(new Error("network error"));
      const user = userEvent.setup();
      const hunks: DiffHunk[] = [
        makeHunk({ oldStart: 1, oldLines: 3, newStart: 1, newLines: 3 }),
        makeHunk({
          oldStart: 10,
          oldLines: 3,
          newStart: 10,
          newLines: 3,
          header: "@@ -10,3 +10,3 @@",
          lines: [makeDiffLine({ kind: "addition", content: "later", oldLineNum: null, newLineNum: 10 })],
        }),
      ];

      render(
        <SimpleDiffView
          hunks={hunks}
          oldTotalLines={15}
          newTotalLines={15}
          {...rangeProps}
        />
      );

      await user.click(screen.getByRole("button", { name: /show 6 unchanged lines/i }));
      await waitFor(() => expect(screen.getByTestId("gap-error")).toBeInTheDocument());
      expect(screen.getByRole("button", { name: /retry/i })).toBeInTheDocument();
    });

    it("retry button re-fires range fetch after error", async () => {
      mockGetRange
        .mockRejectedValueOnce(new Error("network error"))
        .mockResolvedValueOnce([{ lineNum: 4, content: "retried line" }]);
      const user = userEvent.setup();
      const hunks: DiffHunk[] = [
        makeHunk({ oldStart: 1, oldLines: 3, newStart: 1, newLines: 3 }),
        makeHunk({
          oldStart: 10,
          oldLines: 3,
          newStart: 10,
          newLines: 3,
          header: "@@ -10,3 +10,3 @@",
          lines: [makeDiffLine({ kind: "addition", content: "later", oldLineNum: null, newLineNum: 10 })],
        }),
      ];

      render(
        <SimpleDiffView
          hunks={hunks}
          oldTotalLines={15}
          newTotalLines={15}
          {...rangeProps}
        />
      );

      await user.click(screen.getByRole("button", { name: /show 6 unchanged lines/i }));
      await waitFor(() => expect(screen.getByTestId("gap-error")).toBeInTheDocument());

      await user.click(screen.getByRole("button", { name: /retry/i }));
      await waitFor(() => expect(screen.getByText("retried line")).toBeInTheDocument());
      expect(mockGetRange).toHaveBeenCalledTimes(2);
    });
  });
});
