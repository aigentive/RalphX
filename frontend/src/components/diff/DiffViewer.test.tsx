/**
 * DiffViewer component tests
 * Split-view diff component with Changes and History tabs
 */

import { describe, it, expect, vi, beforeEach } from "vitest";
import { render, screen, fireEvent, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { DiffViewer, type FileChange, type Commit, type DiffData } from "./DiffViewer";
import type { PrDiffAnnotation, WorkspaceReviewHunkAnnotation } from "@/api/diff";

// Mock the git-diff-view library
vi.mock("@git-diff-view/react", () => ({
  DiffView: ({ data }: { data: unknown }) => (
    <div data-testid="mock-diff-view" data-diff={JSON.stringify(data)}>
      Mock DiffView
    </div>
  ),
  DiffModeEnum: {
    Unified: "unified",
    Split: "split",
  },
}));

// Test data factories
const createFileChange = (overrides: Partial<FileChange> = {}): FileChange => ({
  path: "src/components/Test.tsx",
  status: "modified",
  additions: 10,
  deletions: 5,
  ...overrides,
});

const createCommit = (overrides: Partial<Commit> = {}): Commit => ({
  sha: "abc123def456",
  shortSha: "abc123d",
  message: "feat: add new feature",
  author: "Test Author",
  date: new Date("2026-01-24T10:00:00Z"),
  ...overrides,
});

const createDiffData = (overrides: Partial<DiffData> = {}): DiffData => ({
  filePath: "src/components/Test.tsx",
  hunks: [
    {
      oldStart: 1,
      oldLines: 1,
      newStart: 1,
      newLines: 1,
      header: "@@ -1,3 +1,3 @@",
      lines: [
        { kind: "deletion", content: "const old = 'value';", oldLineNum: 1, newLineNum: null },
        { kind: "addition", content: "const new = 'value';", oldLineNum: null, newLineNum: 1 },
      ],
    },
  ],
  oldTotalLines: 1,
  newTotalLines: 1,
  isBinary: false,
  ...overrides,
});

const createAnnotation = (
  overrides: Partial<PrDiffAnnotation> = {},
): PrDiffAnnotation => ({
  id: "annotation-1",
  source: "check_run",
  path: "src/components/Test.tsx",
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

const createHunkAnnotation = (
  overrides: Partial<WorkspaceReviewHunkAnnotation> = {},
): WorkspaceReviewHunkAnnotation => ({
  id: "workspace-review-hunk-1",
  conversationId: "conversation-1",
  projectId: "project-1",
  artifactId: "artifact-1",
  artifactVersion: 1,
  targetScope: "selected_source",
  headSha: "head-sha",
  diffFingerprint: "fingerprint-1",
  path: "plan.txt",
  diffSource: "committed",
  hunkHeader: "@@ -1,3 +1,3 @@",
  oldStart: 1,
  oldLines: 1,
  newStart: 1,
  newLines: 1,
  title: "Review summary",
  message: "This commit hunk has workspace review context.",
  level: "notice",
  createdByRunId: "run-1",
  createdAt: "2026-07-01T00:00:00Z",
  ...overrides,
});

const defaultProps = {
  changes: [] as FileChange[],
  commits: [] as Commit[],
  onFetchDiff: vi.fn().mockResolvedValue(null),
};

describe("DiffViewer", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  describe("rendering", () => {
    it("renders diff viewer container with correct testid", () => {
      render(<DiffViewer {...defaultProps} />);
      expect(screen.getByTestId("diff-viewer")).toBeInTheDocument();
    });

    it("applies design system background color", () => {
      render(<DiffViewer {...defaultProps} />);
      const viewer = screen.getByTestId("diff-viewer");
      expect(viewer).toHaveClass("bg-[var(--bg-base)]");
    });

    it("renders Changes and History tabs", () => {
      render(<DiffViewer {...defaultProps} />);
      expect(screen.getByTestId("tab-changes")).toBeInTheDocument();
      expect(screen.getByTestId("tab-history")).toBeInTheDocument();
    });

    it("shows Changes tab as active by default", () => {
      render(<DiffViewer {...defaultProps} />);
      const changesTab = screen.getByTestId("tab-changes");
      expect(changesTab).toHaveAttribute("data-state", "active");
    });

    it("shows History tab as active when defaultTab is history", () => {
      render(<DiffViewer {...defaultProps} defaultTab="history" />);
      const historyTab = screen.getByTestId("tab-history");
      expect(historyTab).toHaveAttribute("data-state", "active");
    });
  });

  describe("tab bar", () => {
    it("displays file count badge on Changes tab", () => {
      const changes = [
        createFileChange({ path: "file1.ts" }),
        createFileChange({ path: "file2.ts" }),
      ];
      render(<DiffViewer {...defaultProps} changes={changes} />);
      expect(screen.getByText("2")).toBeInTheDocument();
    });

    it("displays commit count badge on History tab", () => {
      const commits = [
        createCommit({ sha: "sha1" }),
        createCommit({ sha: "sha2" }),
        createCommit({ sha: "sha3" }),
      ];
      render(<DiffViewer {...defaultProps} commits={commits} />);
      expect(screen.getByText("3")).toBeInTheDocument();
    });

    it("switches to History tab when clicked", async () => {
      const user = userEvent.setup();
      render(<DiffViewer {...defaultProps} />);

      const historyTab = screen.getByTestId("tab-history");
      await user.click(historyTab);

      expect(historyTab).toHaveAttribute("data-state", "active");
      expect(screen.getByTestId("tab-changes")).toHaveAttribute("data-state", "inactive");
    });

    it("calls onTabChange callback when tab is changed", async () => {
      const user = userEvent.setup();
      const onTabChange = vi.fn();
      render(<DiffViewer {...defaultProps} onTabChange={onTabChange} />);

      await user.click(screen.getByTestId("tab-history"));

      expect(onTabChange).toHaveBeenCalledWith("history");
    });
  });

  describe("Changes tab - empty state", () => {
    it("shows empty file tree when no changes", () => {
      render(<DiffViewer {...defaultProps} changes={[]} />);
      expect(screen.getByTestId("file-tree-empty")).toBeInTheDocument();
      expect(screen.getByText(/no uncommitted changes/i)).toBeInTheDocument();
    });

    it("shows helpful message in empty state", () => {
      render(<DiffViewer {...defaultProps} changes={[]} />);
      expect(screen.getByText(/working directory is clean/i)).toBeInTheDocument();
    });

    it("supports merged diff empty-state copy", () => {
      render(
        <DiffViewer
          {...defaultProps}
          changes={[]}
          changesLabel="Merged Diff"
          changesEmptyTitle="No merged file changes"
          changesEmptySubtitle="The merge commit did not report file changes"
        />
      );

      expect(screen.getByTestId("tab-changes")).toHaveTextContent("Merged Diff");
      expect(screen.getByText("No merged file changes")).toBeInTheDocument();
      expect(screen.getByText("The merge commit did not report file changes")).toBeInTheDocument();
    });
  });

  describe("Changes tab - file tree", () => {
    it("renders file tree with files", () => {
      const changes = [
        createFileChange({ path: "src/index.ts" }),
        createFileChange({ path: "src/app.ts" }),
      ];
      render(<DiffViewer {...defaultProps} changes={changes} />);

      expect(screen.getByTestId("file-tree")).toBeInTheDocument();
    });

    it("displays file names", () => {
      const changes = [
        createFileChange({ path: "src/components/Button.tsx" }),
      ];
      render(<DiffViewer {...defaultProps} changes={changes} />);

      expect(screen.getByText("Button.tsx")).toBeInTheDocument();
    });

    it("displays status letter for modified files", () => {
      const changes = [
        createFileChange({ path: "test.ts", status: "modified" }),
      ];
      render(<DiffViewer {...defaultProps} changes={changes} />);

      // New implementation shows status letter (M for modified)
      expect(screen.getByText("M")).toBeInTheDocument();
    });

    it("shows directory structure for nested files", () => {
      const changes = [
        createFileChange({ path: "src/components/Button.tsx" }),
      ];
      render(<DiffViewer {...defaultProps} changes={changes} />);

      // Directory should be shown
      expect(screen.getByTestId("dir-src")).toBeInTheDocument();
      expect(screen.getByTestId("dir-src/components")).toBeInTheDocument();
    });

    it("allows expanding/collapsing directories", () => {
      const changes = [
        createFileChange({ path: "src/deep/nested/file.ts" }),
      ];
      render(<DiffViewer {...defaultProps} changes={changes} />);

      // Click to collapse src directory
      fireEvent.click(screen.getByTestId("dir-src"));

      // Nested directories should be hidden
      expect(screen.queryByTestId("dir-src/deep")).not.toBeInTheDocument();

      // Click to expand again
      fireEvent.click(screen.getByTestId("dir-src"));
      expect(screen.getByTestId("dir-src/deep")).toBeInTheDocument();
    });

    it("calls onFetchDiff when file is selected", async () => {
      const onFetchDiff = vi.fn().mockResolvedValue(createDiffData());
      const changes = [createFileChange({ path: "test.ts" })];

      render(<DiffViewer {...defaultProps} changes={changes} onFetchDiff={onFetchDiff} />);

      fireEvent.click(screen.getByTestId("file-test.ts"));

      await waitFor(() => {
        expect(onFetchDiff).toHaveBeenCalledWith("test.ts");
      });
    });
  });

  describe("Changes tab - file status icons", () => {
    it("shows added file icon for new files", () => {
      const changes = [createFileChange({ path: "new.ts", status: "added" })];
      render(<DiffViewer {...defaultProps} changes={changes} />);

      const fileButton = screen.getByTestId("file-new.ts");
      expect(fileButton).toBeInTheDocument();
    });

    it("shows modified file icon for changed files", () => {
      const changes = [createFileChange({ path: "modified.ts", status: "modified" })];
      render(<DiffViewer {...defaultProps} changes={changes} />);

      const fileButton = screen.getByTestId("file-modified.ts");
      expect(fileButton).toBeInTheDocument();
    });

    it("shows deleted file icon for removed files", () => {
      const changes = [createFileChange({ path: "deleted.ts", status: "deleted" })];
      render(<DiffViewer {...defaultProps} changes={changes} />);

      const fileButton = screen.getByTestId("file-deleted.ts");
      expect(fileButton).toBeInTheDocument();
    });

    it("shows renamed file icon for renamed files", () => {
      const changes = [createFileChange({ path: "renamed.ts", status: "renamed" })];
      render(<DiffViewer {...defaultProps} changes={changes} />);

      const fileButton = screen.getByTestId("file-renamed.ts");
      expect(fileButton).toBeInTheDocument();
    });
  });

  describe("Changes tab - diff panel", () => {
    it("shows empty state when no file selected", () => {
      render(<DiffViewer {...defaultProps} changes={[]} />);
      expect(screen.getByTestId("diff-empty")).toBeInTheDocument();
    });

    it("shows loading state while fetching diff", async () => {
      const onFetchDiff = vi.fn().mockImplementation(() => new Promise(() => {})); // Never resolves
      const changes = [createFileChange({ path: "test.ts" })];

      render(<DiffViewer {...defaultProps} changes={changes} onFetchDiff={onFetchDiff} />);

      // Auto-selects first file, triggering loading
      await waitFor(() => {
        expect(screen.getByTestId("diff-loading")).toBeInTheDocument();
      });
    });

    it("shows diff content when loaded", async () => {
      const diffData = createDiffData();
      const onFetchDiff = vi.fn().mockResolvedValue(diffData);
      const changes = [createFileChange({ path: "test.ts" })];

      render(<DiffViewer {...defaultProps} changes={changes} onFetchDiff={onFetchDiff} />);

      await waitFor(() => {
        expect(screen.getByTestId("diff-content")).toBeInTheDocument();
      });
    });

    it("renders inline diff content with fetched data", async () => {
      const diffData = createDiffData({ filePath: "src/app.ts" });
      const onFetchDiff = vi.fn().mockResolvedValue(diffData);
      const changes = [createFileChange({ path: "src/app.ts" })];

      render(<DiffViewer {...defaultProps} changes={changes} onFetchDiff={onFetchDiff} />);

      await waitFor(() => {
        expect(screen.getByTestId("diff-content")).toBeInTheDocument();
        expect(screen.getByText("const old = 'value';")).toBeInTheDocument();
        expect(screen.getByText("const new = 'value';")).toBeInTheDocument();
      });
    });

    it("renders matching PR annotations for the selected changes file", async () => {
      const diffData = createDiffData({ filePath: "src/annotated.ts" });
      const onFetchDiff = vi.fn().mockResolvedValue(diffData);
      const changes = [createFileChange({ path: "src/annotated.ts" })];

      render(
        <DiffViewer
          {...defaultProps}
          changes={changes}
          onFetchDiff={onFetchDiff}
          annotations={[
            createAnnotation({ path: "src/annotated.ts" }),
            createAnnotation({
              id: "annotation-other",
              path: "src/other.ts",
              message: "Other file annotation.",
            }),
          ]}
        />
      );

      await waitFor(() => {
        expect(screen.getByTestId("diff-annotation-row")).toBeInTheDocument();
      });
      expect(screen.getByText("CodeQL warning")).toBeInTheDocument();
      expect(screen.queryByText("Other file annotation.")).not.toBeInTheDocument();
    });

    it("renders staged workspace review hunk annotations for staged changes", async () => {
      const diffData = createDiffData({ filePath: "plan.txt" });
      const onFetchDiff = vi.fn().mockResolvedValue(diffData);
      const changes = [createFileChange({ path: "plan.txt" })];

      render(
        <DiffViewer
          {...defaultProps}
          changes={changes}
          changesRefKind={{ kind: "staged" }}
          onFetchDiff={onFetchDiff}
          hunkAnnotations={[
            createHunkAnnotation({
              diffSource: "staged",
              title: "Staged review summary",
              message: "This staged hunk has workspace review context.",
            }),
            createHunkAnnotation({
              id: "workspace-review-hunk-unstaged",
              diffSource: "unstaged",
              title: "Unstaged review summary",
              message: "This unstaged hunk should not render in staged mode.",
            }),
          ]}
        />
      );

      expect(await screen.findByTestId("diff-hunk-annotation-row")).toBeInTheDocument();
      expect(screen.getByText("Staged review summary")).toBeInTheDocument();
      expect(screen.queryByText("Unstaged review summary")).not.toBeInTheDocument();
    });

    it("shows error state when diff fetch fails", async () => {
      const onFetchDiff = vi.fn().mockResolvedValue(null);
      const changes = [createFileChange({ path: "test.ts" })];

      render(<DiffViewer {...defaultProps} changes={changes} onFetchDiff={onFetchDiff} />);

      await waitFor(() => {
        expect(screen.getByTestId("diff-error")).toBeInTheDocument();
      });
    });

    it("displays file path in diff panel header", async () => {
      const diffData = createDiffData({ filePath: "src/components/Button.tsx" });
      const onFetchDiff = vi.fn().mockResolvedValue(diffData);
      const changes = [createFileChange({ path: "src/components/Button.tsx" })];

      render(<DiffViewer {...defaultProps} changes={changes} onFetchDiff={onFetchDiff} />);

      await waitFor(() => {
        // New implementation shows full file path
        expect(screen.getByText("src/components/Button.tsx")).toBeInTheDocument();
      });
    });
  });

  describe("Changes tab - Open in IDE button", () => {
    it("renders Open in IDE button when onOpenInIDE provided", async () => {
      const onOpenInIDE = vi.fn();
      const diffData = createDiffData();
      const onFetchDiff = vi.fn().mockResolvedValue(diffData);
      const changes = [createFileChange({ path: "test.ts" })];

      render(
        <DiffViewer
          {...defaultProps}
          changes={changes}
          onFetchDiff={onFetchDiff}
          onOpenInIDE={onOpenInIDE}
        />
      );

      await waitFor(() => {
        expect(screen.getByTestId("open-in-ide")).toBeInTheDocument();
      });
    });

    it("calls onOpenInIDE with file path when clicked", async () => {
      const onOpenInIDE = vi.fn();
      const diffData = createDiffData({ filePath: "src/app.ts" });
      const onFetchDiff = vi.fn().mockResolvedValue(diffData);
      const changes = [createFileChange({ path: "src/app.ts" })];

      render(
        <DiffViewer
          {...defaultProps}
          changes={changes}
          onFetchDiff={onFetchDiff}
          onOpenInIDE={onOpenInIDE}
        />
      );

      await waitFor(() => {
        const button = screen.getByTestId("open-in-ide");
        fireEvent.click(button);
      });

      expect(onOpenInIDE).toHaveBeenCalledWith("src/app.ts");
    });

    it("does not render Open in IDE button when onOpenInIDE not provided", async () => {
      const diffData = createDiffData();
      const onFetchDiff = vi.fn().mockResolvedValue(diffData);
      const changes = [createFileChange({ path: "test.ts" })];

      render(<DiffViewer {...defaultProps} changes={changes} onFetchDiff={onFetchDiff} />);

      await waitFor(() => {
        expect(screen.getByTestId("diff-content")).toBeInTheDocument();
      });

      expect(screen.queryByTestId("open-in-ide")).not.toBeInTheDocument();
    });
  });

  describe("History tab - empty state", () => {
    it("shows empty commit list when no commits", () => {
      render(<DiffViewer {...defaultProps} defaultTab="history" commits={[]} />);
      expect(screen.getByTestId("commit-list-empty")).toBeInTheDocument();
      expect(screen.getByText(/no commit history/i)).toBeInTheDocument();
    });

    it("shows helpful message in empty state", () => {
      render(<DiffViewer {...defaultProps} defaultTab="history" commits={[]} />);
      expect(screen.getByText(/Make your first commit to see history here/i)).toBeInTheDocument();
    });
  });

  describe("History tab - commit list", () => {
    it("renders commit list", () => {
      const commits = [createCommit()];
      render(<DiffViewer {...defaultProps} defaultTab="history" commits={commits} />);
      expect(screen.getByTestId("commit-list")).toBeInTheDocument();
    });

    it("displays commit messages", () => {
      const commits = [
        createCommit({ message: "feat: add new feature" }),
        createCommit({ sha: "def456", message: "fix: resolve bug" }),
      ];
      render(<DiffViewer {...defaultProps} defaultTab="history" commits={commits} />);

      expect(screen.getByText("feat: add new feature")).toBeInTheDocument();
      expect(screen.getByText("fix: resolve bug")).toBeInTheDocument();
    });

    it("displays commit short SHA", () => {
      const commits = [createCommit({ shortSha: "abc1234" })];
      render(<DiffViewer {...defaultProps} defaultTab="history" commits={commits} />);
      expect(screen.getByText("abc1234")).toBeInTheDocument();
    });

    it("displays commit author", () => {
      const commits = [createCommit({ author: "John Doe" })];
      render(<DiffViewer {...defaultProps} defaultTab="history" commits={commits} />);
      expect(screen.getByText(/John Doe/)).toBeInTheDocument();
    });

    it("calls onCommitSelect when commit is clicked", () => {
      const onCommitSelect = vi.fn();
      const commit = createCommit({ shortSha: "abc1234" });

      render(
        <DiffViewer
          {...defaultProps}
          defaultTab="history"
          commits={[commit]}
          onCommitSelect={onCommitSelect}
        />
      );

      fireEvent.click(screen.getByTestId("commit-abc1234"));

      expect(onCommitSelect).toHaveBeenCalledWith(commit);
    });
  });

  describe("History tab - commit diff panel", () => {
    it("shows empty state when no commit selected", () => {
      const commits = [createCommit()];
      render(<DiffViewer {...defaultProps} defaultTab="history" commits={commits} />);
      expect(screen.getByTestId("commit-diff-empty")).toBeInTheDocument();
    });

    it("shows helpful message when commit selected", () => {
      const commits = [createCommit()];
      render(<DiffViewer {...defaultProps} defaultTab="history" commits={commits} />);
      expect(screen.getByText(/Select a commit to view changes/i)).toBeInTheDocument();
    });

    it("auto-selects the first commit when requested", async () => {
      const commit = createCommit({ sha: "sha-1", shortSha: "sha-1" });
      const onCommitSelect = vi.fn();
      const onFetchCommitFiles = vi.fn().mockResolvedValue(undefined);

      render(
        <DiffViewer
          {...defaultProps}
          defaultTab="history"
          commits={[commit]}
          autoSelectFirstCommit
          onCommitSelect={onCommitSelect}
          onFetchCommitFiles={onFetchCommitFiles}
        />
      );

      await waitFor(() => {
        expect(onCommitSelect).toHaveBeenCalledWith(commit);
      });
      expect(onFetchCommitFiles).toHaveBeenCalledWith("sha-1");
      expect(screen.queryByText(/Select a commit to view changes/i)).not.toBeInTheDocument();
    });

    it("auto-selects the first changed file for the selected commit when requested", async () => {
      const commit = createCommit({ sha: "sha-1", shortSha: "sha-1" });
      const file = createFileChange({ path: "plan.txt", status: "added" });
      const onFetchDiff = vi.fn().mockResolvedValue(createDiffData({ filePath: "plan.txt" }));

      render(
        <DiffViewer
          {...defaultProps}
          defaultTab="history"
          commits={[commit]}
          commitFiles={[file]}
          autoSelectFirstCommit
          autoSelectFirstCommitFile
          onFetchDiff={onFetchDiff}
        />
      );

      await waitFor(() => {
        expect(onFetchDiff).toHaveBeenCalledWith("plan.txt", "sha-1");
      });
    });

    it("passes committed workspace review hunk annotations to selected History files", async () => {
      const commit = createCommit({ sha: "sha-1", shortSha: "sha-1" });
      const file = createFileChange({ path: "plan.txt", status: "added" });
      const onFetchDiff = vi.fn().mockResolvedValue(createDiffData({ filePath: "plan.txt" }));

      render(
        <DiffViewer
          {...defaultProps}
          defaultTab="history"
          commits={[commit]}
          commitFiles={[file]}
          autoSelectFirstCommit
          autoSelectFirstCommitFile
          onFetchDiff={onFetchDiff}
          hunkAnnotations={[createHunkAnnotation()]}
        />
      );

      expect(await screen.findByTestId("diff-hunk-annotation-row")).toBeInTheDocument();
      expect(screen.getByText("Workspace review")).toBeInTheDocument();
      expect(screen.getByText("Review summary")).toBeInTheDocument();
    });
  });

  describe("loading states", () => {
    it("shows loading skeleton when isLoadingChanges is true", () => {
      render(<DiffViewer {...defaultProps} isLoadingChanges={true} />);
      // Skeleton loading should be visible (uses animate-pulse)
      const skeletons = screen.getByTestId("diff-viewer").querySelectorAll(".animate-pulse");
      expect(skeletons.length).toBeGreaterThan(0);
    });

    it("shows loading skeleton when isLoadingHistory is true", () => {
      render(<DiffViewer {...defaultProps} defaultTab="history" isLoadingHistory={true} />);
      // Skeleton loading should be visible (uses animate-pulse)
      const skeletons = screen.getByTestId("diff-viewer").querySelectorAll(".animate-pulse");
      expect(skeletons.length).toBeGreaterThan(0);
    });
  });

  describe("auto-selection", () => {
    it("auto-selects first file when changes tab is active", async () => {
      const onFetchDiff = vi.fn().mockResolvedValue(createDiffData());
      const changes = [
        createFileChange({ path: "first.ts" }),
        createFileChange({ path: "second.ts" }),
      ];

      render(<DiffViewer {...defaultProps} changes={changes} onFetchDiff={onFetchDiff} />);

      await waitFor(() => {
        expect(onFetchDiff).toHaveBeenCalledWith("first.ts");
      });
    });
  });

  describe("tab switching behavior", () => {
    it("resets selection when switching tabs", async () => {
      const user = userEvent.setup();
      const onFetchDiff = vi.fn().mockResolvedValue(createDiffData());
      const changes = [createFileChange({ path: "test.ts" })];
      const commits = [createCommit()];

      render(
        <DiffViewer
          {...defaultProps}
          changes={changes}
          commits={commits}
          onFetchDiff={onFetchDiff}
        />
      );

      // Wait for initial file selection
      await waitFor(() => {
        expect(onFetchDiff).toHaveBeenCalled();
      });

      // Switch to history tab
      await user.click(screen.getByTestId("tab-history"));

      // Should show empty commit diff state
      expect(screen.getByTestId("commit-diff-empty")).toBeInTheDocument();

      // Switch back to changes tab
      await user.click(screen.getByTestId("tab-changes"));

      // onFetchDiff should be called again for auto-selection
      await waitFor(() => {
        expect(onFetchDiff).toHaveBeenCalledTimes(2);
      });
    });
  });

  describe("file tree sorting", () => {
    it("sorts directories before files", () => {
      const changes = [
        createFileChange({ path: "zebra.ts" }),
        createFileChange({ path: "src/alpha.ts" }),
        createFileChange({ path: "beta.ts" }),
      ];

      render(<DiffViewer {...defaultProps} changes={changes} />);

      const fileTree = screen.getByTestId("file-tree");
      const buttons = fileTree.querySelectorAll("button");

      // First button should be the 'src' directory
      expect(buttons[0]).toHaveAttribute("data-testid", "dir-src");
    });

    it("sorts files alphabetically within directories", () => {
      const changes = [
        createFileChange({ path: "c.ts" }),
        createFileChange({ path: "a.ts" }),
        createFileChange({ path: "b.ts" }),
      ];

      render(<DiffViewer {...defaultProps} changes={changes} />);

      const fileTree = screen.getByTestId("file-tree");
      const fileButtons = fileTree.querySelectorAll('button[data-testid^="file-"]');

      expect(fileButtons[0]).toHaveAttribute("data-testid", "file-a.ts");
      expect(fileButtons[1]).toHaveAttribute("data-testid", "file-b.ts");
      expect(fileButtons[2]).toHaveAttribute("data-testid", "file-c.ts");
    });
  });

  describe("relative date formatting", () => {
    it("shows 'just now' for very recent commits", () => {
      const commits = [
        createCommit({ date: new Date() }),
      ];
      render(<DiffViewer {...defaultProps} defaultTab="history" commits={commits} />);
      expect(screen.getByText(/just now/)).toBeInTheDocument();
    });

    it("shows minutes ago for recent commits", () => {
      const fiveMinutesAgo = new Date(Date.now() - 5 * 60 * 1000);
      const commits = [createCommit({ date: fiveMinutesAgo })];
      render(<DiffViewer {...defaultProps} defaultTab="history" commits={commits} />);
      expect(screen.getByText(/5m ago/)).toBeInTheDocument();
    });

    it("shows hours ago for older commits", () => {
      const threeHoursAgo = new Date(Date.now() - 3 * 60 * 60 * 1000);
      const commits = [createCommit({ date: threeHoursAgo })];
      render(<DiffViewer {...defaultProps} defaultTab="history" commits={commits} />);
      expect(screen.getByText(/3h ago/)).toBeInTheDocument();
    });

    it("shows days ago for older commits", () => {
      const twoDaysAgo = new Date(Date.now() - 2 * 24 * 60 * 60 * 1000);
      const commits = [createCommit({ date: twoDaysAgo })];
      render(<DiffViewer {...defaultProps} defaultTab="history" commits={commits} />);
      expect(screen.getByText(/2d ago/)).toBeInTheDocument();
    });
  });

  describe("accessibility", () => {
    it("tabs have correct role", () => {
      render(<DiffViewer {...defaultProps} />);
      expect(screen.getByTestId("tab-changes")).toHaveAttribute("role", "tab");
      expect(screen.getByTestId("tab-history")).toHaveAttribute("role", "tab");
    });

    it("active tab has data-state active", () => {
      render(<DiffViewer {...defaultProps} />);
      expect(screen.getByTestId("tab-changes")).toHaveAttribute("data-state", "active");
      expect(screen.getByTestId("tab-history")).toHaveAttribute("data-state", "inactive");
    });
  });

  describe("conversationId / refKind plumbing", () => {
    // Diff with a trailing gap: hunk covers lines 1-2, file has 50 lines → 48 unchanged trailing
    const diffWithGap = createDiffData({
      hunks: [
        {
          oldStart: 1,
          oldLines: 2,
          newStart: 1,
          newLines: 2,
          header: "@@ -1,2 +1,2 @@",
          lines: [
            { kind: "deletion", content: "old", oldLineNum: 1, newLineNum: null },
            { kind: "addition", content: "new", oldLineNum: null, newLineNum: 1 },
            { kind: "context", content: "same", oldLineNum: 2, newLineNum: 2 },
          ],
        },
      ],
      newTotalLines: 50, // trailing gap = 50 - 1 - 2 + 1 = 48 lines
    });

    it("shows clickable 'Show N unchanged lines' button when conversationId is provided", async () => {
      const onFetchDiff = vi.fn().mockResolvedValue(diffWithGap);
      const changes = [createFileChange({ path: "src/test.ts" })];

      render(
        <DiffViewer
          {...defaultProps}
          changes={changes}
          onFetchDiff={onFetchDiff}
          conversationId="conv-123"
        />
      );

      await waitFor(() => {
        expect(screen.getByTestId("diff-content")).toBeInTheDocument();
      });

      // Gap should render as a clickable button (lazy fetch enabled)
      expect(
        screen.getByRole("button", { name: /Show 48 unchanged lines/i })
      ).toBeInTheDocument();
    });

    it("shows static 'N unchanged lines' label when no conversationId", async () => {
      const onFetchDiff = vi.fn().mockResolvedValue(diffWithGap);
      const changes = [createFileChange({ path: "src/test.ts" })];

      render(
        <DiffViewer
          {...defaultProps}
          changes={changes}
          onFetchDiff={onFetchDiff}
          // no conversationId — falls back to static label
        />
      );

      await waitFor(() => {
        expect(screen.getByTestId("diff-content")).toBeInTheDocument();
      });

      // Gap renders as static text (no fetch possible)
      expect(screen.getByText("48 unchanged lines")).toBeInTheDocument();
    });
  });
});
