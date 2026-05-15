// Transform functions for diff API (snake_case -> camelCase)

import type { z } from "zod";
import type {
  AgentWorkspaceReviewResponseSchema,
  FileChangeSchema,
  FileDiffSchema,
  DiffLineSchema,
  DiffHunkSchema,
  CommitInfoSchema,
  RangeLineSchema,
} from "./diff.schemas";
import type {
  AgentWorkspaceReview,
  FileChange,
  FileDiff,
  DiffLine,
  DiffHunk,
  CommitInfo,
  RangeLine,
} from "./diff.types";

type RawFileChange = z.infer<typeof FileChangeSchema>;
type RawFileDiff = z.infer<typeof FileDiffSchema>;
type RawDiffLine = z.infer<typeof DiffLineSchema>;
type RawDiffHunk = z.infer<typeof DiffHunkSchema>;
type RawCommitInfo = z.infer<typeof CommitInfoSchema>;
type RawRangeLine = z.infer<typeof RangeLineSchema>;
type RawAgentWorkspaceReview = z.infer<typeof AgentWorkspaceReviewResponseSchema>;

export function transformFileChange(raw: RawFileChange): FileChange {
  return {
    path: raw.path,
    status: raw.status,
    additions: raw.additions,
    deletions: raw.deletions,
  };
}

export function transformDiffLine(raw: RawDiffLine): DiffLine {
  return {
    kind: raw.kind,
    content: raw.content,
    oldLineNum: raw.old_line_num,
    newLineNum: raw.new_line_num,
  };
}

export function transformDiffHunk(raw: RawDiffHunk): DiffHunk {
  return {
    oldStart: raw.old_start,
    oldLines: raw.old_lines,
    newStart: raw.new_start,
    newLines: raw.new_lines,
    header: raw.header,
    lines: raw.lines.map(transformDiffLine),
  };
}

export function transformFileDiff(raw: RawFileDiff): FileDiff {
  return {
    filePath: raw.file_path,
    language: raw.language,
    hunks: raw.hunks.map(transformDiffHunk),
    oldTotalLines: raw.old_total_lines,
    newTotalLines: raw.new_total_lines,
    isBinary: raw.is_binary,
  };
}

export function transformRangeLine(raw: RawRangeLine): RangeLine {
  return {
    lineNum: raw.line_num,
    content: raw.content,
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
