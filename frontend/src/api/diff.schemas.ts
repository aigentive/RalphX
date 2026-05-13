// Zod schemas for diff API - matches Rust response format (snake_case)

import { z } from "zod";

export const FileChangeStatusSchema = z.enum(["added", "modified", "deleted"]);

export const FileChangeSchema = z.object({
  path: z.string(),
  status: FileChangeStatusSchema,
  additions: z.number(),
  deletions: z.number(),
});

export const FileDiffSchema = z.object({
  file_path: z.string(),
  old_content: z.string(),
  new_content: z.string(),
  language: z.string(),
});

export const FileChangesResponseSchema = z.array(FileChangeSchema);

export const CommitInfoSchema = z.object({
  sha: z.string(),
  short_sha: z.string(),
  message: z.string(),
  author: z.string(),
  timestamp: z.string(),
});

export const TaskCommitsResponseSchema = z.object({
  commits: z.array(CommitInfoSchema),
});

export const AgentWorkspaceReviewResponseSchema = z.object({
  changes: z.array(FileChangeSchema),
  commits: z.array(CommitInfoSchema),
  base_ref: z.string(),
  head_ref: z.string(),
});
