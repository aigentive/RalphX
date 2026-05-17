/**
 * DiffViewer types and utility functions
 *
 * Extracted from DiffViewer.tsx for reuse and to reduce file size.
 */

import {
  File,
  FileCode,
  FileJson,
} from "lucide-react";

// ============================================================================
// Types
// ============================================================================

export type DiffViewTab = "changes" | "history";

export interface FileChange {
  /** File path relative to repository root */
  path: string;
  /** Change status: added, modified, deleted, renamed */
  status: "added" | "modified" | "deleted" | "renamed";
  /** Number of additions */
  additions: number;
  /** Number of deletions */
  deletions: number;
  /** Old path for renamed files */
  oldPath?: string;
}

export interface Commit {
  /** Commit SHA */
  sha: string;
  /** Short SHA (first 7 characters) */
  shortSha: string;
  /** Commit message subject line */
  message: string;
  /** Author name */
  author: string;
  /** Commit date */
  date: Date;
}

export interface DiffData {
  /** File path */
  filePath: string;
  /** Parsed diff hunks (from server) */
  hunks: import("@/api/diff").DiffHunk[];
  /** Total lines in old version */
  oldTotalLines: number;
  /** Total lines in new version */
  newTotalLines: number;
  /** Whether this is a binary file */
  isBinary: boolean;
  /** File language for syntax highlighting */
  language?: string;
}

export interface DiffViewerProps {
  /** Current changes (uncommitted) */
  changes: FileChange[];
  /** Commit history */
  commits: Commit[];
  /** Files changed in selected commit */
  commitFiles?: FileChange[];
  /** Callback to fetch diff for a file */
  onFetchDiff: (filePath: string, commitSha?: string) => Promise<DiffData | null>;
  /** GitHub PR review/check annotations for the current workspace diff. */
  annotations?: import("@/api/diff").PrDiffAnnotation[];
  /** Callback to fetch files changed in a specific commit */
  onFetchCommitFiles?: (commitSha: string) => Promise<void>;
  /** Callback to open file in IDE */
  onOpenInIDE?: (filePath: string) => void;
  /** Loading state for changes */
  isLoadingChanges?: boolean;
  /** Loading state for history */
  isLoadingHistory?: boolean;
  /** Loading state for commit files */
  isLoadingCommitFiles?: boolean;
  /** Currently active tab */
  defaultTab?: DiffViewTab;
  /** Label for the changes tab. */
  changesLabel?: string;
  /** Empty-state title for the changes tree. */
  changesEmptyTitle?: string;
  /** Empty-state subtitle for the changes tree. */
  changesEmptySubtitle?: string;
  /** Select the first commit when opening the History tab. */
  autoSelectFirstCommit?: boolean;
  /** Select the first changed file after a History commit is selected. */
  autoSelectFirstCommitFile?: boolean;
  /** Callback when tab changes */
  onTabChange?: (tab: DiffViewTab) => void;
  /** Callback when commit is selected */
  onCommitSelect?: (commit: Commit) => void;
  /**
   * Conversation ID for workspace range fetches.
   * When provided, "Show N unchanged lines" gaps in SimpleDiffView will
   * lazily fetch context lines from the backend.
   */
  conversationId?: string;
  /**
   * refKind for the Changes tab. Defaults to `{ kind: "head" }` when absent.
   * Pass a staged/unstaged/cumulative_head variant if the changes tab is
   * scoped to a specific ref.
   */
  changesRefKind?: import("@/api/diff").DiffRefKind;
}

// ============================================================================
// File Tree Types
// ============================================================================

export interface DirectoryNode {
  name: string;
  path: string;
  isDirectory: true;
  children: (DirectoryNode | FileNode)[];
  expanded: boolean;
}

export interface FileNode {
  name: string;
  path: string;
  isDirectory: false;
  file: FileChange;
}

export type TreeNode = DirectoryNode | FileNode;

// ============================================================================
// Utility Functions
// ============================================================================

export function getStatusColor(status: FileChange["status"]) {
  switch (status) {
    case "added":
      return "var(--status-success)";
    case "modified":
      return "var(--status-warning)";
    case "deleted":
      return "var(--status-error)";
    case "renamed":
      return "var(--status-info)";
  }
}

export function getStatusLetter(status: FileChange["status"]) {
  switch (status) {
    case "added":
      return "A";
    case "modified":
      return "M";
    case "deleted":
      return "D";
    case "renamed":
      return "R";
  }
}

export function formatRelativeDate(date: Date): string {
  const now = new Date();
  const diffMs = now.getTime() - date.getTime();
  const diffMins = Math.floor(diffMs / 60000);
  const diffHours = Math.floor(diffMs / 3600000);
  const diffDays = Math.floor(diffMs / 86400000);

  if (diffMins < 1) return "just now";
  if (diffMins < 60) return `${diffMins}m ago`;
  if (diffHours < 24) return `${diffHours}h ago`;
  if (diffDays < 7) return `${diffDays}d ago`;
  return date.toLocaleDateString();
}

export function getLanguageFromPath(path: string): string {
  const ext = path.split(".").pop()?.toLowerCase() ?? "";
  const langMap: Record<string, string> = {
    ts: "typescript",
    tsx: "typescript",
    js: "javascript",
    jsx: "javascript",
    rs: "rust",
    py: "python",
    go: "go",
    java: "java",
    c: "c",
    cpp: "cpp",
    h: "c",
    hpp: "cpp",
    cs: "csharp",
    rb: "ruby",
    php: "php",
    swift: "swift",
    kt: "kotlin",
    md: "markdown",
    json: "json",
    yaml: "yaml",
    yml: "yaml",
    toml: "toml",
    html: "html",
    css: "css",
    scss: "scss",
    sql: "sql",
    sh: "bash",
    bash: "bash",
    zsh: "bash",
  };
  return langMap[ext] ?? "plaintext";
}

export function getFileIcon(path: string) {
  const ext = path.split(".").pop()?.toLowerCase() ?? "";
  const codeExts = ["ts", "tsx", "js", "jsx", "rs", "py", "go", "java", "c", "cpp", "rb", "php", "swift", "kt"];
  const configExts = ["json", "yaml", "yml", "toml"];

  if (codeExts.includes(ext)) {
    return <FileCode className="w-4 h-4" />;
  }
  if (configExts.includes(ext)) {
    return <FileJson className="w-4 h-4" />;
  }
  return <File className="w-4 h-4" />;
}

// ============================================================================
// File Tree Building
// ============================================================================

export function buildFileTree(files: FileChange[]): TreeNode[] {
  const root: DirectoryNode = {
    name: "",
    path: "",
    isDirectory: true,
    children: [],
    expanded: true,
  };

  for (const file of files) {
    const parts = file.path.split("/");
    let current = root;

    for (let i = 0; i < parts.length; i++) {
      const part = parts[i];
      if (part === undefined) continue;

      const isFile = i === parts.length - 1;
      const pathSoFar = parts.slice(0, i + 1).join("/");

      if (isFile) {
        current.children.push({
          name: part,
          path: file.path,
          isDirectory: false,
          file,
        });
      } else {
        let dirNode = current.children.find(
          (c): c is DirectoryNode => c.isDirectory && c.name === part
        );
        if (!dirNode) {
          dirNode = {
            name: part,
            path: pathSoFar,
            isDirectory: true,
            children: [],
            expanded: true,
          };
          current.children.push(dirNode);
        }
        current = dirNode;
      }
    }
  }

  // Sort: directories first, then files, both alphabetically
  function sortChildren(node: DirectoryNode): void {
    node.children.sort((a, b) => {
      if (a.isDirectory && !b.isDirectory) return -1;
      if (!a.isDirectory && b.isDirectory) return 1;
      return a.name.localeCompare(b.name);
    });
    for (const child of node.children) {
      if (child.isDirectory) {
        sortChildren(child);
      }
    }
  }
  sortChildren(root);

  return root.children;
}
