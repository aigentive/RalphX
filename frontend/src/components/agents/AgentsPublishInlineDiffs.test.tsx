/**
 * AgentsPublishInlineDiffs tests
 * Container that orchestrates fetch-management + filter + file cards.
 * Receives (conversationId, review, commits) from parent — no re-fetching at parent level.
 */

import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { QueryClient, QueryClientProvider } from "@tanstack/react-query";

import { TooltipProvider } from "@/components/ui/tooltip";

// Stub child components
vi.mock("./AgentsPublishDiffFilter", () => ({
  AgentsPublishDiffFilter: ({
    mode,
    onModeChange,
    uncommittedCount,
  }: {
    mode: string;
    onModeChange: (m: string) => void;
    uncommittedCount: number;
    stagedCount?: number;
    unstagedCount?: number;
    commits: unknown[];
  }) => (
    <div data-testid="mock-diff-filter" data-mode={mode} data-count={uncommittedCount}>
      <button onClick={() => onModeChange("uncommitted")}>Uncommitted</button>
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
    isExpanded,
    onCopyPath,
    onOpenFullscreen,
    refKind,
    conversationId,
    shouldHydrate,
    annotations,
    isShowAnywayOverridden,
    onShowAnyway,
  }: {
    file: { path: string };
    diff: unknown;
    isExpanded: boolean;
    onToggle: () => void;
    onCopyPath: (p: string) => void;
    onOpenFullscreen: (p: string) => void;
    onRetry?: () => void;
    refKind?: { kind: string };
    conversationId?: string;
    shouldHydrate: boolean;
    annotations?: unknown[];
    isShowAnywayOverridden: boolean;
    onShowAnyway: () => void;
  }) => (
    <div
      data-testid={`mock-file-diff-${file.path.replace(/\//g, "-")}`}
      data-expanded={String(isExpanded)}
      data-diff-status={typeof diff === "string" ? diff : diff ? "loaded" : "undefined"}
      data-ref-kind={refKind?.kind}
      data-conversation-id={conversationId}
      data-should-hydrate={String(shouldHydrate)}
      data-annotation-count={String(annotations?.length ?? 0)}
      data-show-anyway-overridden={String(isShowAnywayOverridden)}
    >
      <button onClick={() => onCopyPath(file.path)}>copy</button>
      <button onClick={() => onOpenFullscreen(file.path)}>fullscreen</button>
      <button
        data-testid={`show-anyway-${file.path.replace(/\//g, "-")}`}
        onClick={onShowAnyway}
      >
        Show anyway
      </button>
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
    getAgentConversationWorkspaceCumulativeFileChanges: (...args: unknown[]) =>
      mockGetCumulativeFiles(...args),
    getAgentConversationWorkspaceStagedFileDiff: (...args: unknown[]) =>
      mockGetStagedFileDiff(...args),
    getAgentConversationWorkspaceUnstagedFileDiff: (...args: unknown[]) =>
      mockGetUnstagedFileDiff(...args),
    getAgentConversationWorkspaceCumulativeFileDiff: (...args: unknown[]) =>
      mockGetCumulativeFileDiff(...args),
  },
}));

// ── IntersectionObserver shim for jsdom ──────────────────────────────────────
// Captures callbacks so tests can simulate intersection events.
// Installed at module level so the guard `typeof IntersectionObserver === "undefined"`
// in AgentsPublishInlineDiffs doesn't skip the effect.
const ioCallbacks: IntersectionObserverCallback[] = [];
const ioObservedElements: Element[] = [];

class IOStub implements IntersectionObserver {
  readonly root: Element | null = null;
  readonly rootMargin = "200px";
  readonly thresholds: ReadonlyArray<number> = [];
  observe = vi.fn((el: Element) => {
    ioObservedElements.push(el);
  });
  unobserve = vi.fn();
  disconnect = vi.fn(() => {
    ioObservedElements.splice(0);
  });
  takeRecords = vi.fn(() => [] as IntersectionObserverEntry[]);
  constructor(cb: IntersectionObserverCallback) {
    ioCallbacks.push(cb);
  }
}

(globalThis as unknown as { IntersectionObserver: typeof IntersectionObserver }).IntersectionObserver =
  IOStub as unknown as typeof IntersectionObserver;

/** Fire an intersection event for the element at `elementIdx` in `ioObservedElements`. */
function fireIntersection(elementIdx: number, isIntersecting: boolean) {
  const cb = ioCallbacks[ioCallbacks.length - 1];
  if (!cb) throw new Error("No IntersectionObserver callback registered");
  const target = ioObservedElements[elementIdx];
  if (!target) throw new Error(`No observed element at index ${elementIdx}`);
  cb(
    [
      {
        isIntersecting,
        target,
        boundingClientRect: {} as DOMRectReadOnly,
        intersectionRatio: isIntersecting ? 1 : 0,
        intersectionRect: {} as DOMRectReadOnly,
        rootBounds: null,
        time: 0,
      } as IntersectionObserverEntry,
    ],
    {} as IntersectionObserver,
  );
}

import { AgentsPublishInlineDiffs } from "./AgentsPublishInlineDiffs";
import type { FileChange, PrDiffAnnotation } from "@/api/diff";
import type { Commit as DiffViewerCommit } from "@/components/diff";
import type { AgentWorkspaceReview } from "@/api/diff";

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

describe("AgentsPublishInlineDiffs", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    // Reset IO state between tests to avoid cross-test pollution
    ioCallbacks.splice(0);
    ioObservedElements.splice(0);
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

    mockGetUncommittedDiff.mockResolvedValue(makeHunkDiff("src/Foo.tsx"));
    mockGetCommitDiff.mockResolvedValue(makeHunkDiff("src/Foo.tsx"));
    mockGetCommitFiles.mockResolvedValue([makeFileChange("src/CommitOnly.tsx")]);
    mockGetStagedFiles.mockResolvedValue([makeFileChange("src/StagedFile.tsx")]);
    mockGetUnstagedFiles.mockResolvedValue([makeFileChange("src/UnstagedFile.tsx")]);
    mockGetCumulativeFiles.mockResolvedValue([makeFileChange("src/CumulativeFile.tsx")]);
    mockGetStagedFileDiff.mockResolvedValue(makeHunkDiff("src/StagedFile.tsx"));
    mockGetUnstagedFileDiff.mockResolvedValue(makeHunkDiff("src/UnstagedFile.tsx"));
    mockGetCumulativeFileDiff.mockResolvedValue(makeHunkDiff("src/CumulativeFile.tsx"));
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

    it("passes uncommittedCount from review.changes.length to filter", () => {
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

    it("renders a file card for each change in review.changes (uncommitted mode)", () => {
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
  });

  describe("mode=uncommitted — diff fetching", () => {
    it("fetches uncommitted diff for each expanded file", async () => {
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
      await waitFor(() =>
        expect(mockGetUncommittedDiff).toHaveBeenCalledWith("conv-42", "src/Foo.tsx"),
      );
    });

    it("passes diff data to file card after fetch", async () => {
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
  });

  describe("mode=commit — diff fetching", () => {
    it("fetches commit diff when mode switches to a commit SHA", async () => {
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
      // Uncommitted file no longer shown
      expect(screen.queryByTestId("mock-file-diff-src-Foo.tsx")).toBeNull();
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
      // File count reflects commit list (2), not uncommitted list (1)
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
      // Totals reflect commit file (7/3), not uncommitted file (99/88)
      await waitFor(() =>
        expect(screen.getByTestId("inline-diffs-additions")).toHaveTextContent("+7"),
      );
      expect(screen.getByTestId("inline-diffs-deletions")).toHaveTextContent("−3");
    });
  });

  describe("mode=staged — diff fetching", () => {
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

    it("renders staged file list (not uncommitted list) when mode is staged", async () => {
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

    it("fetches staged diff for each expanded staged file", async () => {
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
        expect(mockGetStagedFileDiff).toHaveBeenCalledWith("conv-1", "src/StagedFile.tsx"),
      );
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
  });

  describe("mode=unstaged — diff fetching", () => {
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

    it("renders unstaged file list (not uncommitted list) when mode is unstaged", async () => {
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

    it("fetches unstaged diff for each expanded unstaged file", async () => {
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
        expect(mockGetUnstagedFileDiff).toHaveBeenCalledWith("conv-1", "src/UnstagedFile.tsx"),
      );
    });
  });

  describe("mode=cumulative — diff fetching", () => {
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

    it("renders cumulative file list (not uncommitted list) when mode is cumulative", async () => {
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

    it("fetches cumulative diff for each expanded cumulative file", async () => {
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
        expect(mockGetCumulativeFileDiff).toHaveBeenCalledWith("conv-1", "src/CumulativeFile.tsx"),
      );
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
  });

  describe("openInDialog", () => {
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
    beforeEach(() => {
      // scrollIntoView is not implemented in jsdom — provide a spy
      HTMLElement.prototype.scrollIntoView = vi.fn();
    });

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

    it("calls scrollIntoView when a file is selected from the jump popover", async () => {
      const scrollIntoView = vi.fn();
      HTMLElement.prototype.scrollIntoView = scrollIntoView;
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
      await user.click(screen.getByTestId("jump-to-file-item-src/Foo.tsx"));
      expect(scrollIntoView).toHaveBeenCalled();
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

  describe("lazy hydration — IntersectionObserver", () => {
    it("initially passes shouldHydrate=false for all files", () => {
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
      expect(card).toHaveAttribute("data-should-hydrate", "false");
    });

    it("sets shouldHydrate=true when IntersectionObserver fires for a file", async () => {
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

      // IO effect has run and observed the wrapper element
      fireIntersection(0, true);

      await waitFor(() => {
        expect(screen.getByTestId("mock-file-diff-src-Foo.tsx")).toHaveAttribute(
          "data-should-hydrate",
          "true",
        );
      });
    });

    it("does NOT set shouldHydrate=true when isIntersecting=false", async () => {
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

      fireIntersection(0, false);

      // Wait a tick — state should NOT change
      await new Promise((r) => setTimeout(r, 10));
      expect(screen.getByTestId("mock-file-diff-src-Foo.tsx")).toHaveAttribute(
        "data-should-hydrate",
        "false",
      );
    });

    it("resets shouldHydrate to false when mode changes", async () => {
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

      // First fire intersection so file is hydrated
      fireIntersection(0, true);
      await waitFor(() => {
        expect(screen.getByTestId("mock-file-diff-src-Foo.tsx")).toHaveAttribute(
          "data-should-hydrate",
          "true",
        );
      });

      // Switch mode → hydratedPaths should reset
      await user.click(screen.getByRole("button", { name: "Staged" }));

      // The uncommitted file card is no longer in DOM (mode changed to staged)
      // but the reset itself can be verified: if we switch back, shouldHydrate is false again
      await user.click(screen.getByRole("button", { name: "Uncommitted" }));
      await waitFor(() => {
        // The card is re-rendered after mode switch; hydratedPaths was cleared
        expect(screen.getByTestId("mock-file-diff-src-Foo.tsx")).toHaveAttribute(
          "data-should-hydrate",
          "false",
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
