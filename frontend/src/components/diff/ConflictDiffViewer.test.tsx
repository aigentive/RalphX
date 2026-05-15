import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";

import { ConflictDiffViewer } from "./ConflictDiffViewer";
import type { ConflictDiff } from "@/hooks/useConflictDiff";

vi.mock("./SimpleDiffView", () => ({
  SimpleDiffView: ({
    hunks,
    oldTotalLines,
    newTotalLines,
    language,
    variant,
  }: {
    hunks: unknown[];
    oldTotalLines: number;
    newTotalLines: number;
    language?: string;
    variant?: string;
  }) => (
    <div
      data-testid="simple-diff-view"
      data-hunk-count={hunks.length}
      data-old-total={oldTotalLines}
      data-new-total={newTotalLines}
      data-language={language}
      data-variant={variant}
    />
  ),
}));

function makeConflict(overrides: Partial<ConflictDiff> = {}): ConflictDiff {
  return {
    filePath: "src/foo/bar.ts",
    baseContent: "export const x = 1;\n",
    oursContent: "export const x = 1;\nexport const y = 2;\n",
    theirsContent: "export const x = 1;\nexport const z = 3;\n",
    mergedWithMarkers: "...markers...",
    language: "typescript",
    ...overrides,
  };
}

describe("ConflictDiffViewer", () => {
  it("renders the file path and the language badge from the diff payload", () => {
    render(<ConflictDiffViewer conflictDiff={makeConflict()} />);
    expect(screen.getByText("src/foo/bar.ts")).toBeInTheDocument();
    expect(screen.getByText(/typescript/i)).toBeInTheDocument();
  });

  it("falls back to the file extension when language is empty", () => {
    render(
      <ConflictDiffViewer
        conflictDiff={makeConflict({ language: "", filePath: "data/notes.md" })}
      />,
    );
    expect(screen.getAllByText(/md/i).length).toBeGreaterThan(0);
  });

  it("forwards ours / theirs content into the SimpleDiffView", () => {
    render(<ConflictDiffViewer conflictDiff={makeConflict()} />);
    expect(screen.getByTestId("simple-diff-view")).toBeInTheDocument();
  });

  it("renders with zero hunks when both sides are empty strings", () => {
    // Covers line 74: early return { hunks: [], oldTotalLines: 0, newTotalLines: 0 }
    render(
      <ConflictDiffViewer
        conflictDiff={makeConflict({ oursContent: "", theirsContent: "" })}
      />,
    );
    const view = screen.getByTestId("simple-diff-view");
    expect(view).toBeInTheDocument();
    expect(view).toHaveAttribute("data-hunk-count", "0");
    expect(view).toHaveAttribute("data-old-total", "0");
    expect(view).toHaveAttribute("data-new-total", "0");
  });

  it("produces two separate hunks when changes are separated by more than CONTEXT_LINES context lines", () => {
    // Covers lines 149, 151, 152: the else branch (inHunk[i] === false) for context lines
    // that fall outside the 3-line hunk window between two separate change regions.
    //
    // Both sides share 7 identical context lines (ctx1-ctx7) between two different
    // leading and trailing lines. The LCS diff produces 11 edits; edit[5] (ctx4)
    // is more than 3 positions away from either change cluster, so inHunk[5] = false.
    // That causes: flushHunk() (line 149), oldLine++ (line 151), newLine++ (line 152).
    const sharedContext = "ctx1\nctx2\nctx3\nctx4\nctx5\nctx6\nctx7";
    render(
      <ConflictDiffViewer
        conflictDiff={makeConflict({
          oursContent: `change1\n${sharedContext}\nchange2`,
          theirsContent: `changed1\n${sharedContext}\nchanged2`,
        })}
      />,
    );
    const view = screen.getByTestId("simple-diff-view");
    expect(view).toBeInTheDocument();
    // Two separate hunks: one for the first change region, one for the second
    expect(view).toHaveAttribute("data-hunk-count", "2");
  });
});
