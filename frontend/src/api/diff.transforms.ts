// Transform functions for diff API (snake_case -> camelCase)

import type { z } from "zod";
import type {
  AgentWorkspaceReviewResponseSchema,
  FileChangeSchema,
  FileDiffSchema,
  CommitInfoSchema,
} from "./diff.schemas";
import type {
  AgentWorkspaceReview,
  FileChange,
  FileDiff,
  CommitInfo,
} from "./diff.types";

type RawFileChange = z.infer<typeof FileChangeSchema>;
type RawFileDiff = z.infer<typeof FileDiffSchema>;
type RawCommitInfo = z.infer<typeof CommitInfoSchema>;
type RawAgentWorkspaceReview = z.infer<typeof AgentWorkspaceReviewResponseSchema>;

export function transformFileChange(raw: RawFileChange): FileChange {
  return {
    path: raw.path,
    status: raw.status,
    additions: raw.additions,
    deletions: raw.deletions,
  };
}

export function transformFileDiff(raw: RawFileDiff): FileDiff {
  return {
    filePath: raw.file_path,
    oldContent: raw.old_content,
    newContent: raw.new_content,
    language: raw.language,
  };
}

export function transformCommitInfo(raw: RawCommitInfo): CommitInfo {
  return {
    sha: raw.sha,
    shortSha: raw.short_sha,
    message: raw.message,
    author: raw.author,
    date: new Date(raw.timestamp),
  };
}

export function transformAgentWorkspaceReview(
  raw: RawAgentWorkspaceReview
): AgentWorkspaceReview {
  return {
    changes: raw.changes.map(transformFileChange),
    commits: raw.commits.map(transformCommitInfo),
    baseRef: raw.base_ref,
    headRef: raw.head_ref,
  };
}
