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
    commits: unknown[];
  }) => (
    <div data-testid="mock-diff-filter" data-mode={mode} data-count={uncommittedCount}>
      <button onClick={() => onModeChange("uncommitted")}>Uncommitted</button>
      <button onClick={() => onModeChange("sha-abc")}>Commit sha-abc</button>
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
  }: {
    file: { path: string };
    diff: unknown;
    isExpanded: boolean;
    onToggle: () => void;
    onCopyPath: (p: string) => void;
    onOpenFullscreen: (p: string) => void;
    onRetry?: () => void;
  }) => (
    <div
      data-testid={`mock-file-diff-${file.path.replace(/\//g, "-")}`}
      data-expanded={String(isExpanded)}
      data-diff-status={typeof diff === "string" ? diff : diff ? "loaded" : "undefined"}
    >
      <button onClick={() => onCopyPath(file.path)}>copy</button>
      <button onClick={() => onOpenFullscreen(file.path)}>fullscreen</button>
      {file.path}
    </div>
  ),
}));

// Mock diffApi
const mockGetUncommittedDiff = vi.fn();
const mockGetCommitDiff = vi.fn();
const mockGetCommitFiles = vi.fn();

vi.mock("@/api/diff", () => ({
  diffApi: {
    getAgentConversationWorkspaceFileDiff: (...args: unknown[]) =>
      mockGetUncommittedDiff(...args),
    getAgentConversationWorkspaceCommitFileDiff: (...args: unknown[]) =>
      mockGetCommitDiff(...args),
    getAgentConversationWorkspaceCommitFileChanges: (...args: unknown[]) =>
      mockGetCommitFiles(...args),
  },
}));

import { AgentsPublishInlineDiffs } from "./AgentsPublishInlineDiffs";
import type { FileChange } from "@/api/diff";
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

describe("AgentsPublishInlineDiffs", () => {
  beforeEach(() => {
    vi.clearAllMocks();
    mockGetUncommittedDiff.mockResolvedValue({
      filePath: "src/Foo.tsx",
      oldContent: "old",
      newContent: "new",
      language: "typescript",
    });
    mockGetCommitDiff.mockResolvedValue({
      filePath: "src/Foo.tsx",
      oldContent: "old-commit",
      newContent: "new-commit",
      language: "typescript",
    });
    mockGetCommitFiles.mockResolvedValue([makeFileChange("src/CommitOnly.tsx")]);
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
});
