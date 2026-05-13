// TypeScript types for diff API (camelCase)

export type FileChangeStatus = "added" | "modified" | "deleted";

export interface FileChange {
  path: string;
  status: FileChangeStatus;
  additions: number;
  deletions: number;
}

export interface FileDiff {
  filePath: string;
  oldContent: string;
  newContent: string;
  language: string;
}

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
