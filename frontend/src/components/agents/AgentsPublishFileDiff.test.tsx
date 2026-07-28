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
import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

import { TooltipProvider } from "@/components/ui/tooltip";

// Mock SimpleDiffView (heavy body component)
vi.mock("@/components/diff/SimpleDiffView", () => ({
  SimpleDiffView: ({
    hunks,
    isBinary,
    stickyGutter,
    hunkAnnotations,
  }: {
    hunks: unknown[];
    isBinary?: boolean;
    stickyGutter?: boolean;
    hunkAnnotations?: unknown[];
  }) => (
    <div
      data-testid="simple-diff-view"
      data-hunk-count={hunks.length}
      data-binary={String(isBinary ?? false)}
      data-sticky-gutter={String(stickyGutter ?? true)}
      data-hunk-annotation-count={String(hunkAnnotations?.length ?? 0)}
    >
      SimpleDiffView
    </div>
  ),
}));

vi.mock("@/components/diff/ConflictDiffViewer", () => ({
  ConflictDiffViewer: ({ conflictDiff }: { conflictDiff: { filePath: string } }) => (
    <div data-testid="conflict-diff-viewer" data-file-path={conflictDiff.filePath}>
      ConflictDiffViewer
    </div>
  ),
}));

vi.mock("@/components/diff/PagedDiffView", async () => {
  const React = await vi.importActual<typeof import("react")>("react");
  return {
    PagedDiffView: ({
    conversationId,
    filePath,
    refKind,
    scrollContainer,
    inlineScrollParent,
    defaultWrapLines,
    initialTotalRows,
    initialIsBinary,
    hunkAnnotations,
  }: {
    conversationId: string;
    filePath: string;
    refKind: { kind: string };
    scrollContainer?: boolean;
    inlineScrollParent?: HTMLElement | null;
    defaultWrapLines?: boolean;
    initialTotalRows?: number;
    initialIsBinary?: boolean;
    hunkAnnotations?: unknown[];
    }) => {
      const initialRefKind = React.useRef(refKind.kind);
      return (
        <div
          data-testid="paged-diff-view"
          data-conversation-id={conversationId}
          data-file-path={filePath}
          data-ref-kind={refKind.kind}
          data-initial-ref-kind={initialRefKind.current}
          data-scroll-container={String(scrollContainer ?? false)}
          data-inline-scroll-parent={String(Boolean(inlineScrollParent))}
          data-default-wrap-lines={String(defaultWrapLines ?? true)}
          data-initial-total-rows={initialTotalRows ?? ""}
          data-initial-is-binary={String(initialIsBinary ?? false)}
          data-hunk-annotation-count={String(hunkAnnotations?.length ?? 0)}
        >
          PagedDiffView
        </div>
      );
    },
  };
});

import { AgentsPublishFileDiff } from "./AgentsPublishFileDiff";
import type {
  ConflictDiff,
  FileChange,
  FileDiff,
  PrDiffAnnotation,
  WorkspaceReviewHunkAnnotation,
} from "@/api/diff";

function withProviders(node: React.ReactNode) {
  return <TooltipProvider delayDuration={0}>{node}</TooltipProvider>;
}

const makeFileChange = (overrides: Partial<FileChange> = {}): FileChange => ({
  path: "src/components/Foo.tsx",
  status: "modified",
  additions: 12,
  deletions: 3,
  isGenerated: false,
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

const makeConflictDiff = (
  overrides: Partial<ConflictDiff> = {},
): ConflictDiff => ({
  filePath: "src/components/Foo.tsx",
  baseContent: "base\n",
  oursContent: "ours\n",
  theirsContent: "theirs\n",
  mergedWithMarkers: "<<<<<<< HEAD\nours\n=======\ntheirs\n>>>>>>> branch\n",
  language: "typescript",
  ...overrides,
});

const makeAnnotation = (overrides: Partial<PrDiffAnnotation> = {}): PrDiffAnnotation => ({
  id: "annotation-1",
  source: "review_comment",
  path: "src/components/Foo.tsx",
  side: "right",
  startLine: 1,
  endLine: 1,
  startColumn: null,
  endColumn: null,
  level: "comment",
  status: null,
  title: null,
  message: "Please tighten this guard.",
  author: "octocat",
  checkName: null,
  url: null,
  isOutdated: false,
  createdAt: null,
  ...overrides,
});

const makeHunkAnnotation = (
  overrides: Partial<WorkspaceReviewHunkAnnotation> = {},
): WorkspaceReviewHunkAnnotation => ({
  id: "workspace-review-hunk-1",
  conversationId: "conv-1",
  projectId: "project-1",
  artifactId: "artifact-1",
  artifactVersion: 1,
  targetScope: "selected_source",
  headSha: "head-sha",
  diffFingerprint: "fingerprint-1",
  path: "src/components/Foo.tsx",
  diffSource: "selected_source",
  hunkHeader: "@@ -1,1 +1,1 @@",
  oldStart: 1,
  oldLines: 1,
  newStart: 1,
  newLines: 1,
  title: "Review summary",
  message: "This hunk updates the file diff card.",
  level: "notice",
  createdByRunId: "run-1",
  createdAt: "2026-07-01T00:00:00Z",
  ...overrides,
});

describe("AgentsPublishFileDiff", () => {
  const onToggle = vi.fn();
  const onCopyPath = vi.fn();
  const onOpenFullscreen = vi.fn();
  const onRetry = vi.fn();
  const onShowAnyway = vi.fn();

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
            shouldHydrate={true}
            isShowAnywayOverridden={false}
            onShowAnyway={onShowAnyway}
          />,
        ),
      );
      expect(screen.getByText("src/components/Foo.tsx")).toBeInTheDocument();
    });

    it("constrains the card to its row content width", () => {
      render(
        withProviders(
          <AgentsPublishFileDiff
            file={makeFileChange({ path: "src/components/Foo.tsx" })}
            diff={undefined}
            isExpanded={false}
            onToggle={onToggle}
            onCopyPath={onCopyPath}
            onOpenFullscreen={onOpenFullscreen}
            shouldHydrate={true}
            isShowAnywayOverridden={false}
            onShowAnyway={onShowAnyway}
          />,
        ),
      );

      expect(screen.getByTestId("publish-file-diff-src/components/Foo.tsx")).toHaveClass(
        "w-full",
        "max-w-full",
      );
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
            shouldHydrate={true}
            isShowAnywayOverridden={false}
            onShowAnyway={onShowAnyway}
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
            shouldHydrate={true}
            isShowAnywayOverridden={false}
            onShowAnyway={onShowAnyway}
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
            shouldHydrate={true}
            isShowAnywayOverridden={false}
            onShowAnyway={onShowAnyway}
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
            shouldHydrate={true}
            isShowAnywayOverridden={false}
            onShowAnyway={onShowAnyway}
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
            shouldHydrate={true}
            isShowAnywayOverridden={false}
            onShowAnyway={onShowAnyway}
          />,
        ),
      );
      expect(screen.getByTestId("file-status-badge")).toHaveTextContent("D");
    });

    it("renders annotation count immediately", () => {
      render(
        withProviders(
          <AgentsPublishFileDiff
            file={makeFileChange()}
            diff={undefined}
            isExpanded={false}
            onToggle={onToggle}
            onCopyPath={onCopyPath}
            onOpenFullscreen={onOpenFullscreen}
            shouldHydrate={true}
            annotations={[makeAnnotation(), makeAnnotation({ id: "annotation-2" })]}
            isShowAnywayOverridden={false}
            onShowAnyway={onShowAnyway}
          />,
        ),
      );

      expect(screen.getByTestId("file-diff-annotation-count")).toHaveTextContent("2");
    });

    it("renders workspace review hunk annotation count immediately", () => {
      render(
        withProviders(
          <AgentsPublishFileDiff
            file={makeFileChange()}
            diff={undefined}
            isExpanded={false}
            onToggle={onToggle}
            onCopyPath={onCopyPath}
            onOpenFullscreen={onOpenFullscreen}
            shouldHydrate={true}
            hunkAnnotations={[
              makeHunkAnnotation(),
              makeHunkAnnotation({ id: "workspace-review-hunk-2" }),
            ]}
            isShowAnywayOverridden={false}
            onShowAnyway={onShowAnyway}
          />,
        ),
      );

      expect(screen.getByTestId("file-diff-hunk-annotation-count")).toHaveTextContent(
        "2 review",
      );
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
            shouldHydrate={true}
            isShowAnywayOverridden={false}
            onShowAnyway={onShowAnyway}
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
            shouldHydrate={true}
            isShowAnywayOverridden={false}
            onShowAnyway={onShowAnyway}
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
            shouldHydrate={true}
            isShowAnywayOverridden={false}
            onShowAnyway={onShowAnyway}
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
            shouldHydrate={true}
            isShowAnywayOverridden={false}
            onShowAnyway={onShowAnyway}
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
            shouldHydrate={true}
            isShowAnywayOverridden={false}
            onShowAnyway={onShowAnyway}
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
            shouldHydrate={true}
            isShowAnywayOverridden={false}
            onShowAnyway={onShowAnyway}
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
            shouldHydrate={true}
            isShowAnywayOverridden={false}
            onShowAnyway={onShowAnyway}
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
            shouldHydrate={true}
            isShowAnywayOverridden={false}
            onShowAnyway={onShowAnyway}
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
            shouldHydrate={true}
            isShowAnywayOverridden={false}
            onShowAnyway={onShowAnyway}
          />,
        ),
      );
      expect(screen.getByTestId("simple-diff-view")).toBeInTheDocument();
    });

    it("passes non-sticky gutters to embedded SimpleDiffView fallback", () => {
      render(
        withProviders(
          <AgentsPublishFileDiff
            file={makeFileChange({ additions: 647, deletions: 72 })}
            diff={makeDiff()}
            isExpanded={true}
            onToggle={onToggle}
            onCopyPath={onCopyPath}
            onOpenFullscreen={onOpenFullscreen}
            conversationId="conv-1"
            shouldHydrate={true}
            isShowAnywayOverridden={false}
            onShowAnyway={onShowAnyway}
          />,
        ),
      );

      expect(screen.getByTestId("simple-diff-view")).toHaveAttribute(
        "data-sticky-gutter",
        "false",
      );
      expect(screen.queryByTestId("paged-diff-view")).toBeNull();
    });

    it("passes workspace review hunk annotations into SimpleDiffView", () => {
      render(
        withProviders(
          <AgentsPublishFileDiff
            file={makeFileChange()}
            diff={makeDiff()}
            isExpanded={true}
            onToggle={onToggle}
            onCopyPath={onCopyPath}
            onOpenFullscreen={onOpenFullscreen}
            shouldHydrate={true}
            hunkAnnotations={[makeHunkAnnotation()]}
            isShowAnywayOverridden={false}
            onShowAnyway={onShowAnyway}
          />,
        ),
      );

      expect(screen.getByTestId("simple-diff-view")).toHaveAttribute(
        "data-hunk-annotation-count",
        "1",
      );
    });

    it("mounts PagedDiffView for a medium hydrated diff when page refs are available", () => {
      render(
        withProviders(
          <AgentsPublishFileDiff
            file={makeFileChange({ additions: 647, deletions: 72 })}
            diff={undefined}
            isExpanded={true}
            onToggle={onToggle}
            onCopyPath={onCopyPath}
            onOpenFullscreen={onOpenFullscreen}
            conversationId="conv-1"
            diffPageRefKind={{ kind: "head" }}
            diffPageSummary={{ totalRows: 719, isBinary: false }}
            shouldHydrate={true}
            isShowAnywayOverridden={false}
            onShowAnyway={onShowAnyway}
          />,
        ),
      );

      expect(screen.getByTestId("paged-diff-view")).toHaveAttribute(
        "data-file-path",
        "src/components/Foo.tsx",
      );
      expect(screen.getByTestId("paged-diff-view")).toHaveAttribute(
        "data-ref-kind",
        "head",
      );
      expect(screen.getByTestId("paged-diff-view")).toHaveAttribute(
        "data-default-wrap-lines",
        "false",
      );
      expect(screen.getByTestId("paged-diff-view")).toHaveAttribute(
        "data-initial-total-rows",
        "719",
      );
      expect(screen.getByTestId("paged-diff-view")).toHaveAttribute(
        "data-initial-is-binary",
        "false",
      );
      expect(screen.queryByTestId("simple-diff-view")).toBeNull();
    });

    it("remounts paged rows when the diff ref identity changes", () => {
      const props = {
        file: makeFileChange({ additions: 400 }),
        diff: undefined,
        isExpanded: true,
        onToggle,
        onCopyPath,
        onOpenFullscreen,
        conversationId: "conv-1",
        shouldHydrate: true,
        isShowAnywayOverridden: false,
        onShowAnyway,
      } as const;
      const { rerender } = render(
        withProviders(
          <AgentsPublishFileDiff
            {...props}
            diffPageRefKind={{ kind: "head" }}
          />,
        ),
      );
      expect(screen.getByTestId("paged-diff-view")).toHaveAttribute(
        "data-initial-ref-kind",
        "head",
      );

      rerender(
        withProviders(
          <AgentsPublishFileDiff
            {...props}
            diffPageRefKind={{ kind: "cumulative_head" }}
          />,
        ),
      );

      expect(screen.getByTestId("paged-diff-view")).toHaveAttribute(
        "data-initial-ref-kind",
        "cumulative_head",
      );
    });

    it("falls back to SimpleDiffView for a medium diff without page refs", () => {
      render(
        withProviders(
          <AgentsPublishFileDiff
            file={makeFileChange({ additions: 647, deletions: 72 })}
            diff={makeDiff()}
            isExpanded={true}
            onToggle={onToggle}
            onCopyPath={onCopyPath}
            onOpenFullscreen={onOpenFullscreen}
            conversationId="conv-1"
            shouldHydrate={true}
            isShowAnywayOverridden={false}
            onShowAnyway={onShowAnyway}
          />,
        ),
      );

      expect(screen.getByTestId("simple-diff-view")).toBeInTheDocument();
      expect(screen.queryByTestId("paged-diff-view")).toBeNull();
    });

    it("does not render PagedDiffView for conflict diffs", () => {
      render(
        withProviders(
          <AgentsPublishFileDiff
            file={makeFileChange({ additions: 647, deletions: 72 })}
            diff={undefined}
            conflictDiff={makeConflictDiff()}
            isConflictMode={true}
            isExpanded={true}
            onToggle={onToggle}
            onCopyPath={onCopyPath}
            onOpenFullscreen={onOpenFullscreen}
            conversationId="conv-1"
            diffPageRefKind={{ kind: "head" }}
            shouldHydrate={true}
            isShowAnywayOverridden={false}
            onShowAnyway={onShowAnyway}
          />,
        ),
      );

      expect(screen.getByTestId("conflict-diff-viewer")).toBeInTheDocument();
      expect(screen.queryByTestId("paged-diff-view")).toBeNull();
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
            shouldHydrate={true}
            isShowAnywayOverridden={false}
            onShowAnyway={onShowAnyway}
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
            shouldHydrate={true}
            isShowAnywayOverridden={false}
            onShowAnyway={onShowAnyway}
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
            shouldHydrate={true}
            isShowAnywayOverridden={false}
            onShowAnyway={onShowAnyway}
          />,
        ),
      );
      expect(screen.queryByTestId("simple-diff-view")).toBeNull();
    });
  });

  describe("lazy hydration — shouldHydrate prop", () => {
    it("shows an explicit loading state when shouldHydrate=false and expanded", () => {
      render(
        withProviders(
          <AgentsPublishFileDiff
            file={makeFileChange()}
            diff={makeDiff()}
            isExpanded={true}
            onToggle={onToggle}
            onCopyPath={onCopyPath}
            onOpenFullscreen={onOpenFullscreen}
            shouldHydrate={false}
            isShowAnywayOverridden={false}
            onShowAnyway={onShowAnyway}
          />,
        ),
      );
      expect(screen.getByTestId("file-diff-pre-hydration")).toHaveAttribute(
        "aria-label",
        "Loading file diff",
      );
      expect(screen.getByTestId("file-diff-pre-hydration")).toHaveAttribute(
        "aria-busy",
        "true",
      );
      expect(screen.getByTestId("file-diff-pre-hydration")).toHaveTextContent(
        "Loading diff",
      );
      expect(screen.queryByTestId("simple-diff-view")).toBeNull();
    });

    it("does NOT show pre-hydration placeholder when collapsed", () => {
      render(
        withProviders(
          <AgentsPublishFileDiff
            file={makeFileChange()}
            diff={makeDiff()}
            isExpanded={false}
            onToggle={onToggle}
            onCopyPath={onCopyPath}
            onOpenFullscreen={onOpenFullscreen}
            shouldHydrate={false}
            isShowAnywayOverridden={false}
            onShowAnyway={onShowAnyway}
          />,
        ),
      );
      expect(screen.queryByTestId("file-diff-pre-hydration")).toBeNull();
    });

    it("mounts SimpleDiffView when shouldHydrate=true and expanded with diff data", () => {
      render(
        withProviders(
          <AgentsPublishFileDiff
            file={makeFileChange()}
            diff={makeDiff()}
            isExpanded={true}
            onToggle={onToggle}
            onCopyPath={onCopyPath}
            onOpenFullscreen={onOpenFullscreen}
            shouldHydrate={true}
            isShowAnywayOverridden={false}
            onShowAnyway={onShowAnyway}
          />,
        ),
      );
      expect(screen.getByTestId("simple-diff-view")).toBeInTheDocument();
      expect(screen.queryByTestId("file-diff-pre-hydration")).toBeNull();
    });

    it("still renders header synchronously when shouldHydrate=false", () => {
      render(
        withProviders(
          <AgentsPublishFileDiff
            file={makeFileChange({ path: "src/Heavy.tsx" })}
            diff={makeDiff()}
            isExpanded={true}
            onToggle={onToggle}
            onCopyPath={onCopyPath}
            onOpenFullscreen={onOpenFullscreen}
            shouldHydrate={false}
            isShowAnywayOverridden={false}
            onShowAnyway={onShowAnyway}
          />,
        ),
      );
      expect(screen.getByText("src/Heavy.tsx")).toBeInTheDocument();
    });
  });

  describe("generated-file placeholder", () => {
    it("shows generated placeholder when isGenerated=true and not overridden", () => {
      render(
        withProviders(
          <AgentsPublishFileDiff
            file={makeFileChange({ isGenerated: true, additions: 100, deletions: 50 })}
            diff={makeDiff()}
            isExpanded={true}
            onToggle={onToggle}
            onCopyPath={onCopyPath}
            onOpenFullscreen={onOpenFullscreen}
            conversationId="conv-1"
            diffPageRefKind={{ kind: "head" }}
            shouldHydrate={true}
            isShowAnywayOverridden={false}
            onShowAnyway={onShowAnyway}
          />,
        ),
      );
      expect(screen.getByTestId("file-diff-generated-placeholder")).toBeInTheDocument();
      expect(screen.queryByTestId("simple-diff-view")).toBeNull();
      expect(screen.queryByTestId("paged-diff-view")).toBeNull();
    });

    it("shows +N and -M counts in generated placeholder", () => {
      render(
        withProviders(
          <AgentsPublishFileDiff
            file={makeFileChange({ isGenerated: true, additions: 100, deletions: 50 })}
            diff={makeDiff()}
            isExpanded={true}
            onToggle={onToggle}
            onCopyPath={onCopyPath}
            onOpenFullscreen={onOpenFullscreen}
            shouldHydrate={true}
            isShowAnywayOverridden={false}
            onShowAnyway={onShowAnyway}
          />,
        ),
      );
      const placeholder = screen.getByTestId("file-diff-generated-placeholder");
      expect(within(placeholder).getByText("+100")).toBeInTheDocument();
      expect(within(placeholder).getByText("−50")).toBeInTheDocument();
    });

    it("clicking 'Show anyway' calls onShowAnyway", async () => {
      const user = userEvent.setup();
      render(
        withProviders(
          <AgentsPublishFileDiff
            file={makeFileChange({ isGenerated: true })}
            diff={makeDiff()}
            isExpanded={true}
            onToggle={onToggle}
            onCopyPath={onCopyPath}
            onOpenFullscreen={onOpenFullscreen}
            shouldHydrate={true}
            isShowAnywayOverridden={false}
            onShowAnyway={onShowAnyway}
          />,
        ),
      );
      await user.click(screen.getByTestId("file-diff-show-anyway"));
      expect(onShowAnyway).toHaveBeenCalledOnce();
    });

    it("mounts SimpleDiffView when isGenerated=true and isShowAnywayOverridden=true", () => {
      render(
        withProviders(
          <AgentsPublishFileDiff
            file={makeFileChange({ isGenerated: true })}
            diff={makeDiff()}
            isExpanded={true}
            onToggle={onToggle}
            onCopyPath={onCopyPath}
            onOpenFullscreen={onOpenFullscreen}
            shouldHydrate={true}
            isShowAnywayOverridden={true}
            onShowAnyway={onShowAnyway}
          />,
        ),
      );
      expect(screen.getByTestId("simple-diff-view")).toBeInTheDocument();
      expect(screen.queryByTestId("file-diff-generated-placeholder")).toBeNull();
    });

    it("auto-mounts PagedDiffView for a large hydrated diff", () => {
      render(
        withProviders(
          <AgentsPublishFileDiff
            file={makeFileChange({ additions: 1_250, deletions: 25 })}
            diff={undefined}
            isExpanded={true}
            onToggle={onToggle}
            onCopyPath={onCopyPath}
            onOpenFullscreen={onOpenFullscreen}
            conversationId="conv-1"
            diffPageRefKind={{ kind: "head" }}
            shouldHydrate={true}
            isShowAnywayOverridden={false}
            onShowAnyway={onShowAnyway}
          />,
        ),
      );
      expect(screen.getByTestId("paged-diff-view")).toHaveAttribute("data-ref-kind", "head");
      expect(screen.getByTestId("paged-diff-view")).toHaveAttribute(
        "data-scroll-container",
        "false",
      );
      expect(screen.queryByTestId("simple-diff-view")).toBeNull();
      expect(screen.queryByTestId("file-diff-large-placeholder")).toBeNull();
    });

    it("passes workspace review hunk annotations into PagedDiffView", () => {
      render(
        withProviders(
          <AgentsPublishFileDiff
            file={makeFileChange({ additions: 1_250, deletions: 25 })}
            diff={undefined}
            isExpanded={true}
            onToggle={onToggle}
            onCopyPath={onCopyPath}
            onOpenFullscreen={onOpenFullscreen}
            conversationId="conv-1"
            diffPageRefKind={{ kind: "head" }}
            shouldHydrate={true}
            hunkAnnotations={[makeHunkAnnotation()]}
            isShowAnywayOverridden={false}
            onShowAnyway={onShowAnyway}
          />,
        ),
      );

      expect(screen.getByTestId("paged-diff-view")).toHaveAttribute(
        "data-hunk-annotation-count",
        "1",
      );
    });

    it("defers a large paged diff until the file row is hydrated", () => {
      render(
        withProviders(
          <AgentsPublishFileDiff
            file={makeFileChange({ additions: 1_250, deletions: 25 })}
            diff={undefined}
            isExpanded={true}
            onToggle={onToggle}
            onCopyPath={onCopyPath}
            onOpenFullscreen={onOpenFullscreen}
            conversationId="conv-1"
            diffPageRefKind={{ kind: "head" }}
            shouldHydrate={false}
            isShowAnywayOverridden={false}
            onShowAnyway={onShowAnyway}
          />,
        ),
      );
      expect(screen.getByTestId("file-diff-pre-hydration")).toBeInTheDocument();
      expect(screen.queryByTestId("paged-diff-view")).toBeNull();
      expect(screen.queryByTestId("simple-diff-view")).toBeNull();
      expect(screen.queryByTestId("file-diff-large-placeholder")).toBeNull();
    });

    it("does NOT show generated placeholder when collapsed (even if isGenerated=true)", () => {
      render(
        withProviders(
          <AgentsPublishFileDiff
            file={makeFileChange({ isGenerated: true })}
            diff={makeDiff()}
            isExpanded={false}
            onToggle={onToggle}
            onCopyPath={onCopyPath}
            onOpenFullscreen={onOpenFullscreen}
            shouldHydrate={true}
            isShowAnywayOverridden={false}
            onShowAnyway={onShowAnyway}
          />,
        ),
      );
      expect(screen.queryByTestId("file-diff-generated-placeholder")).toBeNull();
    });

    it("generated placeholder has accessible name on Show anyway button", () => {
      render(
        withProviders(
          <AgentsPublishFileDiff
            file={makeFileChange({ isGenerated: true })}
            diff={makeDiff()}
            isExpanded={true}
            onToggle={onToggle}
            onCopyPath={onCopyPath}
            onOpenFullscreen={onOpenFullscreen}
            shouldHydrate={true}
            isShowAnywayOverridden={false}
            onShowAnyway={onShowAnyway}
          />,
        ),
      );
      const btn = screen.getByTestId("file-diff-show-anyway");
      expect(btn).toHaveAttribute("aria-label");
      expect(btn.getAttribute("aria-label")).not.toBe("");
    });
  });
});
