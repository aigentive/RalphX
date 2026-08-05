// Tauri invoke wrappers for diff API with type safety using Zod schemas

import { invoke } from "@tauri-apps/api/core";
import { z } from "zod";
import {
  ConflictDiffSchema,
  FileChangesResponseSchema,
  FileDiffSchema,
  FileDiffPageSchema,
  TaskCommitsResponseSchema,
  AgentWorkspaceChangeSummaryResponseSchema,
  AgentWorkspaceReviewResponseSchema,
  RemoteAgentWorkspaceChangeSummaryResponseSchema,
  RemoteAgentWorkspaceReviewResponseSchema,
  RemoteAgentWorkspaceFileDiffPageResponseSchema,
  RemoteAgentWorkspaceFileDiffResponseSchema,
  PrDiffAnnotationsResponseSchema,
  WorkspaceReviewHunkAnnotationsResponseSchema,
  RangeFetchResponseSchema,
} from "./diff.schemas";
import {
  transformConflictDiff,
  transformFileChange,
  transformFileDiff,
  transformFileDiffPage,
  transformCommitInfo,
  transformAgentWorkspaceChangeSummary,
  transformAgentWorkspaceReview,
  transformPrDiffAnnotationsResponse,
  transformWorkspaceReviewHunkAnnotationsResponse,
  transformRangeLine,
} from "./diff.transforms";
import type {
  AgentWorkspaceChangeSummary,
  AgentWorkspaceReview,
  ConflictDiff,
  FileChange,
  FileDiff,
  FileDiffPage,
  CommitInfo,
  DiffRefKind,
  PrDiffAnnotationsResponse,
  WorkspaceReviewHunkAnnotationsResponse,
  RangeLine,
} from "./diff.types";
import { backendFetch } from "./backend";
import {
  getTransportEnvironmentId,
  isRemoteEnvironmentId,
} from "@/lib/remote/active-environment";

// Re-export types for convenience
export type {
  AgentWorkspaceChangeBucketSummary,
  AgentWorkspaceChangeSummary,
  AgentWorkspaceConflictSummary,
  AgentWorkspaceRepairState,
  AgentWorkspaceReview,
  ConflictDiff,
  FileChange,
  FileDiff,
  FileDiffPage,
  DiffPageRow,
  FileChangeStatus,
  CommitInfo,
  DiffLineKind,
  DiffLine,
  DiffHunk,
  DiffRefKind,
  PrDiffAnnotation,
  PrAnnotationSourceUnavailable,
  PrDiffAnnotationsResponse,
  WorkspaceReviewHunkAnnotation,
  WorkspaceReviewHunkAnnotationsResponse,
  RangeLine,
} from "./diff.types";

// Re-export schemas for consumers that need validation
export {
  ConflictDiffSchema,
  FileChangeSchema,
  FileChangeStatusSchema,
  FileDiffSchema,
  FileDiffPageSchema,
  DiffPageRowSchema,
  FileChangesResponseSchema,
  CommitInfoSchema,
  TaskCommitsResponseSchema,
  AgentWorkspaceChangeBucketSummarySchema,
  AgentWorkspaceConflictSummarySchema,
  AgentWorkspaceRepairStateSchema,
  AgentWorkspaceChangeSummaryResponseSchema,
  AgentWorkspaceReviewResponseSchema,
  RemoteAgentWorkspaceChangeSummaryResponseSchema,
  RemoteAgentWorkspaceReviewResponseSchema,
  RemoteAgentWorkspaceFileDiffPageResponseSchema,
  RemoteAgentWorkspaceFileDiffResponseSchema,
  DiffLineKindSchema,
  DiffLineSchema,
  DiffHunkSchema,
  DiffRefKindSchema,
  PrDiffAnnotationSchema,
  PrAnnotationSourceUnavailableSchema,
  PrDiffAnnotationsResponseSchema,
  WorkspaceReviewHunkAnnotationSchema,
  WorkspaceReviewHunkAnnotationsResponseSchema,
  RangeLineSchema,
  RangeFetchResponseSchema,
} from "./diff.schemas";

// Re-export transforms for consumers that need manual transformation
export {
  transformAgentWorkspaceChangeSummary,
  transformAgentWorkspaceReview,
  transformConflictDiff,
  transformFileChange,
  transformFileDiff,
  transformFileDiffPage,
  transformDiffPageRow,
  transformDiffLine,
  transformDiffHunk,
  transformCommitInfo,
  transformPrDiffAnnotation,
  transformPrAnnotationSourceUnavailable,
  transformPrDiffAnnotationsResponse,
  transformWorkspaceReviewHunkAnnotation,
  transformWorkspaceReviewHunkAnnotationsResponse,
  transformRangeLine,
} from "./diff.transforms";

// ============================================================================
// Typed Invoke Helper
// ============================================================================

async function typedInvokeWithTransform<TRaw, TResult>(
  cmd: string,
  args: Record<string, unknown>,
  schema: z.ZodType<TRaw>,
  transform: (raw: TRaw) => TResult
): Promise<TResult> {
  const result = await invoke(cmd, args);
  const validated = schema.parse(result);
  return transform(validated);
}

// ============================================================================
// API Object
// ============================================================================

/**
 * Diff API wrappers for Tauri commands
 * Provides file change and diff data from agent execution
 */
export const diffApi = {
  /**
   * Get all files changed by the agent for a task
   * Extracts file paths from Write/Edit tool calls in activity events
   * and uses git to determine change status
   * Backend determines working path (worktree or local) from task/project
   * @param taskId - The task ID to get file changes for
   * @returns Array of file changes with status and line counts
   */
  getTaskFileChanges: (taskId: string): Promise<FileChange[]> =>
    typedInvokeWithTransform(
      "get_task_file_changes",
      { taskId },
      FileChangesResponseSchema,
      (changes) => changes.map(transformFileChange)
    ),

  /**
   * Get the diff content for a specific file
   * Compares current file content with git HEAD
   * Backend determines working path (worktree or local) from task/project
   * @param taskId - The task ID (used to determine working directory)
   * @param filePath - The file path relative to project root
   * @returns File diff with hunks
   */
  getFileDiff: (taskId: string, filePath: string): Promise<FileDiff> =>
    typedInvokeWithTransform(
      "get_file_diff",
      { taskId, filePath },
      FileDiffSchema,
      transformFileDiff
    ),

  /**
   * Get commits on task branch since it diverged from base
   * @param taskId - The task ID to get commits for
   * @returns Array of commit info
   */
  getTaskCommits: (taskId: string): Promise<CommitInfo[]> =>
    typedInvokeWithTransform(
      "get_task_commits",
      { taskId },
      TaskCommitsResponseSchema,
      (response) => response.commits.map(transformCommitInfo)
    ),

  /**
   * Get files changed in a specific commit
   * @param taskId - The task ID (used to determine working directory)
   * @param commitSha - The commit SHA to get file changes for
   * @returns Array of file changes with status and line counts
   */
  getCommitFileChanges: (taskId: string, commitSha: string): Promise<FileChange[]> =>
    typedInvokeWithTransform(
      "get_commit_file_changes",
      { taskId, commitSha },
      FileChangesResponseSchema,
      (changes) => changes.map(transformFileChange)
    ),

  /**
   * Get diff for a file in a specific commit (comparing to parent)
   * @param taskId - The task ID (used to determine working directory)
   * @param commitSha - The commit SHA to get the diff from
   * @param filePath - The file path relative to project root
   * @returns File diff with hunks
   */
  getCommitFileDiff: (taskId: string, commitSha: string, filePath: string): Promise<FileDiff> =>
    typedInvokeWithTransform(
      "get_commit_file_diff",
      { taskId, commitSha, filePath },
      FileDiffSchema,
      transformFileDiff
    ),

  getAgentConversationWorkspaceFileChanges: (conversationId: string): Promise<FileChange[]> =>
    typedInvokeWithTransform(
      "get_agent_conversation_workspace_file_changes",
      { conversationId },
      FileChangesResponseSchema,
      (changes) => changes.map(transformFileChange)
    ),

  getAgentConversationWorkspaceReview: (
    conversationId: string
  ): Promise<AgentWorkspaceReview | null> => {
    if (!isRemoteEnvironmentId(getTransportEnvironmentId())) {
      return typedInvokeWithTransform(
        "get_agent_conversation_workspace_review",
        { conversationId },
        AgentWorkspaceReviewResponseSchema,
        transformAgentWorkspaceReview
      );
    }
    return typedInvokeWithTransform(
      "get_remote_agent_conversation_workspace_review",
      { conversationId },
      RemoteAgentWorkspaceReviewResponseSchema,
      (envelope) =>
        envelope.snapshot === null
          ? null
          : {
              ...transformAgentWorkspaceReview(envelope.snapshot),
              ...(envelope.captured_at !== null && {
                snapshotCapturedAt: envelope.captured_at,
              }),
              ...(envelope.cache_version !== null && {
                snapshotCacheVersion: envelope.cache_version,
              }),
              ...(envelope.context_source !== null && {
                snapshotContextSource: envelope.context_source,
              }),
            }
    );
  },

  getAgentConversationWorkspaceChangeSummary: (
    conversationId: string
  ): Promise<AgentWorkspaceChangeSummary | null> => {
    if (!isRemoteEnvironmentId(getTransportEnvironmentId())) {
      return typedInvokeWithTransform(
        "get_agent_conversation_workspace_change_summary",
        { conversationId },
        AgentWorkspaceChangeSummaryResponseSchema,
        transformAgentWorkspaceChangeSummary
      );
    }
    return typedInvokeWithTransform(
      "get_remote_agent_conversation_workspace_change_summary",
      { conversationId },
      RemoteAgentWorkspaceChangeSummaryResponseSchema,
      (envelope) =>
        envelope.snapshot === null
          ? null
          : {
              ...transformAgentWorkspaceChangeSummary(envelope.snapshot),
              ...(envelope.captured_at !== null && {
                snapshotCapturedAt: envelope.captured_at,
              }),
              ...(envelope.cache_version !== null && {
                snapshotCacheVersion: envelope.cache_version,
              }),
              ...(envelope.context_source !== null && {
                snapshotContextSource: envelope.context_source,
              }),
            }
    );
  },

  getAgentConversationWorkspaceRepairChangeSummary: (
    conversationId: string
  ): Promise<AgentWorkspaceChangeSummary> =>
    typedInvokeWithTransform(
      "get_agent_conversation_workspace_repair_change_summary",
      { conversationId },
      AgentWorkspaceChangeSummaryResponseSchema,
      transformAgentWorkspaceChangeSummary
    ),

  getAgentConversationWorkspacePrAnnotations: (
    conversationId: string
  ): Promise<PrDiffAnnotationsResponse> =>
    typedInvokeWithTransform(
      "get_agent_conversation_workspace_pr_annotations",
      { conversationId },
      PrDiffAnnotationsResponseSchema,
      transformPrDiffAnnotationsResponse
    ),

  getAgentConversationWorkspaceReviewHunkAnnotations: (
    conversationId: string
  ): Promise<WorkspaceReviewHunkAnnotationsResponse> =>
    typedInvokeWithTransform(
      "get_agent_conversation_workspace_review_hunk_annotations",
      { conversationId },
      WorkspaceReviewHunkAnnotationsResponseSchema,
      transformWorkspaceReviewHunkAnnotationsResponse
    ),

  getAgentConversationWorkspaceFileDiff: (
    conversationId: string,
    filePath: string
  ): Promise<FileDiff | null> => {
    if (!isRemoteEnvironmentId(getTransportEnvironmentId())) {
      return typedInvokeWithTransform(
        "get_agent_conversation_workspace_file_diff",
        { conversationId, filePath },
        FileDiffSchema,
        transformFileDiff
      );
    }
    return typedInvokeWithTransform(
      "get_remote_agent_conversation_workspace_file_diff",
      { conversationId, filePath },
      RemoteAgentWorkspaceFileDiffResponseSchema,
      (envelope) =>
        envelope.snapshot === null
          ? null
          : {
              ...transformFileDiff(envelope.snapshot),
              ...(envelope.captured_at !== null && { snapshotCapturedAt: envelope.captured_at }),
              ...(envelope.cache_version !== null && { snapshotCacheVersion: envelope.cache_version }),
              ...(envelope.context_source !== null && { snapshotContextSource: envelope.context_source }),
            }
    );
  },

  getAgentConversationWorkspaceCommits: (
    conversationId: string
  ): Promise<CommitInfo[]> =>
    typedInvokeWithTransform(
      "get_agent_conversation_workspace_commits",
      { conversationId },
      TaskCommitsResponseSchema,
      (response) => response.commits.map(transformCommitInfo)
    ),

  getAgentConversationWorkspaceCommitFileChanges: (
    conversationId: string,
    commitSha: string
  ): Promise<FileChange[]> =>
    typedInvokeWithTransform(
      "get_agent_conversation_workspace_commit_file_changes",
      { conversationId, commitSha },
      FileChangesResponseSchema,
      (changes) => changes.map(transformFileChange)
    ),

  getAgentConversationWorkspaceCommitFileDiff: (
    conversationId: string,
    commitSha: string,
    filePath: string
  ): Promise<FileDiff | null> => {
    if (!isRemoteEnvironmentId(getTransportEnvironmentId())) {
      return typedInvokeWithTransform(
        "get_agent_conversation_workspace_commit_file_diff",
        { conversationId, commitSha, filePath },
        FileDiffSchema,
        transformFileDiff
      );
    }
    return typedInvokeWithTransform(
      "get_remote_agent_conversation_workspace_commit_file_diff",
      { conversationId, commitSha, filePath },
      RemoteAgentWorkspaceFileDiffResponseSchema,
      (envelope) =>
        envelope.snapshot === null
          ? null
          : {
              ...transformFileDiff(envelope.snapshot),
              ...(envelope.captured_at !== null && { snapshotCapturedAt: envelope.captured_at }),
              ...(envelope.cache_version !== null && { snapshotCacheVersion: envelope.cache_version }),
              ...(envelope.context_source !== null && { snapshotContextSource: envelope.context_source }),
            }
    );
  },

  getAgentConversationWorkspaceStagedFileChanges: (
    conversationId: string
  ): Promise<FileChange[]> =>
    typedInvokeWithTransform(
      "get_agent_conversation_workspace_staged_file_changes",
      { conversationId },
      FileChangesResponseSchema,
      (changes) => changes.map(transformFileChange)
    ),

  getAgentConversationWorkspaceUnstagedFileChanges: (
    conversationId: string
  ): Promise<FileChange[]> =>
    typedInvokeWithTransform(
      "get_agent_conversation_workspace_unstaged_file_changes",
      { conversationId },
      FileChangesResponseSchema,
      (changes) => changes.map(transformFileChange)
    ),

  getAgentConversationWorkspaceRepairStagedFileChanges: (
    conversationId: string
  ): Promise<FileChange[]> =>
    typedInvokeWithTransform(
      "get_agent_conversation_workspace_repair_staged_file_changes",
      { conversationId },
      FileChangesResponseSchema,
      (changes) => changes.map(transformFileChange)
    ),

  getAgentConversationWorkspaceRepairUnstagedFileChanges: (
    conversationId: string
  ): Promise<FileChange[]> =>
    typedInvokeWithTransform(
      "get_agent_conversation_workspace_repair_unstaged_file_changes",
      { conversationId },
      FileChangesResponseSchema,
      (changes) => changes.map(transformFileChange)
    ),

  getAgentConversationWorkspaceStagedFileDiff: (
    conversationId: string,
    filePath: string
  ): Promise<FileDiff> =>
    typedInvokeWithTransform(
      "get_agent_conversation_workspace_staged_file_diff",
      { conversationId, filePath },
      FileDiffSchema,
      transformFileDiff
    ),

  getAgentConversationWorkspaceUnstagedFileDiff: (
    conversationId: string,
    filePath: string
  ): Promise<FileDiff> =>
    typedInvokeWithTransform(
      "get_agent_conversation_workspace_unstaged_file_diff",
      { conversationId, filePath },
      FileDiffSchema,
      transformFileDiff
    ),

  getAgentConversationWorkspaceRepairStagedFileDiff: (
    conversationId: string,
    filePath: string
  ): Promise<FileDiff> =>
    typedInvokeWithTransform(
      "get_agent_conversation_workspace_repair_staged_file_diff",
      { conversationId, filePath },
      FileDiffSchema,
      transformFileDiff
    ),

  getAgentConversationWorkspaceRepairUnstagedFileDiff: (
    conversationId: string,
    filePath: string
  ): Promise<FileDiff> =>
    typedInvokeWithTransform(
      "get_agent_conversation_workspace_repair_unstaged_file_diff",
      { conversationId, filePath },
      FileDiffSchema,
      transformFileDiff
    ),

  getAgentConversationWorkspaceRepairConflictFileDiff: (
    conversationId: string,
    filePath: string
  ): Promise<ConflictDiff> =>
    typedInvokeWithTransform(
      "get_agent_conversation_workspace_repair_conflict_file_diff",
      { conversationId, filePath },
      ConflictDiffSchema,
      transformConflictDiff
    ),

  getAgentConversationWorkspaceCumulativeFileChanges: (
    conversationId: string
  ): Promise<FileChange[]> =>
    typedInvokeWithTransform(
      "get_agent_conversation_workspace_cumulative_file_changes",
      { conversationId },
      FileChangesResponseSchema,
      (changes) => changes.map(transformFileChange)
    ),

  getAgentConversationWorkspaceCumulativeFileDiff: (
    conversationId: string,
    filePath: string
  ): Promise<FileDiff | null> => {
    if (!isRemoteEnvironmentId(getTransportEnvironmentId())) {
      return typedInvokeWithTransform(
        "get_agent_conversation_workspace_cumulative_file_diff",
        { conversationId, filePath },
        FileDiffSchema,
        transformFileDiff
      );
    }
    return typedInvokeWithTransform(
      "get_remote_agent_conversation_workspace_cumulative_file_diff",
      { conversationId, filePath },
      RemoteAgentWorkspaceFileDiffResponseSchema,
      (envelope) =>
        envelope.snapshot === null
          ? null
          : {
              ...transformFileDiff(envelope.snapshot),
              ...(envelope.captured_at !== null && { snapshotCapturedAt: envelope.captured_at }),
              ...(envelope.cache_version !== null && { snapshotCacheVersion: envelope.cache_version }),
              ...(envelope.context_source !== null && { snapshotContextSource: envelope.context_source }),
            }
    );
  },

  /**
   * Fetch a bounded page of flattened diff rows for one agent workspace file.
   * Used by large inline diffs so explicit "Show anyway" does not request the
   * full hunk payload up front.
   */
  getAgentConversationWorkspaceFileDiffPage: async (args: {
    conversationId: string;
    path: string;
    refKind: DiffRefKind;
    offset: number;
    limit: number;
  }): Promise<FileDiffPage | null> => {
    const { conversationId, path, refKind, offset, limit } = args;
    if (isRemoteEnvironmentId(getTransportEnvironmentId())) {
      return typedInvokeWithTransform(
        "get_remote_agent_conversation_workspace_file_diff_page",
        { conversationId, filePath: path, refKind, offset, limit },
        RemoteAgentWorkspaceFileDiffPageResponseSchema,
        (envelope) =>
          envelope.snapshot === null
            ? null
            : {
                ...transformFileDiffPage(envelope.snapshot),
                ...(envelope.captured_at !== null && { snapshotCapturedAt: envelope.captured_at }),
                ...(envelope.cache_version !== null && { snapshotCacheVersion: envelope.cache_version }),
                ...(envelope.context_source !== null && { snapshotContextSource: envelope.context_source }),
              }
      );
    }
    const endpoint = `agent-workspaces/${encodeURIComponent(conversationId)}/file-diff-page`;
    const params = new URLSearchParams({
      path,
      ref_kind: refKind.kind,
      offset: String(offset),
      limit: String(limit),
    });
    if (refKind.kind === "commit") {
      params.set("sha", refKind.sha);
    }
    const response = await backendFetch(`${endpoint}?${params.toString()}`);
    if (!response.ok) {
      throw new Error(`File diff page fetch failed: ${response.status} ${response.statusText}`);
    }
    const data: unknown = await response.json();
    const validated = FileDiffPageSchema.parse(data);
    return transformFileDiffPage(validated);
  },

  /**
   * Fetch a range of lines from a file in the agent workspace.
   * Used by SimpleDiffView to lazy-load "Show N unchanged lines" gaps.
   *
   * HTTP GET /api/agent-workspaces/{conversationId}/file-content-range
   *
   * @param args.conversationId - Workspace conversation ID
   * @param args.side - "old" or "new" side of the diff
   * @param args.path - File path relative to project root
   * @param args.refKind - Which ref to read the file from
   * @param args.from - First line number (1-indexed, inclusive)
   * @param args.to - Last line number (1-indexed, inclusive)
   */
  getAgentConversationWorkspaceFileContentRange: async (args: {
    conversationId: string;
    side: "old" | "new";
    path: string;
    refKind: DiffRefKind;
    from: number;
    to: number;
  }): Promise<RangeLine[]> => {
    const { conversationId, side, path, refKind, from, to } = args;
    const endpoint = `agent-workspaces/${encodeURIComponent(conversationId)}/file-content-range`;
    const params = new URLSearchParams({
      side,
      path,
      ref_kind: refKind.kind,
      from: String(from),
      to: String(to),
    });
    if (refKind.kind === "commit") {
      params.set("sha", refKind.sha);
    }
    const response = await backendFetch(`${endpoint}?${params.toString()}`);
    if (!response.ok) {
      throw new Error(
        `File content range fetch failed: ${response.status} ${response.statusText}`
      );
    }
    const data: unknown = await response.json();
    const validated = RangeFetchResponseSchema.parse(data);
    return validated.map(transformRangeLine);
  },
} as const;
