// TypeScript types for diff API (camelCase)

export type FileChangeStatus = "added" | "modified" | "deleted";

export interface FileChange {
  path: string;
  status: FileChangeStatus;
  additions: number;
  deletions: number;
  isGenerated: boolean;
}

// ── Hunk-based diff types ─────────────────────────────────────────────────

export type DiffLineKind = "context" | "addition" | "deletion";

export interface DiffLine {
  kind: DiffLineKind;
  content: string;
  oldLineNum: number | null;
  newLineNum: number | null;
}

export interface DiffHunk {
  oldStart: number;
  oldLines: number;
  newStart: number;
  newLines: number;
  header: string;
  lines: DiffLine[];
}

/** Tagged union matching the backend DiffRefKind enum. */
export type DiffRefKind =
  | { kind: "head" }
  | { kind: "staged" }
  | { kind: "unstaged" }
  | { kind: "commit"; sha: string }
  | { kind: "cumulative_base" }
  | { kind: "cumulative_head" };

export interface RangeLine {
  lineNum: number;
  content: string;
}

export interface FileDiff {
  filePath: string;
  language: string;
  hunks: DiffHunk[];
  oldTotalLines: number;
  newTotalLines: number;
  isBinary: boolean;
}

export interface ConflictDiff {
  filePath: string;
  baseContent: string;
  oursContent: string;
  theirsContent: string;
  mergedWithMarkers: string;
  language: string;
}

export type DiffPageRow =
  | {
      kind: "hunk_header";
      header: string;
      oldStart: number;
      oldLines: number;
      newStart: number;
      newLines: number;
    }
  | { kind: "line"; line: DiffLine };

export interface FileDiffPage {
  filePath: string;
  language: string;
  rows: DiffPageRow[];
  offset: number;
  limit: number;
  nextOffset: number | null;
  totalRows: number;
  oldTotalLines: number;
  newTotalLines: number;
  isBinary: boolean;
}

// ── GitHub PR annotation types ─────────────────────────────────────────────

export interface PrDiffAnnotation {
  id: string;
  source: string;
  path: string | null;
  side: string | null;
  startLine: number | null;
  endLine: number | null;
  startColumn: number | null;
  endColumn: number | null;
  level: string;
  status: string | null;
  title: string | null;
  message: string;
  author: string | null;
  checkName: string | null;
  url: string | null;
  isOutdated: boolean;
  createdAt: string | null;
}

export interface PrAnnotationSourceUnavailable {
  source: string;
  reason: string;
}

export interface PrDiffAnnotationsResponse {
  prNumber: number;
  headSha: string | null;
  annotations: PrDiffAnnotation[];
  sourcesUnavailable: PrAnnotationSourceUnavailable[];
}

// ── Workspace review hunk annotation types ────────────────────────────────

export interface WorkspaceReviewHunkAnnotation {
  id: string;
  conversationId: string;
  projectId: string;
  artifactId: string;
  artifactVersion: number;
  targetScope: "selected_source" | "workspace_delta";
  headSha: string | null;
  diffFingerprint: string;
  path: string;
  diffSource: string;
  hunkHeader: string;
  oldStart: number;
  oldLines: number;
  newStart: number;
  newLines: number;
  title: string | null;
  message: string;
  level: string;
  createdByRunId: string | null;
  createdAt: string;
}

export interface WorkspaceReviewHunkAnnotationsResponse {
  artifactId: string | null;
  artifactVersion: number | null;
  targetScope: string | null;
  headSha: string | null;
  diffFingerprint: string | null;
  annotations: WorkspaceReviewHunkAnnotation[];
}

// ── Other domain types ────────────────────────────────────────────────────

export interface CommitInfo {
  sha: string;
  shortSha: string;
  message: string;
  author: string;
  date: Date;
}

export interface AgentWorkspaceReview {
  changes: FileChange[];
  commits: CommitInfo[];
  baseRef: string;
  headRef: string;
  supportsWorktreeModes?: boolean;
  snapshotCapturedAt?: string;
  snapshotCacheVersion?: string;
  snapshotContextSource?: AgentWorkspaceContextSource;
}

export type AgentWorkspaceContextSource =
  | "worktree"
  | "local_branch"
  | "plan_branch"
  | "pull_request_head"
  | "github_patch"
  | "terminal_pull_request_head"
  | "repair_worktree";

export interface AgentWorkspaceChangeBucketSummary {
  fileCount: number;
  additions: number;
  deletions: number;
}

export interface AgentWorkspaceConflictSummary {
  fileCount: number;
  files: string[];
}

export interface AgentWorkspaceRepairState {
  expectedBranch: string;
  checkedOutBranch: string;
  rebaseInProgress: boolean;
  mergeInProgress: boolean;
}

export interface AgentWorkspaceChangeSummary {
  supportsWorktreeModes: boolean;
  staged: AgentWorkspaceChangeBucketSummary;
  unstaged: AgentWorkspaceChangeBucketSummary;
  conflicted?: AgentWorkspaceConflictSummary;
  repairState?: AgentWorkspaceRepairState;
  snapshotCapturedAt?: string;
  snapshotCacheVersion?: string;
  snapshotContextSource?: AgentWorkspaceContextSource;
}
