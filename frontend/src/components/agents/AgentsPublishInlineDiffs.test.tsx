/**
 * AgentsPublishInlineDiffs tests
 * Container that orchestrates fetch-management + filter + file cards.
 * Receives (conversationId, review, commits) from parent — no re-fetching at parent level.
 */

import { describe, it, expect, vi, beforeEach } from "vitest";
import { act, render, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";

import { TooltipProvider } from "@/components/ui/tooltip";

type MockListRange = { startIndex: number; endIndex: number };

const virtuosoMockState = vi.hoisted(() => ({
  range: null as MockListRange | null,
  rangeChanged: null as ((range: MockListRange) => void) | null,
  scrollToIndex: vi.fn(),
}));
const annotationScrollIntoViewMock = vi.hoisted(() => vi.fn());

vi.mock("react-virtuoso", async () => {
  const React = await vi.importActual<typeof import("react")>("react");

  type VirtuosoMockProps = {
    data?: unknown[];
    itemContent?: (index: number, item: unknown) => React.ReactNode;
    computeItemKey?: (index: number, item: unknown) => React.Key;
    rangeChanged?: (range: MockListRange) => void;
    scrollerRef?: (ref: HTMLElement | Window | null) => void;
    className?: string;
    style?: React.CSSProperties;
    "data-testid"?: string;
  };

  const Virtuoso = React.forwardRef<unknown, VirtuosoMockProps>(function MockVirtuoso(
    props,
    ref,
  ) {
    const { scrollerRef: onScrollerRef } = props;
    const data = props.data ?? [];
    const scrollerElementRef = React.useRef<HTMLDivElement | null>(null);
    const startIndex = virtuosoMockState.range?.startIndex ?? 0;
    const endIndex = virtuosoMockState.range?.endIndex ?? data.length - 1;
    const visibleItems = data
      .map((item, index) => ({ item, index }))
      .slice(startIndex, endIndex + 1);

    React.useImperativeHandle(
      ref,
      () => ({
        scrollToIndex: virtuosoMockState.scrollToIndex,
      }),
      [],
    );

    React.useEffect(() => {
      virtuosoMockState.rangeChanged = props.rangeChanged ?? null;
      return () => {
        if (virtuosoMockState.rangeChanged === props.rangeChanged) {
          virtuosoMockState.rangeChanged = null;
        }
      };
    }, [props.rangeChanged]);

    React.useEffect(() => {
      onScrollerRef?.(scrollerElementRef.current);
      return () => onScrollerRef?.(null);
    }, [onScrollerRef]);

    return (
      <div
        ref={scrollerElementRef}
        data-testid={props["data-testid"] ?? "mock-virtuoso"}
        data-count={data.length}
        className={props.className}
        style={props.style}
      >
        {visibleItems.map(({ item, index }) => (
          <div
            key={props.computeItemKey?.(index, item) ?? index}
            data-testid={`mock-virtuoso-item-${index}`}
          >
            {props.itemContent?.(index, item)}
          </div>
        ))}
      </div>
    );
  });

  return { Virtuoso };
});

// Stub child components
vi.mock("./AgentsPublishDiffFilter", () => ({
  AgentsPublishDiffFilter: ({
    mode,
    onModeChange,
    workspaceChangeCount,
    workspaceChangeLabel,
    cumulativeModeLabel,
    stagedCount,
    unstagedCount,
    conflictedCount,
    supportsWorktreeModes,
  }: {
    mode: string;
    onModeChange: (m: string) => void;
    workspaceChangeCount: number;
    workspaceChangeLabel?: string;
    cumulativeModeLabel?: string;
    stagedCount?: number;
    unstagedCount?: number;
    conflictedCount?: number;
    commits: unknown[];
    supportsWorktreeModes?: boolean;
  }) => (
    <div
      data-testid="mock-diff-filter"
      data-mode={mode}
      data-count={workspaceChangeCount}
      data-label={workspaceChangeLabel ?? ""}
      data-cumulative-label={cumulativeModeLabel ?? ""}
      data-staged-count={stagedCount ?? ""}
      data-unstaged-count={unstagedCount ?? ""}
      data-conflicted-count={conflictedCount ?? ""}
      data-supports-worktree-modes={String(supportsWorktreeModes)}
    >
      <button onClick={() => onModeChange("uncommitted")}>
        {workspaceChangeLabel ?? "Workspace changes"}
      </button>
      <button onClick={() => onModeChange("conflicted")}>Conflicted</button>
      <button onClick={() => onModeChange("sha-abc")}>Commit sha-abc</button>
      <button onClick={() => onModeChange("staged")}>Staged</button>
      <button onClick={() => onModeChange("unstaged")}>Unstaged</button>
      <button onClick={() => onModeChange("cumulative")}>All commits</button>
    </div>
  ),
}));

vi.mock("./AgentsPublishFileDiff", () => ({
  AgentsPublishFileDiff: ({
    file,
    diff,
    conflictDiff,
    isExpanded,
    onToggle,
    onCopyPath,
    onOpenFullscreen,
    refKind,
    diffPageRefKind,
    diffPageReloadKey,
    inlineDiffScrollParent,
    diffPageSummary,
    conversationId,
    shouldHydrate,
    annotations,
    hunkAnnotations,
    isShowAnywayOverridden,
    onShowAnyway,
    isFocusTarget,
  }: {
    file: { path: string };
    diff: unknown;
    conflictDiff?: unknown;
    isExpanded: boolean;
    onToggle: () => void;
    onCopyPath: (p: string) => void;
    onOpenFullscreen: (p: string) => void;
    onRetry?: () => void;
    refKind?: { kind: string };
    diffPageRefKind?: { kind: string };
    diffPageReloadKey?: string;
    inlineDiffScrollParent?: HTMLElement | null;
    diffPageSummary?: { totalRows: number; isBinary: boolean };
    conversationId?: string;
    shouldHydrate: boolean;
    annotations?: unknown[];
    hunkAnnotations?: unknown[];
    isShowAnywayOverridden: boolean;
    onShowAnyway: () => void;
    isFocusTarget?: boolean;
  }) => (
    <div
      data-testid={`mock-file-diff-${file.path.replace(/\//g, "-")}`}
      data-expanded={String(isExpanded)}
      data-diff-status={typeof diff === "string" ? diff : diff ? "loaded" : "undefined"}
      data-conflict-diff-status={
        typeof conflictDiff === "string" ? conflictDiff : conflictDiff ? "loaded" : "undefined"
      }
      data-ref-kind={refKind?.kind}
      data-diff-page-ref-kind={diffPageRefKind?.kind}
      data-diff-page-reload-key={diffPageReloadKey}
      data-inline-scroll-parent={String(Boolean(inlineDiffScrollParent))}
      data-diff-page-total-rows={diffPageSummary?.totalRows ?? ""}
      data-diff-page-is-binary={String(diffPageSummary?.isBinary ?? false)}
      data-conversation-id={conversationId}
      data-should-hydrate={String(shouldHydrate)}
      data-annotation-count={String(annotations?.length ?? 0)}
      data-hunk-annotation-count={String(hunkAnnotations?.length ?? 0)}
      data-show-anyway-overridden={String(isShowAnywayOverridden)}
      data-focus-target={String(isFocusTarget)}
    >
      <button
        data-testid={`mock-file-toggle-${file.path.replace(/\//g, "-")}`}
        onClick={onToggle}
      >
        toggle {file.path}
      </button>
      <button onClick={() => onCopyPath(file.path)}>copy</button>
      <button onClick={() => onOpenFullscreen(file.path)}>fullscreen</button>
      <button
        data-testid={`show-anyway-${file.path.replace(/\//g, "-")}`}
        onClick={onShowAnyway}
      >
        Show anyway
      </button>
      {shouldHydrate && (annotations?.length ?? 0) > 0 && (
        <div data-testid="diff-annotation-row">annotation</div>
      )}
      {shouldHydrate &&
        (hunkAnnotations?.length ?? 0) > 0 &&
        file.path.includes("Paged") && (
          <div
            data-testid="delayed-paged-hunk-annotation-host"
            ref={(node: HTMLDivElement | null) => {
              if (!node || node.dataset.hunkAnnotationScheduled === "true") {
                return;
              }
              node.dataset.hunkAnnotationScheduled = "true";
              window.setTimeout(() => {
                const row = document.createElement("div");
                row.dataset.testid = "diff-hunk-annotation-row";
                row.textContent = "hunk annotation";
                node.appendChild(row);
              }, 20);
            }}
          />
        )}
      {shouldHydrate &&
        (hunkAnnotations?.length ?? 0) > 0 &&
        !file.path.includes("Paged") && (
        <div data-testid="diff-hunk-annotation-row">hunk annotation</div>
      )}
      {file.path}
    </div>
  ),
}));

// Mock diffApi
const mockGetUncommittedDiff = vi.fn();
const mockGetCommitDiff = vi.fn();
const mockGetCommitFiles = vi.fn();
const mockGetStagedFiles = vi.fn();
const mockGetUnstagedFiles = vi.fn();
const mockGetCumulativeFiles = vi.fn();
const mockGetStagedFileDiff = vi.fn();
const mockGetUnstagedFileDiff = vi.fn();
const mockGetCumulativeFileDiff = vi.fn();
const mockGetRepairConflictFileDiff = vi.fn();
const mockGetRepairStagedFiles = vi.fn();
const mockGetRepairUnstagedFiles = vi.fn();
const mockGetRepairStagedFileDiff = vi.fn();
const mockGetRepairUnstagedFileDiff = vi.fn();
const mockGetDiffPage = vi.fn();

vi.mock("@/api/diff", () => ({
  diffApi: {
    getAgentConversationWorkspaceFileDiff: (...args: unknown[]) =>
      mockGetUncommittedDiff(...args),
    getAgentConversationWorkspaceCommitFileDiff: (...args: unknown[]) =>
      mockGetCommitDiff(...args),
    getAgentConversationWorkspaceCommitFileChanges: (...args: unknown[]) =>
      mockGetCommitFiles(...args),
    getAgentConversationWorkspaceStagedFileChanges: (...args: unknown[]) =>
      mockGetStagedFiles(...args),
    getAgentConversationWorkspaceUnstagedFileChanges: (...args: unknown[]) =>
      mockGetUnstagedFiles(...args),
    getAgentConversationWorkspaceRepairStagedFileChanges: (...args: unknown[]) =>
      mockGetRepairStagedFiles(...args),
    getAgentConversationWorkspaceRepairUnstagedFileChanges: (...args: unknown[]) =>
      mockGetRepairUnstagedFiles(...args),
    getAgentConversationWorkspaceCumulativeFileChanges: (...args: unknown[]) =>
      mockGetCumulativeFiles(...args),
    getAgentConversationWorkspaceStagedFileDiff: (...args: unknown[]) =>
      mockGetStagedFileDiff(...args),
    getAgentConversationWorkspaceUnstagedFileDiff: (...args: unknown[]) =>
      mockGetUnstagedFileDiff(...args),
    getAgentConversationWorkspaceRepairStagedFileDiff: (...args: unknown[]) =>
      mockGetRepairStagedFileDiff(...args),
    getAgentConversationWorkspaceRepairUnstagedFileDiff: (...args: unknown[]) =>
      mockGetRepairUnstagedFileDiff(...args),
    getAgentConversationWorkspaceCumulativeFileDiff: (...args: unknown[]) =>
      mockGetCumulativeFileDiff(...args),
    getAgentConversationWorkspaceRepairConflictFileDiff: (...args: unknown[]) =>
      mockGetRepairConflictFileDiff(...args),
    getAgentConversationWorkspaceFileDiffPage: (...args: unknown[]) =>
      mockGetDiffPage(...args),
  },
}));

function fireVirtualRange(startIndex: number, endIndex: number) {
  act(() => {
    virtuosoMockState.range = { startIndex, endIndex };
    virtuosoMockState.rangeChanged?.({ startIndex, endIndex });
  });
}

import { AgentsPublishInlineDiffs } from "./AgentsPublishInlineDiffs";
import type {
  FileChange,
  PrDiffAnnotation,
  WorkspaceReviewHunkAnnotation,
} from "@/api/diff";
import type { Commit as DiffViewerCommit } from "@/components/diff";
import type { AgentWorkspaceReview } from "@/api/diff";
import { agentWorkspaceKeys } from "./agentWorkspaceQueries";

function makeQueryClient() {
  return new QueryClient({
    defaultOptions: { queries: { retry: false, gcTime: 0 } },
  });
}

function withProviders(node: React.ReactNode, client?: QueryClient) {
  const qc = client ?? makeQueryClient();
  return (
    <QueryClientProvider client={qc}>
      <TooltipProvider delayDuration={0}>{node}</TooltipProvider>
    </QueryClientProvider>
  );
}

const makeFileChange = (path: string, overrides: Partial<FileChange> = {}): FileChange => ({
  path,
  status: "modified",
  additions: 5,
  deletions: 2,
  isGenerated: false,
  ...overrides,
});

const makeReview = (changes: FileChange[] = []): AgentWorkspaceReview => ({
  baseRef: "main",
  headRef: "feature/test",
  changes,
  commits: [],
});

const makeCommit = (sha: string): DiffViewerCommit => ({
  sha,
  shortSha: sha.slice(0, 7),
  message: `commit ${sha}`,
  author: "Alice",
  date: new Date("2026-01-01"),
});

const makeAnnotation = (
  path: string | null,
  overrides: Partial<PrDiffAnnotation> = {},
): PrDiffAnnotation => ({
  id: `annotation-${path ?? "none"}`,
  source: "check_run",
  path,
  side: "right",
  startLine: 1,
  endLine: 1,
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
});

const makeHunkAnnotation = (
  path: string,
  overrides: Partial<WorkspaceReviewHunkAnnotation> = {},
): WorkspaceReviewHunkAnnotation => ({
  id: `workspace-review-hunk-${path}`,
  conversationId: "conv-1",
  projectId: "project-1",
  artifactId: "artifact-1",
  artifactVersion: 1,
  targetScope: "selected_source",
  headSha: "head-sha",
  diffFingerprint: "fingerprint-1",
  path,
  diffSource: "selected_source",
  hunkHeader: "@@ -1,1 +1,1 @@",
  oldStart: 1,
  oldLines: 1,
  newStart: 1,
  newLines: 1,
  title: "Review summary",
  message: "This hunk updates inline diffs.",
  level: "notice",
  createdByRunId: "run-1",
  createdAt: "2026-07-01T00:00:00Z",
  ...overrides,
});

describe("AgentsPublishInlineDiffs", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    Object.defineProperty(HTMLElement.prototype, "scrollIntoView", {
      configurable: true,
      value: annotationScrollIntoViewMock,
    });
    annotationScrollIntoViewMock.mockClear();
    virtuosoMockState.range = null;
    virtuosoMockState.rangeChanged = null;
    virtuosoMockState.scrollToIndex.mockClear();
    const makeHunkDiff = (filePath: string) => ({
      filePath,
      language: "typescript",
      hunks: [
        {
          oldStart: 1,
          oldLines: 1,
          newStart: 1,
          newLines: 1,
          header: "@@ -1,1 +1,1 @@",
          lines: [{ kind: "addition", content: "new", oldLineNum: null, newLineNum: 1 }],
        },
      ],
      oldTotalLines: 1,
      newTotalLines: 1,
      isBinary: false,
    });
    const makeConflictDiff = (filePath: string) => ({
      filePath,
      baseContent: "base\n",
      oursContent: "ours\n",
      theirsContent: "theirs\n",
      mergedWithMarkers: "<<<<<<< HEAD\nours\n=======\ntheirs\n>>>>>>> branch\n",
      language: "typescript",
    });

    mockGetUncommittedDiff.mockResolvedValue(makeHunkDiff("src/Foo.tsx"));
    mockGetCommitDiff.mockResolvedValue(makeHunkDiff("src/Foo.tsx"));
    mockGetCommitFiles.mockResolvedValue([makeFileChange("src/CommitOnly.tsx")]);
    mockGetStagedFiles.mockResolvedValue([]);
    mockGetUnstagedFiles.mockResolvedValue([]);
    mockGetRepairStagedFiles.mockResolvedValue([]);
    mockGetRepairUnstagedFiles.mockResolvedValue([]);
    mockGetCumulativeFiles.mockResolvedValue([makeFileChange("src/CumulativeFile.tsx")]);
    mockGetStagedFileDiff.mockResolvedValue(makeHunkDiff("src/StagedFile.tsx"));
    mockGetUnstagedFileDiff.mockResolvedValue(makeHunkDiff("src/UnstagedFile.tsx"));
    mockGetRepairStagedFileDiff.mockResolvedValue(makeHunkDiff("src/StagedFile.tsx"));
    mockGetRepairUnstagedFileDiff.mockResolvedValue(makeHunkDiff("src/UnstagedFile.tsx"));
    mockGetCumulativeFileDiff.mockResolvedValue(makeHunkDiff("src/CumulativeFile.tsx"));
    mockGetRepairConflictFileDiff.mockResolvedValue(makeConflictDiff("src/Conflict.tsx"));
    mockGetDiffPage.mockResolvedValue({
      filePath: "src/Foo.tsx",
      language: "typescript",
      rows: [{ kind: "hunk_header", header: "@@ -1,1 +1,1 @@" }],
      offset: 0,
      limit: 1,
      nextOffset: 1,
      totalRows: 719,
      oldTotalLines: 0,
      newTotalLines: 719,
      isBinary: false,
    });
  });

  describe("rendering", () => {
    it("renders container testid", () => {
      render(
        withProviders(
          <AgentsPublishInlineDiffs
            conversationId="conv-1"
            review={makeReview()}
            commits={[]}
            isLoading={false}
          />,
        ),
      );
      expect(screen.getByTestId("agents-publish-inline-diffs")).toBeInTheDocument();
    });

    it("renders sticky bar", () => {
      render(
        withProviders(
          <AgentsPublishInlineDiffs
            conversationId="conv-1"
            review={makeReview()}
            commits={[]}
            isLoading={false}
          />,
        ),
      );
      expect(screen.getByTestId("inline-diffs-sticky-bar")).toBeInTheDocument();
    });

    it("renders diff filter in sticky bar", () => {
      render(
        withProviders(
          <AgentsPublishInlineDiffs
            conversationId="conv-1"
            review={makeReview()}
            commits={[]}
            isLoading={false}
          />,
        ),
      );
      expect(screen.getByTestId("mock-diff-filter")).toBeInTheDocument();
    });

    it("passes workspaceChangeCount from review.changes.length to filter", () => {
      const changes = [makeFileChange("src/Foo.tsx"), makeFileChange("src/Bar.tsx")];
      render(
        withProviders(
          <AgentsPublishInlineDiffs
            conversationId="conv-1"
            review={makeReview(changes)}
            commits={[]}
            isLoading={false}
          />,
        ),
      );
      expect(screen.getByTestId("mock-diff-filter")).toHaveAttribute("data-count", "2");
    });

    it("keeps workspace count, adjacent count, and rendered rows aligned with review changes when live status is clean", () => {
      const changes = [makeFileChange("src/Foo.tsx"), makeFileChange("src/Bar.tsx")];
      render(
        withProviders(
          <AgentsPublishInlineDiffs
            conversationId="conv-1"
            review={makeReview(changes)}
            commits={[]}
            isLoading={false}
            liveSummary={{
              supportsWorktreeModes: true,
              staged: { fileCount: 0, additions: 0, deletions: 0 },
              unstaged: { fileCount: 0, additions: 0, deletions: 0 },
              conflicted: { fileCount: 0, files: [] },
            }}
          />,
        ),
      );

      expect(screen.getByTestId("mock-diff-filter")).toHaveAttribute("data-count", "2");
      expect(screen.getByTestId("inline-diffs-file-count")).toHaveTextContent("2");
      expect(screen.getByTestId("inline-diffs-virtual-list")).toHaveAttribute(
        "data-count",
        "2",
      );
      expect(screen.getByTestId("mock-file-diff-src-Foo.tsx")).toBeInTheDocument();
      expect(screen.getByTestId("mock-file-diff-src-Bar.tsx")).toBeInTheDocument();
    });

    it("renders a file card for each change in review.changes (workspace changes mode)", () => {
      const changes = [makeFileChange("src/Foo.tsx"), makeFileChange("src/Bar.tsx")];
      render(
        withProviders(
          <AgentsPublishInlineDiffs
            conversationId="conv-1"
            review={makeReview(changes)}
            commits={[]}
            isLoading={false}
          />,
        ),
      );
      expect(screen.getByTestId("mock-file-diff-src-Foo.tsx")).toBeInTheDocument();
      expect(screen.getByTestId("mock-file-diff-src-Bar.tsx")).toBeInTheDocument();
    });

    it("renders backend-provided untracked files in workspace changes", () => {
      const changes = [
        makeFileChange("docs/untracked.md", {
          status: "added",
          additions: 2,
          deletions: 0,
        }),
      ];
      render(
        withProviders(
          <AgentsPublishInlineDiffs
            conversationId="conv-1"
            review={makeReview(changes)}
            commits={[]}
            isLoading={false}
          />,
        ),
      );

      expect(screen.getByTestId("mock-diff-filter")).toHaveAttribute("data-count", "1");
      expect(screen.getByTestId("mock-file-diff-docs-untracked.md")).toBeInTheDocument();
      expect(screen.queryByTestId("inline-diffs-empty")).toBeNull();
    });

    it("renders file cards through the virtualized list", () => {
      const changes = [makeFileChange("src/Foo.tsx"), makeFileChange("src/Bar.tsx")];
      render(
        withProviders(
          <AgentsPublishInlineDiffs
            conversationId="conv-1"
            review={makeReview(changes)}
            commits={[]}
            isLoading={false}
          />,
        ),
      );
      expect(screen.getByTestId("inline-diffs-virtual-list")).toHaveAttribute(
        "data-count",
        "2",
      );
    });

    it("constrains the virtualized file list to vertical scrolling", () => {
      const changes = [
        makeFileChange(
          "src/really/deep/path/that/should/not/create/a/horizontal/list/scroller/Foo.tsx",
        ),
      ];
      render(
        withProviders(
          <AgentsPublishInlineDiffs
            conversationId="conv-1"
            review={makeReview(changes)}
            commits={[]}
            isLoading={false}
          />,
        ),
      );

      const list = screen.getByTestId("inline-diffs-virtual-list");
      expect(screen.getByTestId("agents-publish-inline-diffs")).toHaveClass(
        "overflow-x-hidden",
      );
      expect(list).toHaveClass("overflow-x-hidden");
      expect(list).not.toHaveClass("pl-3");
      expect(list).not.toHaveClass("pr-5");
      expect(list).toHaveStyle({ overflowX: "hidden", scrollbarGutter: "stable" });
      expect(screen.getByTestId("inline-diffs-file-row-0")).toHaveClass(
        "box-border",
        "overflow-x-hidden",
        "px-3",
        "w-full",
      );
    });

    it("uses compact item padding instead of first/last variants inside virtual rows", () => {
      const changes = [makeFileChange("src/Foo.tsx"), makeFileChange("src/Bar.tsx")];
      render(
        withProviders(
          <AgentsPublishInlineDiffs
            conversationId="conv-1"
            review={makeReview(changes)}
            commits={[]}
            isLoading={false}
          />,
        ),
      );

      expect(screen.getByTestId("inline-diffs-file-row-0")).toHaveClass("pt-2", "pb-0.5");
      expect(screen.getByTestId("inline-diffs-file-row-1")).toHaveClass("pt-0.5", "pb-2");
      expect(screen.getByTestId("inline-diffs-file-row-0").className).not.toContain("first:");
      expect(screen.getByTestId("inline-diffs-file-row-0").className).not.toContain("last:");
    });

    it("only renders the active virtual range of file cards", () => {
      virtuosoMockState.range = { startIndex: 0, endIndex: 0 };
      const changes = [makeFileChange("src/Foo.tsx"), makeFileChange("src/Bar.tsx")];
      render(
        withProviders(
          <AgentsPublishInlineDiffs
            conversationId="conv-1"
            review={makeReview(changes)}
            commits={[]}
            isLoading={false}
          />,
        ),
      );
      expect(screen.getByTestId("mock-file-diff-src-Foo.tsx")).toBeInTheDocument();
      expect(screen.queryByTestId("mock-file-diff-src-Bar.tsx")).toBeNull();
    });

    it("shows file count in sticky bar", () => {
      const changes = [makeFileChange("src/Foo.tsx"), makeFileChange("src/Bar.tsx")];
      render(
        withProviders(
          <AgentsPublishInlineDiffs
            conversationId="conv-1"
            review={makeReview(changes)}
            commits={[]}
            isLoading={false}
          />,
        ),
      );
      expect(screen.getByTestId("inline-diffs-file-count")).toHaveTextContent("2");
    });

    it("shows total additions in sticky bar", () => {
      const changes = [
        makeFileChange("src/Foo.tsx", { additions: 10 }),
        makeFileChange("src/Bar.tsx", { additions: 5 }),
      ];
      render(
        withProviders(
          <AgentsPublishInlineDiffs
            conversationId="conv-1"
            review={makeReview(changes)}
            commits={[]}
            isLoading={false}
          />,
        ),
      );
      expect(screen.getByTestId("inline-diffs-additions")).toHaveTextContent("+15");
    });

    it("shows empty state when no changes", () => {
      render(
        withProviders(
          <AgentsPublishInlineDiffs
            conversationId="conv-1"
            review={makeReview([])}
            commits={[]}
            isLoading={false}
          />,
        ),
      );
      expect(screen.getByTestId("inline-diffs-empty")).toBeInTheDocument();
    });

    it("auto-selects unstaged files before staged and workspace changes", async () => {
      mockGetStagedFiles.mockResolvedValue([makeFileChange("src/StagedFile.tsx")]);
      mockGetUnstagedFiles.mockResolvedValue([makeFileChange("src/UnstagedFile.tsx")]);

      render(
        withProviders(
          <AgentsPublishInlineDiffs
            conversationId="conv-1"
            review={makeReview([makeFileChange("src/WorkspaceFile.tsx")])}
            commits={[]}
            isLoading={false}
          />,
        ),
      );

      await waitFor(() =>
        expect(screen.getByTestId("mock-diff-filter")).toHaveAttribute("data-mode", "unstaged"),
      );
      expect(screen.getByTestId("mock-diff-filter")).toHaveAttribute("data-unstaged-count", "1");
      expect(screen.getByTestId("mock-diff-filter")).toHaveAttribute("data-staged-count", "1");
      expect(screen.getByTestId("mock-file-diff-src-UnstagedFile.tsx")).toBeInTheDocument();
      expect(screen.queryByTestId("mock-file-diff-src-WorkspaceFile.tsx")).toBeNull();
    });

    it("auto-selects conflicted repair files before staged and unstaged changes", async () => {
      render(
        withProviders(
          <AgentsPublishInlineDiffs
            conversationId="conv-1"
            review={null}
            commits={[]}
            isLoading={false}
            repairMode
            liveSummary={{
              supportsWorktreeModes: true,
              staged: { fileCount: 1, additions: 8, deletions: 2 },
              unstaged: { fileCount: 1, additions: 3, deletions: 1 },
              conflicted: { fileCount: 1, files: ["src/Conflict.tsx"] },
            }}
          />,
        ),
      );

      await waitFor(() =>
        expect(screen.getByTestId("mock-diff-filter")).toHaveAttribute(
          "data-mode",
          "conflicted",
        ),
      );
      expect(screen.getByTestId("mock-diff-filter")).toHaveAttribute(
        "data-conflicted-count",
        "1",
      );
      expect(screen.getByTestId("mock-file-diff-src-Conflict.tsx")).toBeInTheDocument();
      expect(screen.queryByTestId("mock-file-diff-src-UnstagedFile.tsx")).toBeNull();

      fireVirtualRange(0, 0);
      await waitFor(() =>
        expect(mockGetRepairConflictFileDiff).toHaveBeenCalledWith(
          "conv-1",
          "src/Conflict.tsx",
        ),
      );
      expect(screen.getByTestId("mock-file-diff-src-Conflict.tsx")).toHaveAttribute(
        "data-conflict-diff-status",
        "loaded",
      );
      expect(mockGetUnstagedFileDiff).not.toHaveBeenCalled();
      expect(mockGetStagedFileDiff).not.toHaveBeenCalled();
      expect(mockGetUnstagedFiles).not.toHaveBeenCalled();
      expect(mockGetStagedFiles).not.toHaveBeenCalled();
    });

    it("uses paged staged loading for medium repair staged diffs", async () => {
      mockGetRepairStagedFiles.mockResolvedValue([
        makeFileChange("src/MediumRepair.tsx", { additions: 647, deletions: 72 }),
      ]);

      render(
        withProviders(
          <AgentsPublishInlineDiffs
            conversationId="conv-1"
            review={null}
            commits={[]}
            isLoading={false}
            repairMode
            liveSummary={{
              supportsWorktreeModes: true,
              staged: { fileCount: 1, additions: 647, deletions: 72 },
              unstaged: { fileCount: 0, additions: 0, deletions: 0 },
              conflicted: { fileCount: 0, files: [] },
            }}
          />,
        ),
      );

      await waitFor(() =>
        expect(screen.getByTestId("mock-diff-filter")).toHaveAttribute("data-mode", "staged"),
      );
      await waitFor(() =>
        expect(screen.getByTestId("mock-file-diff-src-MediumRepair.tsx")).toBeInTheDocument(),
      );
      fireVirtualRange(0, 0);

      const card = screen.getByTestId("mock-file-diff-src-MediumRepair.tsx");
      expect(card).toHaveAttribute("data-diff-page-ref-kind", "staged");
      expect(mockGetRepairStagedFileDiff).not.toHaveBeenCalled();
    });

    it("uses paged unstaged loading for medium repair unstaged diffs", async () => {
      mockGetRepairUnstagedFiles.mockResolvedValue([
        makeFileChange("src/MediumUnstagedRepair.tsx", { additions: 647, deletions: 72 }),
      ]);

      render(
        withProviders(
          <AgentsPublishInlineDiffs
            conversationId="conv-1"
            review={null}
            commits={[]}
            isLoading={false}
            repairMode
            liveSummary={{
              supportsWorktreeModes: true,
              staged: { fileCount: 0, additions: 0, deletions: 0 },
              unstaged: { fileCount: 1, additions: 647, deletions: 72 },
              conflicted: { fileCount: 0, files: [] },
            }}
          />,
        ),
      );

      await waitFor(() =>
        expect(screen.getByTestId("mock-diff-filter")).toHaveAttribute("data-mode", "unstaged"),
      );
      await waitFor(() =>
        expect(
          screen.getByTestId("mock-file-diff-src-MediumUnstagedRepair.tsx"),
        ).toBeInTheDocument(),
      );
      fireVirtualRange(0, 0);

      const card = screen.getByTestId("mock-file-diff-src-MediumUnstagedRepair.tsx");
      expect(card).toHaveAttribute("data-diff-page-ref-kind", "unstaged");
      expect(mockGetRepairUnstagedFileDiff).not.toHaveBeenCalled();
    });

    it("updates the paged staged reload key after the repair signature changes", async () => {
      const baseSummary = {
        supportsWorktreeModes: true,
        staged: { fileCount: 1, additions: 5, deletions: 2 },
        unstaged: { fileCount: 0, additions: 0, deletions: 0 },
        conflicted: { fileCount: 0, files: [] },
      };
      mockGetRepairStagedFiles.mockResolvedValue([makeFileChange("src/StagedFile.tsx")]);

      const { rerender } = render(
        withProviders(
          <AgentsPublishInlineDiffs
            conversationId="conv-1"
            review={null}
            commits={[]}
            isLoading={false}
            repairMode
            liveSummary={baseSummary}
          />,
        ),
      );

      await waitFor(() =>
        expect(screen.getByTestId("mock-diff-filter")).toHaveAttribute("data-mode", "staged"),
      );
      await waitFor(() =>
        expect(screen.getByTestId("mock-file-diff-src-StagedFile.tsx")).toBeInTheDocument(),
      );
      fireVirtualRange(0, 0);
      await waitFor(() =>
        expect(screen.getByTestId("mock-file-diff-src-StagedFile.tsx")).toHaveAttribute(
          "data-should-hydrate",
          "true",
        ),
      );
      const initialReloadKey = screen
        .getByTestId("mock-file-diff-src-StagedFile.tsx")
        .getAttribute("data-diff-page-reload-key");
      expect(initialReloadKey).toBeTruthy();
      expect(mockGetRepairStagedFileDiff).not.toHaveBeenCalled();

      rerender(
        withProviders(
          <AgentsPublishInlineDiffs
            conversationId="conv-1"
            review={null}
            commits={[]}
            isLoading={false}
            repairMode
            liveSummary={{
              supportsWorktreeModes: true,
              staged: { fileCount: 1, additions: 5, deletions: 2 },
              unstaged: { fileCount: 0, additions: 0, deletions: 0 },
              conflicted: { fileCount: 0, files: [] },
            }}
          />,
        ),
      );
      await new Promise((resolve) => setTimeout(resolve, 10));
      expect(
        screen
          .getByTestId("mock-file-diff-src-StagedFile.tsx")
          .getAttribute("data-diff-page-reload-key"),
      ).toBe(initialReloadKey);
      expect(mockGetRepairStagedFileDiff).not.toHaveBeenCalled();

      rerender(
        withProviders(
          <AgentsPublishInlineDiffs
            conversationId="conv-1"
            review={null}
            commits={[]}
            isLoading={false}
            repairMode
            liveSummary={{
              supportsWorktreeModes: true,
              staged: { fileCount: 1, additions: 6, deletions: 2 },
              unstaged: { fileCount: 0, additions: 0, deletions: 0 },
              conflicted: { fileCount: 0, files: [] },
            }}
          />,
        ),
      );
      await waitFor(() =>
        expect(
          screen
            .getByTestId("mock-file-diff-src-StagedFile.tsx")
            .getAttribute("data-diff-page-reload-key"),
        ).not.toBe(initialReloadKey),
      );
      expect(mockGetRepairStagedFileDiff).not.toHaveBeenCalled();
    });

    it("hydrates published workspace diffs after unstaged changes are published", async () => {
      mockGetStagedFiles.mockResolvedValue([]);
      mockGetUnstagedFiles.mockResolvedValue([makeFileChange("src/UnstagedFile.tsx")]);
      const queryClient = makeQueryClient();

      const { rerender } = render(
        withProviders(
          <AgentsPublishInlineDiffs
            conversationId="conv-1"
            review={makeReview([makeFileChange("src/WorkspaceFile.tsx")])}
            commits={[]}
            isLoading={false}
          />,
          queryClient,
        ),
      );

      await waitFor(() =>
        expect(screen.getByTestId("mock-diff-filter")).toHaveAttribute(
          "data-mode",
          "unstaged",
        ),
      );
      fireVirtualRange(0, 0);
      await waitFor(() =>
        expect(screen.getByTestId("mock-file-diff-src-UnstagedFile.tsx")).toHaveAttribute(
          "data-should-hydrate",
          "true",
        ),
      );

      act(() => {
        queryClient.setQueryData(
          [...agentWorkspaceKeys.diff("conv-1"), "unstaged-files"],
          [],
        );
        queryClient.setQueryData(
          [...agentWorkspaceKeys.diff("conv-1"), "staged-files"],
          [],
        );
      });
      rerender(
        withProviders(
          <AgentsPublishInlineDiffs
            conversationId="conv-1"
            review={makeReview([makeFileChange("src/Published.tsx")])}
            commits={[]}
            isLoading={false}
            workspaceChangeLabel="Published changes"
          />,
          queryClient,
        ),
      );

      await waitFor(() =>
        expect(screen.getByTestId("mock-diff-filter")).toHaveAttribute(
          "data-mode",
          "uncommitted",
        ),
      );
      expect(screen.getByTestId("mock-diff-filter")).toHaveAttribute(
        "data-label",
        "Published changes",
      );
      await waitFor(() =>
        expect(screen.getByTestId("mock-file-diff-src-Published.tsx")).toHaveAttribute(
          "data-should-hydrate",
          "true",
        ),
      );
      expect(screen.getByTestId("mock-file-diff-src-Published.tsx")).toHaveAttribute(
        "data-diff-page-ref-kind",
        "head",
      );
      expect(mockGetUncommittedDiff).not.toHaveBeenCalledWith(
        "conv-1",
        "src/Published.tsx",
      );
    });

    it("auto-selects staged files when there are no unstaged files", async () => {
      mockGetStagedFiles.mockResolvedValue([makeFileChange("src/StagedFile.tsx")]);
      mockGetUnstagedFiles.mockResolvedValue([]);

      render(
        withProviders(
          <AgentsPublishInlineDiffs
            conversationId="conv-1"
            review={makeReview([makeFileChange("src/WorkspaceFile.tsx")])}
            commits={[]}
            isLoading={false}
          />,
        ),
      );

      await waitFor(() =>
        expect(screen.getByTestId("mock-diff-filter")).toHaveAttribute("data-mode", "staged"),
      );
      expect(screen.getByTestId("mock-file-diff-src-StagedFile.tsx")).toBeInTheDocument();
      expect(screen.queryByTestId("mock-file-diff-src-WorkspaceFile.tsx")).toBeNull();
    });

    it("uses mode-specific empty copy for unstaged and staged views", async () => {
      const user = userEvent.setup();
      render(
        withProviders(
          <AgentsPublishInlineDiffs
            conversationId="conv-1"
            review={makeReview([makeFileChange("src/WorkspaceFile.tsx")])}
            commits={[]}
            isLoading={false}
          />,
        ),
      );

      await user.click(screen.getByRole("button", { name: "Unstaged" }));
      await waitFor(() => expect(screen.getByText("No unstaged files")).toBeInTheDocument());
      expect(
        screen.getByText("No unstaged changes detected in this workspace."),
      ).toBeInTheDocument();

      await user.click(screen.getByRole("button", { name: "Staged" }));
      await waitFor(() => expect(screen.getByText("No staged files")).toBeInTheDocument());
      expect(screen.getByText("No staged changes detected in this workspace.")).toBeInTheDocument();
    });

    it("all file cards default to expanded", () => {
      const changes = [makeFileChange("src/Foo.tsx")];
      render(
        withProviders(
          <AgentsPublishInlineDiffs
            conversationId="conv-1"
            review={makeReview(changes)}
            commits={[]}
            isLoading={false}
          />,
        ),
      );
      expect(screen.getByTestId("mock-file-diff-src-Foo.tsx")).toHaveAttribute(
        "data-expanded",
        "true",
      );
    });
  });

  describe("collapse/expand all", () => {
    it("renders collapse-all button", () => {
      const changes = [makeFileChange("src/Foo.tsx")];
      render(
        withProviders(
          <AgentsPublishInlineDiffs
            conversationId="conv-1"
            review={makeReview(changes)}
            commits={[]}
            isLoading={false}
          />,
        ),
      );
      expect(screen.getByTestId("inline-diffs-collapse-all")).toBeInTheDocument();
    });

    it("collapses all when collapse-all clicked", async () => {
      const user = userEvent.setup();
      const changes = [makeFileChange("src/Foo.tsx"), makeFileChange("src/Bar.tsx")];
      render(
        withProviders(
          <AgentsPublishInlineDiffs
            conversationId="conv-1"
            review={makeReview(changes)}
            commits={[]}
            isLoading={false}
          />,
        ),
      );
      await user.click(screen.getByTestId("inline-diffs-collapse-all"));
      expect(screen.getByTestId("mock-file-diff-src-Foo.tsx")).toHaveAttribute(
        "data-expanded",
        "false",
      );
    });

    it("expands all after collapse", async () => {
      const user = userEvent.setup();
      const changes = [makeFileChange("src/Foo.tsx")];
      render(
        withProviders(
          <AgentsPublishInlineDiffs
            conversationId="conv-1"
            review={makeReview(changes)}
            commits={[]}
            isLoading={false}
          />,
        ),
      );
      await user.click(screen.getByTestId("inline-diffs-collapse-all"));
      await user.click(screen.getByTestId("inline-diffs-expand-all"));
      expect(screen.getByTestId("mock-file-diff-src-Foo.tsx")).toHaveAttribute(
        "data-expanded",
        "true",
      );
    });

    it("keeps collapse-all active for newly loaded mode files", async () => {
      const user = userEvent.setup();
      const changes = [makeFileChange("src/Foo.tsx")];
      mockGetCommitFiles.mockResolvedValue([makeFileChange("src/CommitOnly.tsx")]);
      render(
        withProviders(
          <AgentsPublishInlineDiffs
            conversationId="conv-1"
            review={makeReview(changes)}
            commits={[makeCommit("sha-abc-full")]}
            isLoading={false}
          />,
        ),
      );

      await user.click(screen.getByTestId("inline-diffs-collapse-all"));
      await user.click(screen.getByRole("button", { name: "Commit sha-abc" }));

      await waitFor(() =>
        expect(screen.getByTestId("mock-file-diff-src-CommitOnly.tsx")).toHaveAttribute(
          "data-expanded",
          "false",
        ),
      );
    });

    it("keeps expand-all active for newly loaded mode files", async () => {
      const user = userEvent.setup();
      const changes = [makeFileChange("src/Foo.tsx")];
      mockGetCommitFiles.mockResolvedValue([makeFileChange("src/CommitOnly.tsx")]);
      render(
        withProviders(
          <AgentsPublishInlineDiffs
            conversationId="conv-1"
            review={makeReview(changes)}
            commits={[makeCommit("sha-abc-full")]}
            isLoading={false}
          />,
        ),
      );

      await user.click(screen.getByTestId("inline-diffs-collapse-all"));
      await user.click(screen.getByTestId("inline-diffs-expand-all"));
      await user.click(screen.getByRole("button", { name: "Commit sha-abc" }));

      await waitFor(() =>
        expect(screen.getByTestId("mock-file-diff-src-CommitOnly.tsx")).toHaveAttribute(
          "data-expanded",
          "true",
        ),
      );
    });

    it("manual file toggles switch bulk state to custom for newly loaded files", async () => {
      const user = userEvent.setup();
      const changes = [makeFileChange("src/Foo.tsx")];
      mockGetCommitFiles.mockResolvedValue([makeFileChange("src/CommitOnly.tsx")]);
      render(
        withProviders(
          <AgentsPublishInlineDiffs
            conversationId="conv-1"
            review={makeReview(changes)}
            commits={[makeCommit("sha-abc-full")]}
            isLoading={false}
          />,
        ),
      );

      await user.click(screen.getByTestId("inline-diffs-collapse-all"));
      await user.click(screen.getByTestId("mock-file-toggle-src-Foo.tsx"));
      expect(screen.getByTestId("mock-file-diff-src-Foo.tsx")).toHaveAttribute(
        "data-expanded",
        "true",
      );

      await user.click(screen.getByRole("button", { name: "Commit sha-abc" }));

      await waitFor(() =>
        expect(screen.getByTestId("mock-file-diff-src-CommitOnly.tsx")).toHaveAttribute(
          "data-expanded",
          "true",
        ),
      );
    });
  });

  describe("mode=uncommitted — diff fetching", () => {
    it("does not fetch workspace diff before the virtual range hydrates a file", async () => {
      const changes = [makeFileChange("src/Foo.tsx")];
      render(
        withProviders(
          <AgentsPublishInlineDiffs
            conversationId="conv-42"
            review={makeReview(changes)}
            commits={[]}
            isLoading={false}
          />,
        ),
      );
      await new Promise((r) => setTimeout(r, 10));
      expect(mockGetUncommittedDiff).not.toHaveBeenCalled();
    });

    it("skips workspace full-diff fetching for a page-capable hydrated file", async () => {
      const changes = [makeFileChange("src/Foo.tsx", { additions: 647, deletions: 72 })];
      render(
        withProviders(
          <AgentsPublishInlineDiffs
            conversationId="conv-42"
            review={makeReview(changes)}
            commits={[]}
            isLoading={false}
          />,
        ),
      );
      fireVirtualRange(0, 0);
      await waitFor(() =>
        expect(screen.getByTestId("mock-file-diff-src-Foo.tsx")).toHaveAttribute(
          "data-should-hydrate",
          "true",
        ),
      );
      expect(screen.getByTestId("mock-file-diff-src-Foo.tsx")).toHaveAttribute(
        "data-diff-page-ref-kind",
        "head",
      );
      await new Promise((r) => setTimeout(r, 10));
      expect(mockGetUncommittedDiff).not.toHaveBeenCalledWith("conv-42", "src/Foo.tsx");
    });

    it("fetches workspace diff for hydrated expanded files when page refs are omitted", async () => {
      const changes = [makeFileChange("src/Fallback.tsx")];
      render(
        withProviders(
          <AgentsPublishInlineDiffs
            conversationId="conv-42"
            review={makeReview(changes)}
            commits={[]}
            isLoading={false}
            repairMode
          />,
        ),
      );
      fireVirtualRange(0, 0);
      await waitFor(() =>
        expect(mockGetUncommittedDiff).toHaveBeenCalledWith("conv-42", "src/Fallback.tsx"),
      );
      expect(screen.getByTestId("mock-file-diff-src-Fallback.tsx")).not.toHaveAttribute(
        "data-diff-page-ref-kind",
      );
    });

    it("does not fetch off-range expanded files", async () => {
      virtuosoMockState.range = { startIndex: 0, endIndex: 0 };
      const changes = [
        makeFileChange("src/Foo.tsx"),
        makeFileChange("src/Bar.tsx"),
      ];
      render(
        withProviders(
          <AgentsPublishInlineDiffs
            conversationId="conv-42"
            review={makeReview(changes)}
            commits={[]}
            isLoading={false}
            repairMode
          />,
        ),
      );
      await waitFor(() =>
        expect(mockGetUncommittedDiff).toHaveBeenCalledWith("conv-42", "src/Foo.tsx"),
      );
      expect(mockGetUncommittedDiff).not.toHaveBeenCalledWith("conv-42", "src/Bar.tsx");
    });

    it("passes fallback diff data to file card after fetch", async () => {
      const changes = [makeFileChange("src/Foo.tsx")];
      render(
        withProviders(
          <AgentsPublishInlineDiffs
            conversationId="conv-1"
            review={makeReview(changes)}
            commits={[]}
            isLoading={false}
            repairMode
          />,
        ),
      );
      fireVirtualRange(0, 0);
      await waitFor(() =>
        expect(screen.getByTestId("mock-file-diff-src-Foo.tsx")).toHaveAttribute(
          "data-diff-status",
          "loaded",
        ),
      );
    });

    it("passes matching GitHub annotations to head-mode file cards", () => {
      const changes = [makeFileChange("src/Foo.tsx"), makeFileChange("src/Bar.tsx")];
      render(
        withProviders(
          <AgentsPublishInlineDiffs
            conversationId="conv-1"
            review={makeReview(changes)}
            commits={[]}
            isLoading={false}
            annotations={[
              makeAnnotation("src/Foo.tsx"),
              makeAnnotation(null),
              makeAnnotation("src/Other.tsx"),
            ]}
          />,
        ),
      );

      expect(screen.getByTestId("mock-file-diff-src-Foo.tsx")).toHaveAttribute(
        "data-annotation-count",
        "1",
      );
      expect(screen.getByTestId("mock-file-diff-src-Bar.tsx")).toHaveAttribute(
        "data-annotation-count",
        "0",
      );
    });

    it("passes matching workspace review hunk annotations to head-mode file cards", () => {
      const changes = [makeFileChange("src/Foo.tsx"), makeFileChange("src/Bar.tsx")];
      render(
        withProviders(
          <AgentsPublishInlineDiffs
            conversationId="conv-1"
            review={makeReview(changes)}
            commits={[]}
            isLoading={false}
            hunkAnnotations={[
              makeHunkAnnotation("src/Foo.tsx"),
              makeHunkAnnotation("src/Other.tsx"),
            ]}
          />,
        ),
      );

      expect(screen.getByTestId("mock-file-diff-src-Foo.tsx")).toHaveAttribute(
        "data-hunk-annotation-count",
        "1",
      );
      expect(screen.getByTestId("mock-file-diff-src-Bar.tsx")).toHaveAttribute(
        "data-hunk-annotation-count",
        "0",
      );
    });

    it("keeps full diffs visible until the review walkthrough is entered and shows the legend", async () => {
      const user = userEvent.setup();
      const changes = [makeFileChange("src/Foo.tsx"), makeFileChange("src/Bar.tsx")];
      render(
        withProviders(
          <AgentsPublishInlineDiffs
            conversationId="conv-1"
            review={makeReview(changes)}
            commits={[]}
            isLoading={false}
            hunkAnnotations={[
              makeHunkAnnotation("src/Foo.tsx"),
              makeHunkAnnotation("src/Bar.tsx", {
                id: "workspace-review-hunk-bar",
              }),
            ]}
          />,
        ),
      );

      expect(screen.getByTestId("publish-review-walkthrough-enter")).toHaveTextContent(
        "▶ Walkthrough 2",
      );
      expect(screen.getByTestId("mock-file-diff-src-Foo.tsx")).toBeInTheDocument();
      expect(screen.getByTestId("mock-file-diff-src-Bar.tsx")).toBeInTheDocument();

      await user.click(screen.getByTestId("inline-diffs-review-legend"));
      expect(screen.getByText("Review annotation colors")).toBeInTheDocument();
      expect(screen.getByText("failure, error, critical, high")).toBeInTheDocument();
    });

    it("enters the walkthrough deliberately and exits back to the full diff list", async () => {
      const user = userEvent.setup();
      const changes = [makeFileChange("src/Foo.tsx")];
      render(
        withProviders(
          <AgentsPublishInlineDiffs
            conversationId="conv-1"
            review={makeReview(changes)}
            commits={[]}
            isLoading={false}
            annotations={[makeAnnotation("src/Foo.tsx")]}
            repairMode
          />,
        ),
      );

      await user.click(screen.getByTestId("publish-review-walkthrough-enter"));
      expect(screen.getByTestId("publish-review-walkthrough")).toBeInTheDocument();
      await user.click(screen.getByTestId("publish-review-walkthrough-exit"));
      expect(screen.getByTestId("mock-file-diff-src-Foo.tsx")).toBeInTheDocument();
    });

    it("never strands the user in the walkthrough when its findings stop applying", async () => {
      const user = userEvent.setup();
      const changes = [makeFileChange("src/Foo.tsx")];
      const { rerender } = render(
        withProviders(
          <AgentsPublishInlineDiffs
            conversationId="conv-1"
            review={makeReview(changes)}
            commits={[]}
            isLoading={false}
            hunkAnnotations={[makeHunkAnnotation("src/Foo.tsx")]}
          />,
        ),
      );

      await user.click(screen.getByTestId("publish-review-walkthrough-enter"));
      expect(screen.getByTestId("publish-review-walkthrough")).toBeInTheDocument();

      // An annotation refresh empties the finding set out from under the open
      // walkthrough; it must return to the changes list rather than strand the
      // user on a findings-less surface.
      rerender(
        withProviders(
          <AgentsPublishInlineDiffs
            conversationId="conv-1"
            review={makeReview(changes)}
            commits={[]}
            isLoading={false}
            hunkAnnotations={[]}
          />,
        ),
      );

      await waitFor(() =>
        expect(
          screen.queryByTestId("publish-review-walkthrough"),
        ).not.toBeInTheDocument(),
      );
      expect(screen.getByTestId("mock-file-diff-src-Foo.tsx")).toBeInTheDocument();
    });

    it("fetches only the current finding's file while walking through findings", async () => {
      const user = userEvent.setup();
      const changes = [
        makeFileChange("src/Foo.tsx"),
        makeFileChange("src/Bar.tsx"),
        makeFileChange("src/Unannotated.tsx"),
      ];
      render(
        withProviders(
          <AgentsPublishInlineDiffs
            conversationId="conv-1"
            review={makeReview(changes)}
            commits={[]}
            isLoading={false}
            hunkAnnotations={[
              makeHunkAnnotation("src/Foo.tsx"),
              makeHunkAnnotation("src/Bar.tsx", { id: "workspace-review-hunk-bar" }),
            ]}
          />,
        ),
      );

      // Nothing is visible/hydrated, so the normal list fetches nothing.
      await new Promise((resolve) => setTimeout(resolve, 10));
      expect(mockGetUncommittedDiff).not.toHaveBeenCalled();

      await user.click(screen.getByTestId("publish-review-walkthrough-enter"));

      // Entering hydrates ONLY the first finding's file.
      await waitFor(() =>
        expect(mockGetUncommittedDiff).toHaveBeenCalledWith("conv-1", "src/Foo.tsx"),
      );
      expect(mockGetUncommittedDiff).not.toHaveBeenCalledWith("conv-1", "src/Bar.tsx");
      expect(mockGetUncommittedDiff).not.toHaveBeenCalledWith(
        "conv-1",
        "src/Unannotated.tsx",
      );

      // Stepping to the next finding hydrates its file and nothing else.
      await user.click(screen.getByTestId("publish-review-walkthrough-next"));
      await waitFor(() =>
        expect(mockGetUncommittedDiff).toHaveBeenCalledWith("conv-1", "src/Bar.tsx"),
      );
      expect(mockGetUncommittedDiff).not.toHaveBeenCalledWith(
        "conv-1",
        "src/Unannotated.tsx",
      );
    });

    it("does not auto-fetch a generated file's diff until the walkthrough asks for it", async () => {
      const user = userEvent.setup();
      const changes = [makeFileChange("pnpm-lock.yaml", { isGenerated: true })];
      render(
        withProviders(
          <AgentsPublishInlineDiffs
            conversationId="conv-1"
            review={makeReview(changes)}
            commits={[]}
            isLoading={false}
            hunkAnnotations={[makeHunkAnnotation("pnpm-lock.yaml")]}
          />,
        ),
      );

      await user.click(screen.getByTestId("publish-review-walkthrough-enter"));

      // The generated-file gate still applies inside the walkthrough.
      await new Promise((resolve) => setTimeout(resolve, 10));
      expect(mockGetUncommittedDiff).not.toHaveBeenCalled();
      expect(
        screen.getByTestId("publish-review-walkthrough-hunk-blocked"),
      ).toHaveTextContent("generated file");

      await user.click(screen.getByTestId("publish-review-walkthrough-hunk-load"));
      await waitFor(() =>
        expect(mockGetUncommittedDiff).toHaveBeenCalledWith("conv-1", "pnpm-lock.yaml"),
      );
    });

    it("passes staged and unstaged workspace review hunk annotations to default workspace file cards", () => {
      const changes = [makeFileChange("src/Foo.tsx"), makeFileChange("src/Bar.tsx")];
      render(
        withProviders(
          <AgentsPublishInlineDiffs
            conversationId="conv-1"
            review={makeReview(changes)}
            commits={[]}
            isLoading={false}
            hunkAnnotations={[
              makeHunkAnnotation("src/Foo.tsx", {
                id: "workspace-review-hunk-staged",
                diffSource: "staged",
              }),
              makeHunkAnnotation("src/Foo.tsx", {
                id: "workspace-review-hunk-unstaged",
                diffSource: "unstaged",
              }),
              makeHunkAnnotation("src/Other.tsx", {
                id: "workspace-review-hunk-other",
                diffSource: "staged",
              }),
            ]}
          />,
        ),
      );

      expect(screen.getByTestId("mock-file-diff-src-Foo.tsx")).toHaveAttribute(
        "data-hunk-annotation-count",
        "2",
      );
      expect(screen.getByTestId("mock-file-diff-src-Bar.tsx")).toHaveAttribute(
        "data-hunk-annotation-count",
        "0",
      );
    });

    it("auto-scrolls to the first synced GitHub annotation after annotations arrive", async () => {
      const changes = [
        makeFileChange("src/Foo.tsx"),
        makeFileChange("src/Bar.tsx"),
        makeFileChange("src/Baz.tsx"),
      ];
      const client = makeQueryClient();
      const { rerender } = render(
        withProviders(
          <AgentsPublishInlineDiffs
            conversationId="conv-1"
            review={makeReview(changes)}
            commits={[]}
            isLoading={false}
            annotations={[]}
          />,
          client,
        ),
      );

      expect(virtuosoMockState.scrollToIndex).not.toHaveBeenCalled();

      rerender(
        withProviders(
          <AgentsPublishInlineDiffs
            conversationId="conv-1"
            review={makeReview(changes)}
            commits={[]}
            isLoading={false}
            annotations={[
              makeAnnotation("src/Baz.tsx", { id: "annotation-baz" }),
              makeAnnotation("src/Bar.tsx", { id: "annotation-bar" }),
            ]}
          />,
          client,
        ),
      );

      await waitFor(() =>
        expect(virtuosoMockState.scrollToIndex).toHaveBeenCalledWith({
          align: "start",
          behavior: "auto",
          index: 1,
        }),
      );
      await waitFor(() =>
        expect(screen.getByTestId("mock-file-diff-src-Bar.tsx")).toHaveAttribute(
          "data-focus-target",
          "true",
        ),
      );
      await waitFor(() =>
        expect(annotationScrollIntoViewMock).toHaveBeenCalledWith({
          block: "center",
          behavior: "auto",
          inline: "nearest",
        }),
      );

      virtuosoMockState.scrollToIndex.mockClear();
      annotationScrollIntoViewMock.mockClear();

      rerender(
        withProviders(
          <AgentsPublishInlineDiffs
            conversationId="conv-1"
            review={makeReview(changes)}
            commits={[]}
            isLoading={false}
            annotations={[
              makeAnnotation("src/Baz.tsx", { id: "annotation-baz" }),
              makeAnnotation("src/Bar.tsx", { id: "annotation-bar" }),
            ]}
          />,
          client,
        ),
      );

      await new Promise((resolve) => setTimeout(resolve, 10));
      expect(virtuosoMockState.scrollToIndex).not.toHaveBeenCalled();
      expect(annotationScrollIntoViewMock).not.toHaveBeenCalled();
    });

    it("auto-scrolls to the first synced workspace review hunk annotation", async () => {
      const changes = [
        makeFileChange("src/Foo.tsx"),
        makeFileChange("src/Bar.tsx"),
        makeFileChange("src/Baz.tsx"),
      ];
      const client = makeQueryClient();
      const { rerender } = render(
        withProviders(
          <AgentsPublishInlineDiffs
            conversationId="conv-1"
            review={makeReview(changes)}
            commits={[]}
            isLoading={false}
            hunkAnnotations={[]}
          />,
          client,
        ),
      );

      expect(virtuosoMockState.scrollToIndex).not.toHaveBeenCalled();

      rerender(
        withProviders(
          <AgentsPublishInlineDiffs
            conversationId="conv-1"
            review={makeReview(changes)}
            commits={[]}
            isLoading={false}
            hunkAnnotations={[
              makeHunkAnnotation("src/Baz.tsx", {
                id: "workspace-review-hunk-baz",
              }),
              makeHunkAnnotation("src/Bar.tsx", {
                id: "workspace-review-hunk-bar",
              }),
            ]}
          />,
          client,
        ),
      );

      await waitFor(() =>
        expect(virtuosoMockState.scrollToIndex).toHaveBeenCalledWith({
          align: "start",
          behavior: "auto",
          index: 1,
        }),
      );
      await waitFor(() =>
        expect(screen.getByTestId("mock-file-diff-src-Bar.tsx")).toHaveAttribute(
          "data-focus-target",
          "true",
        ),
      );
      await waitFor(() =>
        expect(annotationScrollIntoViewMock).toHaveBeenCalledWith({
          block: "center",
          behavior: "auto",
          inline: "nearest",
        }),
      );
    });

    it("retries hunk annotation auto-scroll after a paged diff hydrates the row", async () => {
      const changes = [
        makeFileChange("src/Foo.tsx"),
        makeFileChange("src/Paged.tsx", { additions: 1_250, deletions: 25 }),
        makeFileChange("src/Baz.tsx"),
      ];
      const client = makeQueryClient();
      const { rerender } = render(
        withProviders(
          <AgentsPublishInlineDiffs
            conversationId="conv-1"
            review={makeReview(changes)}
            commits={[]}
            isLoading={false}
            hunkAnnotations={[]}
          />,
          client,
        ),
      );

      expect(virtuosoMockState.scrollToIndex).not.toHaveBeenCalled();

      rerender(
        withProviders(
          <AgentsPublishInlineDiffs
            conversationId="conv-1"
            review={makeReview(changes)}
            commits={[]}
            isLoading={false}
            hunkAnnotations={[
              makeHunkAnnotation("src/Paged.tsx", {
                id: "workspace-review-hunk-paged",
              }),
            ]}
          />,
          client,
        ),
      );

      await waitFor(() =>
        expect(virtuosoMockState.scrollToIndex).toHaveBeenCalledWith({
          align: "start",
          behavior: "auto",
          index: 1,
        }),
      );
      await waitFor(() =>
        expect(screen.getByTestId("diff-hunk-annotation-row")).toBeInTheDocument(),
      );
      await waitFor(() =>
        expect(annotationScrollIntoViewMock).toHaveBeenCalledWith({
          block: "center",
          behavior: "auto",
          inline: "nearest",
        }),
      );
    });

    it("auto-scrolls to the first synced staged or unstaged workspace review hunk annotation", async () => {
      const changes = [
        makeFileChange("src/Foo.tsx"),
        makeFileChange("src/Bar.tsx"),
        makeFileChange("src/Baz.tsx"),
      ];
      const client = makeQueryClient();
      const { rerender } = render(
        withProviders(
          <AgentsPublishInlineDiffs
            conversationId="conv-1"
            review={makeReview(changes)}
            commits={[]}
            isLoading={false}
            hunkAnnotations={[]}
          />,
          client,
        ),
      );

      expect(virtuosoMockState.scrollToIndex).not.toHaveBeenCalled();

      rerender(
        withProviders(
          <AgentsPublishInlineDiffs
            conversationId="conv-1"
            review={makeReview(changes)}
            commits={[]}
            isLoading={false}
            hunkAnnotations={[
              makeHunkAnnotation("src/Baz.tsx", {
                id: "workspace-review-hunk-baz-unstaged",
                diffSource: "unstaged",
              }),
              makeHunkAnnotation("src/Bar.tsx", {
                id: "workspace-review-hunk-bar-staged",
                diffSource: "staged",
              }),
            ]}
          />,
          client,
        ),
      );

      await waitFor(() =>
        expect(virtuosoMockState.scrollToIndex).toHaveBeenCalledWith({
          align: "start",
          behavior: "auto",
          index: 1,
        }),
      );
      await waitFor(() =>
        expect(screen.getByTestId("mock-file-diff-src-Bar.tsx")).toHaveAttribute(
          "data-focus-target",
          "true",
        ),
      );
      await waitFor(() =>
        expect(annotationScrollIntoViewMock).toHaveBeenCalledWith({
          block: "center",
          behavior: "auto",
          inline: "nearest",
        }),
      );
    });
  });

  describe("mode=commit — diff fetching", () => {
    it("skips commit full-diff fetching when page refs are available", async () => {
      const user = userEvent.setup();
      const changes = [makeFileChange("src/Foo.tsx")];
      const commits = [makeCommit("sha-abc-full")];
      render(
        withProviders(
          <AgentsPublishInlineDiffs
            conversationId="conv-1"
            review={makeReview(changes)}
            commits={commits}
            isLoading={false}
          />,
        ),
      );
      // Use mock filter's "Commit sha-abc" button (which calls onModeChange("sha-abc"))
      await user.click(screen.getByRole("button", { name: "Commit sha-abc" }));
      await waitFor(() =>
        expect(mockGetCommitFiles).toHaveBeenCalledWith("conv-1", "sha-abc"),
      );
      await waitFor(() =>
        expect(screen.getByTestId("mock-file-diff-src-CommitOnly.tsx")).toBeInTheDocument(),
      );
      fireVirtualRange(0, 0);
      await waitFor(() =>
        expect(screen.getByTestId("mock-file-diff-src-CommitOnly.tsx")).toHaveAttribute(
          "data-should-hydrate",
          "true",
        ),
      );
      expect(screen.getByTestId("mock-file-diff-src-CommitOnly.tsx")).toHaveAttribute(
        "data-diff-page-ref-kind",
        "commit",
      );
      await new Promise((resolve) => setTimeout(resolve, 10));
      expect(mockGetCommitDiff).not.toHaveBeenCalledWith(
        "conv-1",
        "sha-abc",
        "src/CommitOnly.tsx",
      );
    });

    it("renders commit file list (not uncommitted list) when mode switches to commit SHA", async () => {
      const user = userEvent.setup();
      const changes = [makeFileChange("src/Foo.tsx")]; // uncommitted file
      const commits = [makeCommit("sha-abc-full")];
      // beforeEach sets mockGetCommitFiles → [makeFileChange("src/CommitOnly.tsx")]
      render(
        withProviders(
          <AgentsPublishInlineDiffs
            conversationId="conv-1"
            review={makeReview(changes)}
            commits={commits}
            isLoading={false}
          />,
        ),
      );
      await user.click(screen.getByRole("button", { name: "Commit sha-abc" }));
      // Commit file appears
      await waitFor(() =>
        expect(screen.getByTestId("mock-file-diff-src-CommitOnly.tsx")).toBeInTheDocument(),
      );
      // Workspace-change file no longer shown
      expect(screen.queryByTestId("mock-file-diff-src-Foo.tsx")).toBeNull();
    });

    it("passes workspace review hunk annotations to specific commit file cards", async () => {
      const user = userEvent.setup();
      mockGetCommitFiles.mockResolvedValue([makeFileChange("src/CommitOnly.tsx")]);
      const changes = [makeFileChange("src/Foo.tsx")];
      const commits = [makeCommit("sha-abc-full")];
      render(
        withProviders(
          <AgentsPublishInlineDiffs
            conversationId="conv-1"
            review={makeReview(changes)}
            commits={commits}
            isLoading={false}
            hunkAnnotations={[
              makeHunkAnnotation("src/CommitOnly.tsx", {
                id: "workspace-review-hunk-commit",
                diffSource: "committed",
              }),
            ]}
          />,
        ),
      );

      await user.click(screen.getByRole("button", { name: "Commit sha-abc" }));

      await waitFor(() =>
        expect(screen.getByTestId("mock-file-diff-src-CommitOnly.tsx")).toHaveAttribute(
          "data-hunk-annotation-count",
          "1",
        ),
      );
    });

    it("shows file count from commit file list in sticky bar", async () => {
      const user = userEvent.setup();
      mockGetCommitFiles.mockResolvedValue([
        makeFileChange("src/CommitOnly.tsx"),
        makeFileChange("src/AnotherCommit.tsx"),
      ]);
      const changes = [makeFileChange("src/Foo.tsx")]; // 1 uncommitted file
      const commits = [makeCommit("sha-abc-full")];
      render(
        withProviders(
          <AgentsPublishInlineDiffs
            conversationId="conv-1"
            review={makeReview(changes)}
            commits={commits}
            isLoading={false}
          />,
        ),
      );
      await user.click(screen.getByRole("button", { name: "Commit sha-abc" }));
      // File count reflects commit list (2), not workspace-change list (1)
      await waitFor(() =>
        expect(screen.getByTestId("inline-diffs-file-count")).toHaveTextContent("2"),
      );
    });

    it("shows +/− totals from commit file list in sticky bar", async () => {
      const user = userEvent.setup();
      mockGetCommitFiles.mockResolvedValue([
        makeFileChange("src/CommitOnly.tsx", { additions: 7, deletions: 3 }),
      ]);
      const changes = [makeFileChange("src/Foo.tsx", { additions: 99, deletions: 88 })];
      const commits = [makeCommit("sha-abc-full")];
      render(
        withProviders(
          <AgentsPublishInlineDiffs
            conversationId="conv-1"
            review={makeReview(changes)}
            commits={commits}
            isLoading={false}
          />,
        ),
      );
      await user.click(screen.getByRole("button", { name: "Commit sha-abc" }));
      // Totals reflect commit file (7/3), not workspace-change file (99/88)
      await waitFor(() =>
        expect(screen.getByTestId("inline-diffs-additions")).toHaveTextContent("+7"),
      );
      expect(screen.getByTestId("inline-diffs-deletions")).toHaveTextContent("−3");
    });
  });

  describe("mode=staged — diff fetching", () => {
    beforeEach(() => {
      mockGetStagedFiles.mockResolvedValue([makeFileChange("src/StagedFile.tsx")]);
    });

    it("fetches staged file changes when mode switches to staged", async () => {
      const user = userEvent.setup();
      const changes = [makeFileChange("src/Foo.tsx")];
      render(
        withProviders(
          <AgentsPublishInlineDiffs
            conversationId="conv-1"
            review={makeReview(changes)}
            commits={[]}
            isLoading={false}
          />,
        ),
      );
      await user.click(screen.getByRole("button", { name: "Staged" }));
      await waitFor(() => expect(mockGetStagedFiles).toHaveBeenCalledWith("conv-1"));
    });

    it("renders staged file list (not workspace-change list) when mode is staged", async () => {
      const user = userEvent.setup();
      const changes = [makeFileChange("src/Foo.tsx")];
      render(
        withProviders(
          <AgentsPublishInlineDiffs
            conversationId="conv-1"
            review={makeReview(changes)}
            commits={[]}
            isLoading={false}
          />,
        ),
      );
      await user.click(screen.getByRole("button", { name: "Staged" }));
      await waitFor(() =>
        expect(screen.getByTestId("mock-file-diff-src-StagedFile.tsx")).toBeInTheDocument(),
      );
      expect(screen.queryByTestId("mock-file-diff-src-Foo.tsx")).toBeNull();
    });

    it("shows +/− totals from staged file list in sticky bar", async () => {
      const user = userEvent.setup();
      mockGetStagedFiles.mockResolvedValue([
        makeFileChange("src/StagedFile.tsx", { additions: 6, deletions: 4 }),
      ]);
      const changes = [makeFileChange("src/Foo.tsx", { additions: 99, deletions: 88 })];
      render(
        withProviders(
          <AgentsPublishInlineDiffs
            conversationId="conv-1"
            review={makeReview(changes)}
            commits={[]}
            isLoading={false}
          />,
        ),
      );
      await user.click(screen.getByRole("button", { name: "Staged" }));
      await waitFor(() =>
        expect(screen.getByTestId("inline-diffs-additions")).toHaveTextContent("+6"),
      );
      expect(screen.getByTestId("inline-diffs-deletions")).toHaveTextContent("−4");
    });

    it("skips staged full-diff fetching when page refs are available", async () => {
      const user = userEvent.setup();
      const changes = [makeFileChange("src/Foo.tsx")];
      render(
        withProviders(
          <AgentsPublishInlineDiffs
            conversationId="conv-1"
            review={makeReview(changes)}
            commits={[]}
            isLoading={false}
          />,
        ),
      );
      await user.click(screen.getByRole("button", { name: "Staged" }));
      await waitFor(() =>
        expect(screen.getByTestId("mock-file-diff-src-StagedFile.tsx")).toBeInTheDocument(),
      );
      fireVirtualRange(0, 0);
      await waitFor(() =>
        expect(screen.getByTestId("mock-file-diff-src-StagedFile.tsx")).toHaveAttribute(
          "data-should-hydrate",
          "true",
        ),
      );
      expect(screen.getByTestId("mock-file-diff-src-StagedFile.tsx")).toHaveAttribute(
        "data-diff-page-ref-kind",
        "staged",
      );
      await new Promise((resolve) => setTimeout(resolve, 10));
      expect(mockGetStagedFileDiff).not.toHaveBeenCalledWith("conv-1", "src/StagedFile.tsx");
    });

    it("passes refKind { kind: 'staged' } to file diff cards in staged mode", async () => {
      const user = userEvent.setup();
      const changes = [makeFileChange("src/Foo.tsx")];
      render(
        withProviders(
          <AgentsPublishInlineDiffs
            conversationId="conv-1"
            review={makeReview(changes)}
            commits={[]}
            isLoading={false}
          />,
        ),
      );
      await user.click(screen.getByRole("button", { name: "Staged" }));
      await waitFor(() => {
        const card = screen.getByTestId("mock-file-diff-src-StagedFile.tsx");
        expect(card).toHaveAttribute("data-ref-kind", "staged");
        expect(card).toHaveAttribute("data-conversation-id", "conv-1");
      });
    });

    it("does not pass PR annotations to staged file cards", async () => {
      const user = userEvent.setup();
      const changes = [makeFileChange("src/Foo.tsx")];
      mockGetStagedFiles.mockResolvedValue([makeFileChange("src/Foo.tsx")]);
      render(
        withProviders(
          <AgentsPublishInlineDiffs
            conversationId="conv-1"
            review={makeReview(changes)}
            commits={[]}
            isLoading={false}
            annotations={[makeAnnotation("src/Foo.tsx")]}
          />,
        ),
      );

      await user.click(screen.getByRole("button", { name: "Staged" }));

      await waitFor(() =>
        expect(screen.getByTestId("mock-file-diff-src-Foo.tsx")).toHaveAttribute(
          "data-annotation-count",
          "0",
        ),
      );
    });

    it("passes staged workspace review hunk annotations only to staged file cards", async () => {
      const user = userEvent.setup();
      const changes = [makeFileChange("src/Foo.tsx")];
      mockGetStagedFiles.mockResolvedValue([makeFileChange("src/Foo.tsx")]);
      render(
        withProviders(
          <AgentsPublishInlineDiffs
            conversationId="conv-1"
            review={makeReview(changes)}
            commits={[]}
            isLoading={false}
            hunkAnnotations={[
              makeHunkAnnotation("src/Foo.tsx", {
                id: "workspace-review-hunk-staged",
                diffSource: "staged",
              }),
              makeHunkAnnotation("src/Foo.tsx", {
                id: "workspace-review-hunk-unstaged",
                diffSource: "unstaged",
              }),
              makeHunkAnnotation("src/Other.tsx", {
                id: "workspace-review-hunk-other",
                diffSource: "staged",
              }),
            ]}
          />,
        ),
      );

      await user.click(screen.getByRole("button", { name: "Staged" }));

      await waitFor(() =>
        expect(screen.getByTestId("mock-file-diff-src-Foo.tsx")).toHaveAttribute(
          "data-hunk-annotation-count",
          "1",
        ),
      );
    });
  });

  describe("mode=unstaged — diff fetching", () => {
    beforeEach(() => {
      mockGetUnstagedFiles.mockResolvedValue([makeFileChange("src/UnstagedFile.tsx")]);
    });

    it("fetches unstaged file changes when mode switches to unstaged", async () => {
      const user = userEvent.setup();
      const changes = [makeFileChange("src/Foo.tsx")];
      render(
        withProviders(
          <AgentsPublishInlineDiffs
            conversationId="conv-1"
            review={makeReview(changes)}
            commits={[]}
            isLoading={false}
          />,
        ),
      );
      await user.click(screen.getByRole("button", { name: "Unstaged" }));
      await waitFor(() => expect(mockGetUnstagedFiles).toHaveBeenCalledWith("conv-1"));
    });

    it("renders unstaged file list (not workspace-change list) when mode is unstaged", async () => {
      const user = userEvent.setup();
      const changes = [makeFileChange("src/Foo.tsx")];
      render(
        withProviders(
          <AgentsPublishInlineDiffs
            conversationId="conv-1"
            review={makeReview(changes)}
            commits={[]}
            isLoading={false}
          />,
        ),
      );
      await user.click(screen.getByRole("button", { name: "Unstaged" }));
      await waitFor(() =>
        expect(screen.getByTestId("mock-file-diff-src-UnstagedFile.tsx")).toBeInTheDocument(),
      );
      expect(screen.queryByTestId("mock-file-diff-src-Foo.tsx")).toBeNull();
    });

    it("renders backend-provided untracked files in unstaged mode", async () => {
      const user = userEvent.setup();
      const untrackedFile = makeFileChange("docs/untracked.md", {
        status: "added",
        additions: 2,
        deletions: 0,
      });
      mockGetUnstagedFiles.mockResolvedValue([untrackedFile]);

      render(
        withProviders(
          <AgentsPublishInlineDiffs
            conversationId="conv-1"
            review={makeReview([makeFileChange("src/Foo.tsx")])}
            commits={[]}
            isLoading={false}
          />,
        ),
      );

      await user.click(screen.getByRole("button", { name: "Unstaged" }));
      await waitFor(() =>
        expect(screen.getByTestId("mock-file-diff-docs-untracked.md")).toBeInTheDocument(),
      );
      expect(screen.getByTestId("mock-diff-filter")).toHaveAttribute("data-unstaged-count", "1");
      expect(screen.queryByTestId("inline-diffs-empty")).toBeNull();
      expect(screen.queryByTestId("mock-file-diff-src-Foo.tsx")).toBeNull();
    });

    it("shows +/− totals from unstaged file list in sticky bar", async () => {
      const user = userEvent.setup();
      mockGetUnstagedFiles.mockResolvedValue([
        makeFileChange("src/UnstagedFile.tsx", { additions: 3, deletions: 1 }),
      ]);
      const changes = [makeFileChange("src/Foo.tsx", { additions: 99, deletions: 88 })];
      render(
        withProviders(
          <AgentsPublishInlineDiffs
            conversationId="conv-1"
            review={makeReview(changes)}
            commits={[]}
            isLoading={false}
          />,
        ),
      );
      await user.click(screen.getByRole("button", { name: "Unstaged" }));
      await waitFor(() =>
        expect(screen.getByTestId("inline-diffs-additions")).toHaveTextContent("+3"),
      );
      expect(screen.getByTestId("inline-diffs-deletions")).toHaveTextContent("−1");
    });

    it("skips unstaged full-diff fetching when page refs are available", async () => {
      const user = userEvent.setup();
      const changes = [makeFileChange("src/Foo.tsx")];
      render(
        withProviders(
          <AgentsPublishInlineDiffs
            conversationId="conv-1"
            review={makeReview(changes)}
            commits={[]}
            isLoading={false}
          />,
        ),
      );
      await user.click(screen.getByRole("button", { name: "Unstaged" }));
      await waitFor(() =>
        expect(screen.getByTestId("mock-file-diff-src-UnstagedFile.tsx")).toBeInTheDocument(),
      );
      fireVirtualRange(0, 0);
      await waitFor(() =>
        expect(screen.getByTestId("mock-file-diff-src-UnstagedFile.tsx")).toHaveAttribute(
          "data-should-hydrate",
          "true",
        ),
      );
      expect(screen.getByTestId("mock-file-diff-src-UnstagedFile.tsx")).toHaveAttribute(
        "data-diff-page-ref-kind",
        "unstaged",
      );
      await new Promise((resolve) => setTimeout(resolve, 10));
      expect(mockGetUnstagedFileDiff).not.toHaveBeenCalledWith(
        "conv-1",
        "src/UnstagedFile.tsx",
      );
    });
  });

  describe("mode=cumulative — diff fetching", () => {
    it("uses read-only cumulative mode when historical review cannot inspect a worktree", async () => {
      const changes = [makeFileChange("src/Foo.tsx")];
      render(
        withProviders(
          <AgentsPublishInlineDiffs
            conversationId="conv-1"
            review={{ ...makeReview(changes), supportsWorktreeModes: false }}
            commits={[makeCommit("sha-abc")]}
            isLoading={false}
          />,
        ),
      );

      expect(screen.getByTestId("mock-diff-filter")).toHaveAttribute(
        "data-mode",
        "cumulative",
      );
      expect(screen.getByTestId("mock-diff-filter")).toHaveAttribute(
        "data-supports-worktree-modes",
        "false",
      );
      await waitFor(() => expect(mockGetCumulativeFiles).toHaveBeenCalledWith("conv-1"));
      expect(mockGetStagedFiles).not.toHaveBeenCalled();
      expect(mockGetUnstagedFiles).not.toHaveBeenCalled();
    });

    it("fetches cumulative file changes when mode switches to cumulative", async () => {
      const user = userEvent.setup();
      const changes = [makeFileChange("src/Foo.tsx")];
      render(
        withProviders(
          <AgentsPublishInlineDiffs
            conversationId="conv-1"
            review={makeReview(changes)}
            commits={[makeCommit("sha-abc")]}
            isLoading={false}
          />,
        ),
      );
      await user.click(screen.getByRole("button", { name: "All commits" }));
      await waitFor(() => expect(mockGetCumulativeFiles).toHaveBeenCalledWith("conv-1"));
    });

    it("renders cumulative file list (not workspace-change list) when mode is cumulative", async () => {
      const user = userEvent.setup();
      const changes = [makeFileChange("src/Foo.tsx")];
      render(
        withProviders(
          <AgentsPublishInlineDiffs
            conversationId="conv-1"
            review={makeReview(changes)}
            commits={[makeCommit("sha-abc")]}
            isLoading={false}
          />,
        ),
      );
      await user.click(screen.getByRole("button", { name: "All commits" }));
      await waitFor(() =>
        expect(screen.getByTestId("mock-file-diff-src-CumulativeFile.tsx")).toBeInTheDocument(),
      );
      expect(screen.queryByTestId("mock-file-diff-src-Foo.tsx")).toBeNull();
    });

    it("shows file count from cumulative file list in sticky bar", async () => {
      const user = userEvent.setup();
      mockGetCumulativeFiles.mockResolvedValue([
        makeFileChange("src/CumulativeFile.tsx"),
        makeFileChange("src/AnotherCumulative.tsx"),
      ]);
      const changes = [makeFileChange("src/Foo.tsx")];
      render(
        withProviders(
          <AgentsPublishInlineDiffs
            conversationId="conv-1"
            review={makeReview(changes)}
            commits={[makeCommit("sha-abc")]}
            isLoading={false}
          />,
        ),
      );
      await user.click(screen.getByRole("button", { name: "All commits" }));
      await waitFor(() =>
        expect(screen.getByTestId("inline-diffs-file-count")).toHaveTextContent("2"),
      );
    });

    it("shows +/− totals from cumulative file list in sticky bar", async () => {
      const user = userEvent.setup();
      mockGetCumulativeFiles.mockResolvedValue([
        makeFileChange("src/CumulativeFile.tsx", { additions: 12, deletions: 5 }),
      ]);
      const changes = [makeFileChange("src/Foo.tsx", { additions: 99, deletions: 88 })];
      render(
        withProviders(
          <AgentsPublishInlineDiffs
            conversationId="conv-1"
            review={makeReview(changes)}
            commits={[makeCommit("sha-abc")]}
            isLoading={false}
          />,
        ),
      );
      await user.click(screen.getByRole("button", { name: "All commits" }));
      await waitFor(() =>
        expect(screen.getByTestId("inline-diffs-additions")).toHaveTextContent("+12"),
      );
      expect(screen.getByTestId("inline-diffs-deletions")).toHaveTextContent("−5");
    });

    it("skips cumulative full-diff fetching when page refs are available", async () => {
      const user = userEvent.setup();
      const changes = [makeFileChange("src/Foo.tsx")];
      render(
        withProviders(
          <AgentsPublishInlineDiffs
            conversationId="conv-1"
            review={makeReview(changes)}
            commits={[makeCommit("sha-abc")]}
            isLoading={false}
          />,
        ),
      );
      await user.click(screen.getByRole("button", { name: "All commits" }));
      await waitFor(() =>
        expect(screen.getByTestId("mock-file-diff-src-CumulativeFile.tsx")).toBeInTheDocument(),
      );
      fireVirtualRange(0, 0);
      await waitFor(() =>
        expect(screen.getByTestId("mock-file-diff-src-CumulativeFile.tsx")).toHaveAttribute(
          "data-should-hydrate",
          "true",
        ),
      );
      expect(screen.getByTestId("mock-file-diff-src-CumulativeFile.tsx")).toHaveAttribute(
        "data-diff-page-ref-kind",
        "cumulative_head",
      );
      await new Promise((resolve) => setTimeout(resolve, 10));
      expect(mockGetCumulativeFileDiff).not.toHaveBeenCalledWith(
        "conv-1",
        "src/CumulativeFile.tsx",
      );
    });

    it("passes cumulative_head refKind for branch-backed read-only review context", async () => {
      const changes = [makeFileChange("src/Foo.tsx")];
      render(
        withProviders(
          <AgentsPublishInlineDiffs
            conversationId="conv-1"
            review={{
              ...makeReview(changes),
              headRef: "refs/ralphx/pr-heads/123",
              supportsWorktreeModes: false,
            }}
            commits={[makeCommit("sha-abc")]}
            isLoading={false}
          />,
        ),
      );

      await waitFor(() =>
        expect(screen.getByTestId("mock-file-diff-src-CumulativeFile.tsx")).toHaveAttribute(
          "data-ref-kind",
          "cumulative_head",
        ),
      );
    });

    it("omits range refKind for patch-backed read-only review context", async () => {
      const changes = [makeFileChange("src/Foo.tsx")];
      render(
        withProviders(
          <AgentsPublishInlineDiffs
            conversationId="conv-1"
            review={{
              ...makeReview(changes),
              headRef: "github-pr-diff/123",
              supportsWorktreeModes: false,
            }}
            commits={[makeCommit("sha-abc")]}
            isLoading={false}
          />,
        ),
      );

      await waitFor(() => {
        const card = screen.getByTestId("mock-file-diff-src-CumulativeFile.tsx");
        expect(card).not.toHaveAttribute("data-ref-kind");
        expect(card).toHaveAttribute("data-conversation-id", "conv-1");
      });
    });

    it("passes matching GitHub annotations to cumulative file cards", async () => {
      const user = userEvent.setup();
      const changes = [makeFileChange("src/Foo.tsx")];
      mockGetCumulativeFiles.mockResolvedValue([makeFileChange("src/Foo.tsx")]);
      render(
        withProviders(
          <AgentsPublishInlineDiffs
            conversationId="conv-1"
            review={makeReview(changes)}
            commits={[makeCommit("sha-abc")]}
            isLoading={false}
            annotations={[makeAnnotation("src/Foo.tsx")]}
          />,
        ),
      );

      await user.click(screen.getByRole("button", { name: "All commits" }));

      await waitFor(() =>
        expect(screen.getByTestId("mock-file-diff-src-Foo.tsx")).toHaveAttribute(
          "data-annotation-count",
          "1",
        ),
      );
    });

    it("passes committed workspace review hunk annotations to cumulative file cards", async () => {
      const user = userEvent.setup();
      const changes = [makeFileChange("src/Foo.tsx")];
      mockGetCumulativeFiles.mockResolvedValue([makeFileChange("src/Foo.tsx")]);
      render(
        withProviders(
          <AgentsPublishInlineDiffs
            conversationId="conv-1"
            review={makeReview(changes)}
            commits={[makeCommit("sha-abc")]}
            isLoading={false}
            hunkAnnotations={[
              makeHunkAnnotation("src/Foo.tsx", {
                id: "workspace-review-hunk-committed",
                diffSource: "committed",
              }),
              makeHunkAnnotation("src/Foo.tsx", {
                id: "workspace-review-hunk-staged",
                diffSource: "staged",
              }),
            ]}
          />,
        ),
      );

      await user.click(screen.getByRole("button", { name: "All commits" }));

      await waitFor(() =>
        expect(screen.getByTestId("mock-file-diff-src-Foo.tsx")).toHaveAttribute(
          "data-hunk-annotation-count",
          "1",
        ),
      );
    });
  });

  describe("loading state", () => {
    it("shows loading skeleton when isLoading=true", () => {
      render(
        withProviders(
          <AgentsPublishInlineDiffs
            conversationId="conv-1"
            review={null}
            commits={[]}
            isLoading={true}
          />,
        ),
      );
      expect(screen.getByTestId("inline-diffs-loading")).toBeInTheDocument();
    });

    it("shows loading skeleton while cumulative files query is pending for a merged workspace with empty review changes", async () => {
      let resolveCumulative!: (value: unknown[]) => void;
      mockGetCumulativeFiles.mockReturnValue(
        new Promise((resolve) => {
          resolveCumulative = resolve;
        }),
      );
      const review = { ...makeReview([]), supportsWorktreeModes: false as const };
      render(
        withProviders(
          <AgentsPublishInlineDiffs
            conversationId="conv-merged"
            review={review}
            commits={[makeCommit("sha-abc")]}
            isLoading={false}
          />,
        ),
      );
      expect(screen.getByTestId("inline-diffs-loading")).toBeInTheDocument();
      expect(screen.queryByTestId("inline-diffs-empty")).toBeNull();

      await act(async () => {
        resolveCumulative([makeFileChange("src/MergedFile.tsx")]);
      });
      await waitFor(() =>
        expect(screen.getByTestId("mock-file-diff-src-MergedFile.tsx")).toBeInTheDocument(),
      );
      expect(screen.queryByTestId("inline-diffs-loading")).toBeNull();
      expect(screen.queryByTestId("inline-diffs-empty")).toBeNull();
    });

    it("defaults merged workspaces to All commits while preserving manual mode changes", async () => {
      const user = userEvent.setup();
      mockGetCumulativeFiles.mockResolvedValue([makeFileChange("src/MergedFile.tsx")]);

      render(
        withProviders(
          <AgentsPublishInlineDiffs
            conversationId="conv-merged"
            review={makeReview([])}
            commits={[makeCommit("sha-abc")]}
            isLoading={false}
            defaultMode="cumulative"
          />,
        ),
      );

      expect(screen.getByTestId("mock-diff-filter")).toHaveAttribute(
        "data-mode",
        "cumulative",
      );
      await waitFor(() =>
        expect(screen.getByTestId("mock-file-diff-src-MergedFile.tsx")).toBeInTheDocument(),
      );

      await user.click(screen.getByRole("button", { name: "Workspace changes" }));

      expect(screen.getByTestId("mock-diff-filter")).toHaveAttribute(
        "data-mode",
        "uncommitted",
      );
      expect(screen.getByText("No workspace changes")).toBeInTheDocument();
    });
  });

  describe("openInDialog", () => {
    it("renders the full-dialog action after jump-to-file in the diff header", () => {
      const changes = [makeFileChange("src/Foo.tsx")];
      render(
        withProviders(
          <AgentsPublishInlineDiffs
            conversationId="conv-1"
            review={makeReview(changes)}
            commits={[]}
            isLoading={false}
            onOpenInDialog={vi.fn()}
          />,
        ),
      );

      const stickyBar = screen.getByTestId("inline-diffs-sticky-bar");
      const jumpToFile = within(stickyBar).getByTestId("inline-diffs-jump-to-file");
      const openDialog = within(stickyBar).getByTestId("agents-review-changes");

      expect(openDialog).toHaveAttribute("aria-label", "Open changes in full diff dialog");
      expect(
        jumpToFile.compareDocumentPosition(openDialog) & Node.DOCUMENT_POSITION_FOLLOWING,
      ).toBeTruthy();
    });

    it("calls onOpenInDialog from the diff header action", async () => {
      const user = userEvent.setup();
      const onOpenInDialog = vi.fn();
      const changes = [makeFileChange("src/Foo.tsx")];
      render(
        withProviders(
          <AgentsPublishInlineDiffs
            conversationId="conv-1"
            review={makeReview(changes)}
            commits={[]}
            isLoading={false}
            onOpenInDialog={onOpenInDialog}
          />,
        ),
      );

      await user.click(screen.getByTestId("agents-review-changes"));
      expect(onOpenInDialog).toHaveBeenCalledOnce();
      expect(onOpenInDialog.mock.calls[0]).toEqual([]);
    });

    it("calls onOpenInDialog when fullscreen button is clicked in a card", async () => {
      const user = userEvent.setup();
      const onOpenInDialog = vi.fn();
      const changes = [makeFileChange("src/Foo.tsx")];
      render(
        withProviders(
          <AgentsPublishInlineDiffs
            conversationId="conv-1"
            review={makeReview(changes)}
            commits={[]}
            isLoading={false}
            onOpenInDialog={onOpenInDialog}
          />,
        ),
      );
      await user.click(screen.getByRole("button", { name: "fullscreen" }));
      expect(onOpenInDialog).toHaveBeenCalledWith("src/Foo.tsx");
    });
  });

  describe("jump-to-file", () => {
    it("renders jump-to-file button in sticky bar", () => {
      const changes = [makeFileChange("src/Foo.tsx")];
      render(
        withProviders(
          <AgentsPublishInlineDiffs
            conversationId="conv-1"
            review={makeReview(changes)}
            commits={[]}
            isLoading={false}
          />,
        ),
      );
      expect(screen.getByTestId("inline-diffs-jump-to-file")).toBeInTheDocument();
      expect(screen.getByTestId("inline-diffs-jump-to-file")).toHaveAttribute("aria-label");
    });

    it("opens jump popover with file paths when button is clicked", async () => {
      const user = userEvent.setup();
      const changes = [makeFileChange("src/Foo.tsx"), makeFileChange("src/Bar.tsx")];
      render(
        withProviders(
          <AgentsPublishInlineDiffs
            conversationId="conv-1"
            review={makeReview(changes)}
            commits={[]}
            isLoading={false}
          />,
        ),
      );
      await user.click(screen.getByTestId("inline-diffs-jump-to-file"));
      expect(screen.getByTestId("jump-to-file-popover")).toBeInTheDocument();
      expect(screen.getByTestId("jump-to-file-item-src/Foo.tsx")).toBeInTheDocument();
      expect(screen.getByTestId("jump-to-file-item-src/Bar.tsx")).toBeInTheDocument();
    });

    it("filters file list in jump popover by search input", async () => {
      const user = userEvent.setup();
      const changes = [makeFileChange("src/Foo.tsx"), makeFileChange("src/Bar.tsx")];
      render(
        withProviders(
          <AgentsPublishInlineDiffs
            conversationId="conv-1"
            review={makeReview(changes)}
            commits={[]}
            isLoading={false}
          />,
        ),
      );
      await user.click(screen.getByTestId("inline-diffs-jump-to-file"));
      await user.type(screen.getByTestId("jump-to-file-search"), "Foo");
      expect(screen.getByTestId("jump-to-file-item-src/Foo.tsx")).toBeInTheDocument();
      expect(screen.queryByTestId("jump-to-file-item-src/Bar.tsx")).toBeNull();
    });

    it("scrolls the virtual list when a file is selected from the jump popover", async () => {
      const user = userEvent.setup();
      const changes = [makeFileChange("src/Foo.tsx"), makeFileChange("src/Bar.tsx")];
      render(
        withProviders(
          <AgentsPublishInlineDiffs
            conversationId="conv-1"
            review={makeReview(changes)}
            commits={[]}
            isLoading={false}
          />,
        ),
      );
      await user.click(screen.getByTestId("inline-diffs-jump-to-file"));
      await user.click(screen.getByTestId("jump-to-file-item-src/Bar.tsx"));
      expect(virtuosoMockState.scrollToIndex).toHaveBeenCalledWith({
        align: "start",
        behavior: "auto",
        index: 1,
      });
      await waitFor(() =>
        expect(annotationScrollIntoViewMock).toHaveBeenCalledWith({
          block: "start",
          behavior: "auto",
          inline: "nearest",
        }),
      );
      expect(screen.getByTestId("mock-file-diff-src-Bar.tsx")).toHaveAttribute(
        "data-focus-target",
        "true",
      );
    });

    it("keeps the selected jump file aligned while the row resizes", async () => {
      const user = userEvent.setup();
      const changes = [makeFileChange("src/Foo.tsx"), makeFileChange("src/Bar.tsx")];
      const originalResizeObserver = globalThis.ResizeObserver;
      const originalWindowResizeObserver = window.ResizeObserver;
      const resizeCallbacks: ResizeObserverCallback[] = [];

      class TestResizeObserver {
        constructor(callback: ResizeObserverCallback) {
          resizeCallbacks.push(callback);
        }

        observe = vi.fn();
        unobserve = vi.fn();
        disconnect = vi.fn();
      }

      vi.stubGlobal("ResizeObserver", TestResizeObserver);
      Object.defineProperty(window, "ResizeObserver", {
        configurable: true,
        writable: true,
        value: TestResizeObserver,
      });
      try {
        render(
          withProviders(
            <AgentsPublishInlineDiffs
              conversationId="conv-1"
              review={makeReview(changes)}
              commits={[]}
              isLoading={false}
            />,
          ),
        );
        await user.click(screen.getByTestId("inline-diffs-jump-to-file"));
        await user.click(screen.getByTestId("jump-to-file-item-src/Bar.tsx"));
        await waitFor(() =>
          expect(annotationScrollIntoViewMock).toHaveBeenCalledWith({
            block: "start",
            behavior: "auto",
            inline: "nearest",
          }),
        );

        annotationScrollIntoViewMock.mockClear();
        expect(resizeCallbacks.length).toBeGreaterThan(0);
        act(() => {
          for (const resizeCallback of resizeCallbacks) {
            resizeCallback([], {} as ResizeObserver);
          }
        });

        expect(annotationScrollIntoViewMock).toHaveBeenCalledWith({
          block: "start",
          behavior: "auto",
          inline: "nearest",
        });
      } finally {
        vi.stubGlobal("ResizeObserver", originalResizeObserver);
        Object.defineProperty(window, "ResizeObserver", {
          configurable: true,
          writable: true,
          value: originalWindowResizeObserver,
        });
      }
    });

    it("keeps the selected jump file aligned during settle frames and releases on user input", async () => {
      const user = userEvent.setup();
      const changes = [makeFileChange("src/Foo.tsx"), makeFileChange("src/Bar.tsx")];
      const originalRequestAnimationFrame = window.requestAnimationFrame;
      const originalCancelAnimationFrame = window.cancelAnimationFrame;
      const frameCallbacks: Array<{ callback: FrameRequestCallback; id: number }> = [];
      const cancelledFrames = new Set<number>();
      let nextFrameId = 1;

      Object.defineProperty(window, "requestAnimationFrame", {
        configurable: true,
        writable: true,
        value: (callback: FrameRequestCallback) => {
          const id = nextFrameId++;
          frameCallbacks.push({ callback, id });
          return id;
        },
      });
      Object.defineProperty(window, "cancelAnimationFrame", {
        configurable: true,
        writable: true,
        value: (id: number) => {
          cancelledFrames.add(id);
        },
      });

      try {
        render(
          withProviders(
            <AgentsPublishInlineDiffs
              conversationId="conv-1"
              review={makeReview(changes)}
              commits={[]}
              isLoading={false}
            />,
          ),
        );
        await user.click(screen.getByTestId("inline-diffs-jump-to-file"));
        await user.click(screen.getByTestId("jump-to-file-item-src/Bar.tsx"));
        await waitFor(() =>
          expect(annotationScrollIntoViewMock).toHaveBeenCalledWith({
            block: "start",
            behavior: "auto",
            inline: "nearest",
          }),
        );

        annotationScrollIntoViewMock.mockClear();
        const firstFrame = frameCallbacks.shift();
        expect(firstFrame).toBeDefined();
        act(() => {
          firstFrame?.callback(performance.now());
        });
        expect(annotationScrollIntoViewMock).toHaveBeenCalledWith({
          block: "start",
          behavior: "auto",
          inline: "nearest",
        });

        annotationScrollIntoViewMock.mockClear();
        act(() => {
          window.dispatchEvent(new Event("wheel"));
        });
        const cancelledFrame = frameCallbacks.shift();
        expect(cancelledFrame).toBeDefined();
        expect(cancelledFrames.has(cancelledFrame!.id)).toBe(true);
        act(() => {
          cancelledFrame?.callback(performance.now());
        });
        expect(annotationScrollIntoViewMock).not.toHaveBeenCalled();
      } finally {
        Object.defineProperty(window, "requestAnimationFrame", {
          configurable: true,
          writable: true,
          value: originalRequestAnimationFrame,
        });
        Object.defineProperty(window, "cancelAnimationFrame", {
          configurable: true,
          writable: true,
          value: originalCancelAnimationFrame,
        });
      }
    });

    it("scrolls and expands a file from an external focus request", async () => {
      const user = userEvent.setup();
      const changes = [makeFileChange("src/Foo.tsx"), makeFileChange("src/Bar.tsx")];
      const client = makeQueryClient();
      const { rerender } = render(
        withProviders(
          <AgentsPublishInlineDiffs
            conversationId="conv-1"
            review={makeReview(changes)}
            commits={[]}
            isLoading={false}
          />,
          client,
        ),
      );

      await user.click(screen.getByTestId("inline-diffs-collapse-all"));
      expect(screen.getByTestId("mock-file-diff-src-Bar.tsx")).toHaveAttribute(
        "data-expanded",
        "false",
      );

      rerender(
        withProviders(
          <AgentsPublishInlineDiffs
            conversationId="conv-1"
            review={makeReview(changes)}
            commits={[]}
            isLoading={false}
            focusRequest={{
              conversationId: "conv-1",
              filePath: "src/Bar.tsx",
              mode: "uncommitted",
              requestId: 1,
            }}
          />,
          client,
        ),
      );

      await waitFor(() =>
        expect(virtuosoMockState.scrollToIndex).toHaveBeenCalledWith({
          align: "start",
          behavior: "auto",
          index: 1,
        }),
      );
      expect(screen.getByTestId("mock-file-diff-src-Bar.tsx")).toHaveAttribute(
        "data-expanded",
        "true",
      );
    });

    it("closes jump popover after selecting a file", async () => {
      const user = userEvent.setup();
      const changes = [makeFileChange("src/Foo.tsx")];
      render(
        withProviders(
          <AgentsPublishInlineDiffs
            conversationId="conv-1"
            review={makeReview(changes)}
            commits={[]}
            isLoading={false}
          />,
        ),
      );
      await user.click(screen.getByTestId("inline-diffs-jump-to-file"));
      expect(screen.getByTestId("jump-to-file-popover")).toBeInTheDocument();
      await user.click(screen.getByTestId("jump-to-file-item-src/Foo.tsx"));
      expect(screen.queryByTestId("jump-to-file-popover")).toBeNull();
    });
  });

  describe("lazy hydration — virtual range", () => {
    it("self-hydrates a mounted row without an initial range callback", async () => {
      const changes = [makeFileChange("src/Foo.tsx")];
      render(
        withProviders(
          <AgentsPublishInlineDiffs
            conversationId="conv-1"
            review={makeReview(changes)}
            commits={[]}
            isLoading={false}
          />,
        ),
      );
      const card = screen.getByTestId("mock-file-diff-src-Foo.tsx");
      await waitFor(() =>
        expect(card).toHaveAttribute("data-should-hydrate", "true"),
      );
      await waitFor(() =>
        expect(mockGetDiffPage).toHaveBeenCalledWith(
          expect.objectContaining({
            conversationId: "conv-1",
            path: "src/Foo.tsx",
          }),
        ),
      );
    });

    it("re-registers a mounted row when the hydration generation changes", async () => {
      const changes = [makeFileChange("src/Foo.tsx")];
      const client = makeQueryClient();
      const { rerender } = render(
        withProviders(
          <AgentsPublishInlineDiffs
            conversationId="conv-1"
            review={makeReview(changes)}
            commits={[]}
            isLoading={false}
          />,
          client,
        ),
      );
      await waitFor(() =>
        expect(
          screen.getByTestId("mock-file-diff-src-Foo.tsx"),
        ).toHaveAttribute("data-should-hydrate", "true"),
      );

      rerender(
        withProviders(
          <AgentsPublishInlineDiffs
            conversationId="conv-2"
            review={makeReview(changes)}
            commits={[]}
            isLoading={false}
          />,
          client,
        ),
      );

      await waitFor(() => {
        const card = screen.getByTestId("mock-file-diff-src-Foo.tsx");
        expect(card).toHaveAttribute("data-conversation-id", "conv-2");
        expect(card).toHaveAttribute("data-should-hydrate", "true");
      });
    });

    it("does not probe paged summaries for an unmounted off-range file", async () => {
      virtuosoMockState.range = { startIndex: 0, endIndex: 0 };
      const changes = [
        makeFileChange("src/Visible.tsx", { additions: 800 }),
        makeFileChange("src/Offscreen.tsx", { additions: 900 }),
      ];
      render(
        withProviders(
          <AgentsPublishInlineDiffs
            conversationId="conv-1"
            review={makeReview(changes)}
            commits={[]}
            isLoading={false}
          />,
        ),
      );

      await waitFor(() =>
        expect(mockGetDiffPage).toHaveBeenCalledWith(
          expect.objectContaining({ path: "src/Visible.tsx" }),
        ),
      );
      expect(mockGetDiffPage).not.toHaveBeenCalledWith(
        expect.objectContaining({ path: "src/Offscreen.tsx" }),
      );
    });

    it("sets shouldHydrate=true when the virtual range includes a file", async () => {
      const changes = [makeFileChange("src/Foo.tsx")];
      render(
        withProviders(
          <AgentsPublishInlineDiffs
            conversationId="conv-1"
            review={makeReview(changes)}
            commits={[]}
            isLoading={false}
          />,
        ),
      );

      fireVirtualRange(0, 0);

      await waitFor(() => {
        expect(screen.getByTestId("mock-file-diff-src-Foo.tsx")).toHaveAttribute(
          "data-should-hydrate",
          "true",
        );
      });
    });

    it("keeps off-range files out of the rendered and fetched range", async () => {
      const changes = [makeFileChange("src/Foo.tsx"), makeFileChange("src/Bar.tsx")];
      render(
        withProviders(
          <AgentsPublishInlineDiffs
            conversationId="conv-1"
            review={makeReview(changes)}
            commits={[]}
            isLoading={false}
          />,
        ),
      );

      fireVirtualRange(0, 0);

      await waitFor(() =>
        expect(screen.getByTestId("mock-file-diff-src-Foo.tsx")).toHaveAttribute(
          "data-should-hydrate",
          "true",
        ),
      );
      expect(screen.queryByTestId("mock-file-diff-src-Bar.tsx")).toBeNull();
      expect(mockGetUncommittedDiff).not.toHaveBeenCalledWith("conv-1", "src/Bar.tsx");
    });

    it("rehydrates visible files when mode changes back", async () => {
      const user = userEvent.setup();
      const changes = [makeFileChange("src/Foo.tsx")];
      render(
        withProviders(
          <AgentsPublishInlineDiffs
            conversationId="conv-1"
            review={makeReview(changes)}
            commits={[]}
            isLoading={false}
          />,
        ),
      );

      fireVirtualRange(0, 0);
      await waitFor(() => {
        expect(screen.getByTestId("mock-file-diff-src-Foo.tsx")).toHaveAttribute(
          "data-should-hydrate",
          "true",
        );
      });

      // Switch mode → hydratedPaths resets, but the current viewport range is still known.
      await user.click(screen.getByRole("button", { name: "Staged" }));

      // When the workspace-change card returns, visible rows should hydrate immediately.
      await user.click(screen.getByRole("button", { name: "Workspace changes" }));
      await waitFor(() => {
        expect(screen.getByTestId("mock-file-diff-src-Foo.tsx")).toHaveAttribute(
          "data-should-hydrate",
          "true",
        );
      });
    });
  });

  describe("show-anyway — generated files", () => {
    it("initially passes isShowAnywayOverridden=false", () => {
      const changes = [makeFileChange("src/Foo.tsx", { isGenerated: true })];
      render(
        withProviders(
          <AgentsPublishInlineDiffs
            conversationId="conv-1"
            review={makeReview(changes)}
            commits={[]}
            isLoading={false}
          />,
        ),
      );
      expect(screen.getByTestId("mock-file-diff-src-Foo.tsx")).toHaveAttribute(
        "data-show-anyway-overridden",
        "false",
      );
    });

    it("sets isShowAnywayOverridden=true after clicking Show anyway", async () => {
      const user = userEvent.setup();
      const changes = [makeFileChange("src/Foo.tsx", { isGenerated: true })];
      render(
        withProviders(
          <AgentsPublishInlineDiffs
            conversationId="conv-1"
            review={makeReview(changes)}
            commits={[]}
            isLoading={false}
          />,
        ),
      );

      await user.click(screen.getByTestId("show-anyway-src-Foo.tsx"));

      await waitFor(() => {
        expect(screen.getByTestId("mock-file-diff-src-Foo.tsx")).toHaveAttribute(
          "data-show-anyway-overridden",
          "true",
        );
      });
    });

    it("does not fetch page-capable generated full diffs before or after Show anyway", async () => {
      const user = userEvent.setup();
      const changes = [makeFileChange("src/Foo.tsx", { isGenerated: true })];
      render(
        withProviders(
          <AgentsPublishInlineDiffs
            conversationId="conv-1"
            review={makeReview(changes)}
            commits={[]}
            isLoading={false}
          />,
        ),
      );

      fireVirtualRange(0, 0);
      await new Promise((r) => setTimeout(r, 10));
      expect(mockGetUncommittedDiff).not.toHaveBeenCalled();

      await user.click(screen.getByTestId("show-anyway-src-Foo.tsx"));

      await waitFor(() =>
        expect(screen.getByTestId("mock-file-diff-src-Foo.tsx")).toHaveAttribute(
          "data-show-anyway-overridden",
          "true",
        ),
      );
      expect(screen.getByTestId("mock-file-diff-src-Foo.tsx")).toHaveAttribute(
        "data-diff-page-ref-kind",
        "head",
      );
      await new Promise((r) => setTimeout(r, 10));
      expect(mockGetUncommittedDiff).not.toHaveBeenCalledWith("conv-1", "src/Foo.tsx");
    });

    it("keeps large file diffs off the full-diff fetch path without Show anyway", async () => {
      const changes = [
        makeFileChange("src/Huge.tsx", { additions: 1_250, deletions: 25 }),
      ];
      render(
        withProviders(
          <AgentsPublishInlineDiffs
            conversationId="conv-1"
            review={makeReview(changes)}
            commits={[]}
            isLoading={false}
          />,
        ),
      );

      fireVirtualRange(0, 0);
      await new Promise((r) => setTimeout(r, 10));
      expect(mockGetUncommittedDiff).not.toHaveBeenCalled();
      expect(screen.getByTestId("mock-file-diff-src-Huge.tsx")).toHaveAttribute(
        "data-show-anyway-overridden",
        "false",
      );
      expect(screen.getByTestId("mock-file-diff-src-Huge.tsx")).toHaveAttribute(
        "data-diff-page-ref-kind",
        "head",
      );
      await waitFor(() =>
        expect(mockGetDiffPage).toHaveBeenCalledWith({
          conversationId: "conv-1",
          path: "src/Huge.tsx",
          refKind: { kind: "head" },
          offset: 0,
          limit: 1,
        }),
      );
      await waitFor(() =>
        expect(screen.getByTestId("mock-file-diff-src-Huge.tsx")).toHaveAttribute(
          "data-diff-page-total-rows",
          "719",
        ),
      );
      await waitFor(() =>
        expect(screen.getByTestId("mock-file-diff-src-Huge.tsx")).toHaveAttribute(
          "data-inline-scroll-parent",
          "true",
        ),
      );
    });

    it("fetches generated fallback file diffs after Show anyway when page refs are omitted", async () => {
      const user = userEvent.setup();
      const changes = [
        makeFileChange("src/generated.ts", {
          isGenerated: true,
          additions: 25,
          deletions: 5,
        }),
      ];
      render(
        withProviders(
          <AgentsPublishInlineDiffs
            conversationId="conv-1"
            review={makeReview(changes)}
            commits={[]}
            isLoading={false}
            repairMode
          />,
        ),
      );

      fireVirtualRange(0, 0);
      await user.click(screen.getByTestId("show-anyway-src-generated.ts"));

      await waitFor(() =>
        expect(mockGetUncommittedDiff).toHaveBeenCalledWith(
          "conv-1",
          "src/generated.ts",
        ),
      );
      expect(screen.getByTestId("mock-file-diff-src-generated.ts")).toHaveAttribute(
        "data-show-anyway-overridden",
        "true",
      );
      expect(screen.getByTestId("mock-file-diff-src-generated.ts")).not.toHaveAttribute(
        "data-diff-page-ref-kind",
      );
    });

    it("show-anyway override is per-file and does not affect other files", async () => {
      const user = userEvent.setup();
      const changes = [
        makeFileChange("src/Foo.tsx", { isGenerated: true }),
        makeFileChange("src/Bar.tsx", { isGenerated: true }),
      ];
      render(
        withProviders(
          <AgentsPublishInlineDiffs
            conversationId="conv-1"
            review={makeReview(changes)}
            commits={[]}
            isLoading={false}
          />,
        ),
      );

      await user.click(screen.getByTestId("show-anyway-src-Foo.tsx"));

      await waitFor(() => {
        expect(screen.getByTestId("mock-file-diff-src-Foo.tsx")).toHaveAttribute(
          "data-show-anyway-overridden",
          "true",
        );
      });
      // Bar should still be false
      expect(screen.getByTestId("mock-file-diff-src-Bar.tsx")).toHaveAttribute(
        "data-show-anyway-overridden",
        "false",
      );
    });
  });
});
