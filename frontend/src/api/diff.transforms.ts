// Transform functions for diff API (snake_case -> camelCase)

import type { z } from "zod";
import type {
  AgentWorkspaceChangeSummaryResponseSchema,
  AgentWorkspaceReviewResponseSchema,
  FileChangeSchema,
  FileDiffSchema,
  ConflictDiffSchema,
  FileDiffPageSchema,
  DiffPageRowSchema,
  DiffLineSchema,
  DiffHunkSchema,
  CommitInfoSchema,
  PrDiffAnnotationSchema,
  PrAnnotationSourceUnavailableSchema,
  PrDiffAnnotationsResponseSchema,
  RangeLineSchema,
} from "./diff.schemas";
import type {
  AgentWorkspaceChangeBucketSummary,
  AgentWorkspaceChangeSummary,
  AgentWorkspaceReview,
  FileChange,
  FileDiff,
  ConflictDiff,
  FileDiffPage,
  DiffPageRow,
  DiffLine,
  DiffHunk,
  CommitInfo,
  PrDiffAnnotation,
  PrAnnotationSourceUnavailable,
  PrDiffAnnotationsResponse,
  RangeLine,
} from "./diff.types";

type RawFileChange = z.infer<typeof FileChangeSchema>;
type RawFileDiff = z.infer<typeof FileDiffSchema>;
type RawConflictDiff = z.infer<typeof ConflictDiffSchema>;
type RawFileDiffPage = z.infer<typeof FileDiffPageSchema>;
type RawDiffPageRow = z.infer<typeof DiffPageRowSchema>;
type RawDiffLine = z.infer<typeof DiffLineSchema>;
type RawDiffHunk = z.infer<typeof DiffHunkSchema>;
type RawCommitInfo = z.infer<typeof CommitInfoSchema>;
type RawPrDiffAnnotation = z.infer<typeof PrDiffAnnotationSchema>;
type RawPrAnnotationSourceUnavailable = z.infer<typeof PrAnnotationSourceUnavailableSchema>;
type RawPrDiffAnnotationsResponse = z.infer<typeof PrDiffAnnotationsResponseSchema>;
type RawRangeLine = z.infer<typeof RangeLineSchema>;
type RawAgentWorkspaceReview = z.infer<typeof AgentWorkspaceReviewResponseSchema>;
type RawAgentWorkspaceChangeSummary = z.infer<
  typeof AgentWorkspaceChangeSummaryResponseSchema
>;

export function transformFileChange(raw: RawFileChange): FileChange {
  return {
    path: raw.path,
    status: raw.status,
    additions: raw.additions,
    deletions: raw.deletions,
    isGenerated: raw.is_generated ?? false,
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

export function transformConflictDiff(raw: RawConflictDiff): ConflictDiff {
  return {
    filePath: raw.filePath,
    baseContent: raw.baseContent,
    oursContent: raw.oursContent,
    theirsContent: raw.theirsContent,
    mergedWithMarkers: raw.mergedWithMarkers,
    language: raw.language,
  };
}

export function transformDiffPageRow(raw: RawDiffPageRow): DiffPageRow {
  if (raw.kind === "hunk_header") {
    return {
      kind: raw.kind,
      header: raw.header,
    };
  }
  return {
    kind: raw.kind,
    line: transformDiffLine(raw.line),
  };
}

export function transformFileDiffPage(raw: RawFileDiffPage): FileDiffPage {
  return {
    filePath: raw.file_path,
    language: raw.language,
    rows: raw.rows.map(transformDiffPageRow),
    offset: raw.offset,
    limit: raw.limit,
    nextOffset: raw.next_offset,
    totalRows: raw.total_rows,
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

export function transformPrDiffAnnotation(raw: RawPrDiffAnnotation): PrDiffAnnotation {
  return {
    id: raw.id,
    source: raw.source,
    path: raw.path,
    side: raw.side,
    startLine: raw.start_line,
    endLine: raw.end_line,
    startColumn: raw.start_column,
    endColumn: raw.end_column,
    level: raw.level,
    status: raw.status,
    title: raw.title,
    message: raw.message,
    author: raw.author,
    checkName: raw.check_name,
    url: raw.url,
    isOutdated: raw.is_outdated,
    createdAt: raw.created_at,
  };
}

export function transformPrAnnotationSourceUnavailable(
  raw: RawPrAnnotationSourceUnavailable
): PrAnnotationSourceUnavailable {
  return {
    source: raw.source,
    reason: raw.reason,
  };
}

export function transformPrDiffAnnotationsResponse(
  raw: RawPrDiffAnnotationsResponse
): PrDiffAnnotationsResponse {
  return {
    prNumber: raw.pr_number,
    headSha: raw.head_sha,
    annotations: raw.annotations.map(transformPrDiffAnnotation),
    sourcesUnavailable: raw.sources_unavailable.map(transformPrAnnotationSourceUnavailable),
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
    supportsWorktreeModes: raw.supports_worktree_modes,
  };
}

function transformAgentWorkspaceChangeBucketSummary(
  raw: RawAgentWorkspaceChangeSummary["staged"]
): AgentWorkspaceChangeBucketSummary {
  return {
    fileCount: raw.file_count,
    additions: raw.additions,
    deletions: raw.deletions,
  };
}

export function transformAgentWorkspaceChangeSummary(
  raw: RawAgentWorkspaceChangeSummary
): AgentWorkspaceChangeSummary {
  return {
    supportsWorktreeModes: raw.supports_worktree_modes,
    staged: transformAgentWorkspaceChangeBucketSummary(raw.staged),
    unstaged: transformAgentWorkspaceChangeBucketSummary(raw.unstaged),
    ...(raw.conflicted
      ? {
          conflicted: {
            fileCount: raw.conflicted.file_count,
            files: raw.conflicted.files,
          },
        }
      : {}),
    ...(raw.repair_state
      ? {
          repairState: {
            expectedBranch: raw.repair_state.expected_branch,
            checkedOutBranch: raw.repair_state.checked_out_branch,
            rebaseInProgress: raw.repair_state.rebase_in_progress,
            mergeInProgress: raw.repair_state.merge_in_progress,
          },
        }
      : {}),
  };
}
