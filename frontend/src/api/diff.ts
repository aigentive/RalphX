// Tauri invoke wrappers for diff API with type safety using Zod schemas

import { invoke } from "@tauri-apps/api/core";
import { z } from "zod";
import {
  FileChangesResponseSchema,
  FileDiffSchema,
  TaskCommitsResponseSchema,
  AgentWorkspaceReviewResponseSchema,
} from "./diff.schemas";
import {
  transformFileChange,
  transformFileDiff,
  transformCommitInfo,
  transformAgentWorkspaceReview,
} from "./diff.transforms";
import type {
  AgentWorkspaceReview,
  FileChange,
  FileDiff,
  CommitInfo,
} from "./diff.types";

// Re-export types for convenience
export type {
  AgentWorkspaceReview,
  FileChange,
  FileDiff,
  FileChangeStatus,
  CommitInfo,
} from "./diff.types";

// Re-export schemas for consumers that need validation
export {
  FileChangeSchema,
  FileChangeStatusSchema,
  FileDiffSchema,
  FileChangesResponseSchema,
  CommitInfoSchema,
  TaskCommitsResponseSchema,
  AgentWorkspaceReviewResponseSchema,
} from "./diff.schemas";

// Re-export transforms for consumers that need manual transformation
export {
  transformAgentWorkspaceReview,
  transformFileChange,
  transformFileDiff,
  transformCommitInfo,
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
   * @returns File diff with old and new content
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
   * @returns File diff with old (parent) and new (commit) content
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
  ): Promise<AgentWorkspaceReview> =>
    typedInvokeWithTransform(
      "get_agent_conversation_workspace_review",
      { conversationId },
      AgentWorkspaceReviewResponseSchema,
      transformAgentWorkspaceReview
    ),

  getAgentConversationWorkspaceFileDiff: (
    conversationId: string,
    filePath: string
  ): Promise<FileDiff> =>
    typedInvokeWithTransform(
      "get_agent_conversation_workspace_file_diff",
      { conversationId, filePath },
      FileDiffSchema,
      transformFileDiff
    ),

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
  ): Promise<FileDiff> =>
    typedInvokeWithTransform(
      "get_agent_conversation_workspace_commit_file_diff",
      { conversationId, commitSha, filePath },
      FileDiffSchema,
      transformFileDiff
    ),

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
  ): Promise<FileDiff> =>
    typedInvokeWithTransform(
      "get_agent_conversation_workspace_cumulative_file_diff",
      { conversationId, filePath },
      FileDiffSchema,
      transformFileDiff
    ),
} as const;
