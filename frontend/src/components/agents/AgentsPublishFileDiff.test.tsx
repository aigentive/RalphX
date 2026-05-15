/**
 * AgentsPublishFileDiff tests — parent-managed diff prop interface.
 *
 * Performance contract:
 * - Header (file path, status badge, +/−) paints synchronously.
 * - Body (SimpleDiffView) only mounts when expanded AND diff data is present.
 * - Collapsed cards: no SimpleDiffView in DOM.
 * - Icon-only buttons: aria-label + Tooltip.
 */

import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

import { TooltipProvider } from "@/components/ui/tooltip";

// Mock SimpleDiffView (heavy body component)
vi.mock("@/components/diff/SimpleDiffView", () => ({
  SimpleDiffView: ({
    hunks,
    isBinary,
  }: {
    hunks: unknown[];
    isBinary?: boolean;
  }) => (
    <div
      data-testid="simple-diff-view"
      data-hunk-count={hunks.length}
      data-binary={String(isBinary ?? false)}
    >
      SimpleDiffView
    </div>
  ),
}));

import { AgentsPublishFileDiff } from "./AgentsPublishFileDiff";
import type { FileChange, FileDiff } from "@/api/diff";

function withProviders(node: React.ReactNode) {
  return <TooltipProvider delayDuration={0}>{node}</TooltipProvider>;
}

const makeFileChange = (overrides: Partial<FileChange> = {}): FileChange => ({
  path: "src/components/Foo.tsx",
  status: "modified",
  additions: 12,
  deletions: 3,
  ...overrides,
});

const makeDiff = (overrides: Partial<FileDiff> = {}): FileDiff => ({
  filePath: "src/components/Foo.tsx",
  language: "typescript",
  hunks: [
    {
      oldStart: 1,
      oldLines: 1,
      newStart: 1,
      newLines: 1,
      header: "@@ -1,1 +1,1 @@",
      lines: [
        { kind: "addition", content: "const new = 2;", oldLineNum: null, newLineNum: 1 },
      ],
    },
  ],
  oldTotalLines: 1,
  newTotalLines: 1,
  isBinary: false,
  ...overrides,
});

describe("AgentsPublishFileDiff", () => {
  const onToggle = vi.fn();
  const onCopyPath = vi.fn();
  const onOpenFullscreen = vi.fn();
  const onRetry = vi.fn();

  beforeEach(() => {
    vi.clearAllMocks();
  });

  describe("header — synchronous paint", () => {
    it("renders file path immediately", () => {
      render(
        withProviders(
          <AgentsPublishFileDiff
            file={makeFileChange({ path: "src/components/Foo.tsx" })}
            diff={undefined}
            isExpanded={false}
            onToggle={onToggle}
            onCopyPath={onCopyPath}
            onOpenFullscreen={onOpenFullscreen}
          />,
        ),
      );
      expect(screen.getByText("src/components/Foo.tsx")).toBeInTheDocument();
    });

    it("renders additions count immediately", () => {
      render(
        withProviders(
          <AgentsPublishFileDiff
            file={makeFileChange({ additions: 12, deletions: 3 })}
            diff={undefined}
            isExpanded={false}
            onToggle={onToggle}
            onCopyPath={onCopyPath}
            onOpenFullscreen={onOpenFullscreen}
          />,
        ),
      );
      expect(screen.getByText("+12")).toBeInTheDocument();
    });

    it("renders deletions count immediately", () => {
      render(
        withProviders(
          <AgentsPublishFileDiff
            file={makeFileChange({ additions: 12, deletions: 3 })}
            diff={undefined}
            isExpanded={false}
            onToggle={onToggle}
            onCopyPath={onCopyPath}
            onOpenFullscreen={onOpenFullscreen}
          />,
        ),
      );
      expect(screen.getByText("−3")).toBeInTheDocument();
    });

    it("shows 'M' status badge for modified files", () => {
      render(
        withProviders(
          <AgentsPublishFileDiff
            file={makeFileChange({ status: "modified" })}
            diff={undefined}
            isExpanded={false}
            onToggle={onToggle}
            onCopyPath={onCopyPath}
            onOpenFullscreen={onOpenFullscreen}
          />,
        ),
      );
      expect(screen.getByTestId("file-status-badge")).toHaveTextContent("M");
    });

    it("shows 'A' status badge for added files", () => {
      render(
        withProviders(
          <AgentsPublishFileDiff
            file={makeFileChange({ status: "added" })}
            diff={undefined}
            isExpanded={false}
            onToggle={onToggle}
            onCopyPath={onCopyPath}
            onOpenFullscreen={onOpenFullscreen}
          />,
        ),
      );
      expect(screen.getByTestId("file-status-badge")).toHaveTextContent("A");
    });

    it("shows 'D' status badge for deleted files", () => {
      render(
        withProviders(
          <AgentsPublishFileDiff
            file={makeFileChange({ status: "deleted" })}
            diff={undefined}
            isExpanded={false}
            onToggle={onToggle}
            onCopyPath={onCopyPath}
            onOpenFullscreen={onOpenFullscreen}
          />,
        ),
      );
      expect(screen.getByTestId("file-status-badge")).toHaveTextContent("D");
    });

    it("copy-path button calls onCopyPath with file path", async () => {
      const user = userEvent.setup();
      render(
        withProviders(
          <AgentsPublishFileDiff
            file={makeFileChange({ path: "src/Foo.tsx" })}
            diff={undefined}
            isExpanded={false}
            onToggle={onToggle}
            onCopyPath={onCopyPath}
            onOpenFullscreen={onOpenFullscreen}
          />,
        ),
      );
      await user.click(screen.getByTestId("file-diff-copy-path"));
      expect(onCopyPath).toHaveBeenCalledWith("src/Foo.tsx");
    });

    it("fullscreen button has aria-label", () => {
      render(
        withProviders(
          <AgentsPublishFileDiff
            file={makeFileChange()}
            diff={undefined}
            isExpanded={false}
            onToggle={onToggle}
            onCopyPath={onCopyPath}
            onOpenFullscreen={onOpenFullscreen}
          />,
        ),
      );
      const btn = screen.getByTestId("file-diff-open-fullscreen");
      expect(btn).toHaveAttribute("aria-label");
      expect(btn.getAttribute("aria-label")).not.toBe("");
    });

    it("fullscreen button calls onOpenFullscreen with file path", async () => {
      const user = userEvent.setup();
      render(
        withProviders(
          <AgentsPublishFileDiff
            file={makeFileChange({ path: "src/Foo.tsx" })}
            diff={undefined}
            isExpanded={false}
            onToggle={onToggle}
            onCopyPath={onCopyPath}
            onOpenFullscreen={onOpenFullscreen}
          />,
        ),
      );
      await user.click(screen.getByTestId("file-diff-open-fullscreen"));
      expect(onOpenFullscreen).toHaveBeenCalledWith("src/Foo.tsx");
    });

    it("toggle button calls onToggle when clicked", async () => {
      const user = userEvent.setup();
      render(
        withProviders(
          <AgentsPublishFileDiff
            file={makeFileChange()}
            diff={undefined}
            isExpanded={false}
            onToggle={onToggle}
            onCopyPath={onCopyPath}
            onOpenFullscreen={onOpenFullscreen}
          />,
        ),
      );
      await user.click(screen.getByTestId("file-diff-toggle"));
      expect(onToggle).toHaveBeenCalledOnce();
    });
  });

  describe("collapsed card — no body mounted", () => {
    it("does NOT mount SimpleDiffView when collapsed", () => {
      render(
        withProviders(
          <AgentsPublishFileDiff
            file={makeFileChange()}
            diff={makeDiff()}
            isExpanded={false}
            onToggle={onToggle}
            onCopyPath={onCopyPath}
            onOpenFullscreen={onOpenFullscreen}
          />,
        ),
      );
      expect(screen.queryByTestId("simple-diff-view")).toBeNull();
    });

    it("does NOT show loading skeleton when collapsed (even if diff='loading')", () => {
      render(
        withProviders(
          <AgentsPublishFileDiff
            file={makeFileChange()}
            diff="loading"
            isExpanded={false}
            onToggle={onToggle}
            onCopyPath={onCopyPath}
            onOpenFullscreen={onOpenFullscreen}
          />,
        ),
      );
      expect(screen.queryByTestId("file-diff-skeleton")).toBeNull();
    });
  });

  describe("expanded + loading", () => {
    it("shows loading skeleton when diff='loading' and expanded", () => {
      render(
        withProviders(
          <AgentsPublishFileDiff
            file={makeFileChange()}
            diff="loading"
            isExpanded={true}
            onToggle={onToggle}
            onCopyPath={onCopyPath}
            onOpenFullscreen={onOpenFullscreen}
          />,
        ),
      );
      expect(screen.getByTestId("file-diff-skeleton")).toBeInTheDocument();
    });

    it("does NOT mount SimpleDiffView when loading", () => {
      render(
        withProviders(
          <AgentsPublishFileDiff
            file={makeFileChange()}
            diff="loading"
            isExpanded={true}
            onToggle={onToggle}
            onCopyPath={onCopyPath}
            onOpenFullscreen={onOpenFullscreen}
          />,
        ),
      );
      expect(screen.queryByTestId("simple-diff-view")).toBeNull();
    });
  });

  describe("expanded + diff data", () => {
    it("mounts SimpleDiffView when expanded with diff data", () => {
      render(
        withProviders(
          <AgentsPublishFileDiff
            file={makeFileChange()}
            diff={makeDiff()}
            isExpanded={true}
            onToggle={onToggle}
            onCopyPath={onCopyPath}
            onOpenFullscreen={onOpenFullscreen}
          />,
        ),
      );
      expect(screen.getByTestId("simple-diff-view")).toBeInTheDocument();
    });
  });

  describe("expanded + error", () => {
    it("shows error state when diff='error' and expanded", () => {
      render(
        withProviders(
          <AgentsPublishFileDiff
            file={makeFileChange()}
            diff="error"
            isExpanded={true}
            onToggle={onToggle}
            onCopyPath={onCopyPath}
            onOpenFullscreen={onOpenFullscreen}
            onRetry={onRetry}
          />,
        ),
      );
      expect(screen.getByTestId("file-diff-error")).toBeInTheDocument();
    });

    it("calls onRetry when retry button is clicked", async () => {
      const user = userEvent.setup();
      render(
        withProviders(
          <AgentsPublishFileDiff
            file={makeFileChange()}
            diff="error"
            isExpanded={true}
            onToggle={onToggle}
            onCopyPath={onCopyPath}
            onOpenFullscreen={onOpenFullscreen}
            onRetry={onRetry}
          />,
        ),
      );
      await user.click(screen.getByTestId("file-diff-retry"));
      expect(onRetry).toHaveBeenCalledOnce();
    });

    it("does NOT mount SimpleDiffView in error state", () => {
      render(
        withProviders(
          <AgentsPublishFileDiff
            file={makeFileChange()}
            diff="error"
            isExpanded={true}
            onToggle={onToggle}
            onCopyPath={onCopyPath}
            onOpenFullscreen={onOpenFullscreen}
          />,
        ),
      );
      expect(screen.queryByTestId("simple-diff-view")).toBeNull();
    });
  });
});
