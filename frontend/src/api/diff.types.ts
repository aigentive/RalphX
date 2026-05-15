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
}
