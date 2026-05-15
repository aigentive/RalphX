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
import type { DiffHunk, DiffLine } from "@/api/diff";

// ── Mock diffApi ─────────────────────────────────────────────────────────
const mockGetRange = vi.fn();
vi.mock("@/api/diff", () => ({
  diffApi: {
    getAgentConversationWorkspaceFileContentRange: (...args: unknown[]) =>
      mockGetRange(...args),
  },
}));

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
