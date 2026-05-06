import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";

import { ConflictDiffViewer } from "./ConflictDiffViewer";
import type { ConflictDiff } from "@/hooks/useConflictDiff";

vi.mock("./SimpleDiffView", () => ({
  SimpleDiffView: ({ oldCode, newCode }: { oldCode: string; newCode: string }) => (
    <div data-testid="simple-diff-view">
      <pre>{oldCode}</pre>
      <pre>{newCode}</pre>
    </div>
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
});
