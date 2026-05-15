// Zod schemas for diff API - matches Rust response format (snake_case)

import { z } from "zod";

export const FileChangeStatusSchema = z.enum(["added", "modified", "deleted"]);

export const FileChangeSchema = z.object({
  path: z.string(),
  status: FileChangeStatusSchema,
  additions: z.number(),
  deletions: z.number(),
});

// ── Hunk-based diff schemas ────────────────────────────────────────────────

export const DiffLineKindSchema = z.enum(["context", "addition", "deletion"]);

export const DiffLineSchema = z.object({
  kind: DiffLineKindSchema,
  content: z.string(),
  old_line_num: z.number().nullable(),
  new_line_num: z.number().nullable(),
});

export const DiffHunkSchema = z.object({
  old_start: z.number(),
  old_lines: z.number(),
  new_start: z.number(),
  new_lines: z.number(),
  header: z.string(),
  lines: z.array(DiffLineSchema),
});

export const FileDiffSchema = z.object({
  file_path: z.string(),
  language: z.string(),
  hunks: z.array(DiffHunkSchema),
  old_total_lines: z.number(),
  new_total_lines: z.number(),
  is_binary: z.boolean(),
});

// ── DiffRefKind — tagged enum from backend ────────────────────────────────

export const DiffRefKindSchema = z.discriminatedUnion("kind", [
  z.object({ kind: z.literal("head") }),
  z.object({ kind: z.literal("staged") }),
  z.object({ kind: z.literal("unstaged") }),
  z.object({ kind: z.literal("commit"), sha: z.string() }),
  z.object({ kind: z.literal("cumulative_base") }),
  z.object({ kind: z.literal("cumulative_head") }),
]);

// ── Range fetch response ──────────────────────────────────────────────────

export const RangeLineSchema = z.object({
  line_num: z.number(),
  content: z.string(),
});

export const RangeFetchResponseSchema = z.array(RangeLineSchema);

// ── Collection schemas ────────────────────────────────────────────────────

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
