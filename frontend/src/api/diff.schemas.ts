// Zod schemas for diff API - matches Rust response format (snake_case)

import { z } from "zod";

export const FileChangeStatusSchema = z.enum(["added", "modified", "deleted"]);

export const FileChangeSchema = z.object({
  path: z.string(),
  status: FileChangeStatusSchema,
  additions: z.number(),
  deletions: z.number(),
  is_generated: z.boolean().default(false),
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

export const ConflictDiffSchema = z.object({
  filePath: z.string(),
  baseContent: z.string(),
  oursContent: z.string(),
  theirsContent: z.string(),
  mergedWithMarkers: z.string(),
  language: z.string(),
});

export const DiffPageRowSchema = z.discriminatedUnion("kind", [
  z.object({
    kind: z.literal("hunk_header"),
    header: z.string(),
    old_start: z.number(),
    old_lines: z.number(),
    new_start: z.number(),
    new_lines: z.number(),
  }),
  z.object({
    kind: z.literal("line"),
    line: DiffLineSchema,
  }),
]);

export const FileDiffPageSchema = z.object({
  file_path: z.string(),
  language: z.string(),
  rows: z.array(DiffPageRowSchema),
  offset: z.number(),
  limit: z.number(),
  next_offset: z.number().nullable(),
  total_rows: z.number(),
  old_total_lines: z.number(),
  new_total_lines: z.number(),
  is_binary: z.boolean(),
});

// ── GitHub PR annotation schemas ───────────────────────────────────────────

export const PrDiffAnnotationSchema = z.object({
  id: z.string(),
  source: z.string(),
  path: z.string().nullable(),
  side: z.string().nullable(),
  start_line: z.number().nullable(),
  end_line: z.number().nullable(),
  start_column: z.number().nullable(),
  end_column: z.number().nullable(),
  level: z.string(),
  status: z.string().nullable(),
  title: z.string().nullable(),
  message: z.string(),
  author: z.string().nullable(),
  check_name: z.string().nullable(),
  url: z.string().nullable(),
  is_outdated: z.boolean(),
  created_at: z.string().nullable(),
});

export const PrAnnotationSourceUnavailableSchema = z.object({
  source: z.string(),
  reason: z.string(),
});

export const PrDiffAnnotationsResponseSchema = z.object({
  pr_number: z.number(),
  head_sha: z.string().nullable(),
  annotations: z.array(PrDiffAnnotationSchema),
  sources_unavailable: z.array(PrAnnotationSourceUnavailableSchema),
});

// ── Workspace review hunk annotation schemas ──────────────────────────────

export const WorkspaceReviewHunkAnnotationSchema = z.object({
  id: z.string(),
  conversation_id: z.string(),
  project_id: z.string(),
  artifact_id: z.string(),
  artifact_version: z.number(),
  target_scope: z.enum(["selected_source", "workspace_delta"]),
  head_sha: z.string().nullable(),
  diff_fingerprint: z.string(),
  path: z.string(),
  diff_source: z.string(),
  hunk_header: z.string(),
  old_start: z.number(),
  old_lines: z.number(),
  new_start: z.number(),
  new_lines: z.number(),
  title: z.string().nullable(),
  message: z.string(),
  level: z.string(),
  created_by_run_id: z.string().nullable(),
  created_at: z.string(),
});

export const WorkspaceReviewHunkAnnotationsResponseSchema = z.object({
  artifact_id: z.string().nullable(),
  artifact_version: z.number().nullable(),
  target_scope: z.string().nullable(),
  head_sha: z.string().nullable(),
  diff_fingerprint: z.string().nullable(),
  annotations: z.array(WorkspaceReviewHunkAnnotationSchema),
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
  supports_worktree_modes: z.boolean().default(true),
});

export const AgentWorkspaceChangeBucketSummarySchema = z.object({
  file_count: z.number(),
  additions: z.number(),
  deletions: z.number(),
});

export const AgentWorkspaceConflictSummarySchema = z.object({
  file_count: z.number(),
  files: z.array(z.string()),
});

export const AgentWorkspaceRepairStateSchema = z.object({
  expected_branch: z.string(),
  checked_out_branch: z.string(),
  rebase_in_progress: z.boolean(),
  merge_in_progress: z.boolean(),
});

export const AgentWorkspaceChangeSummaryResponseSchema = z.object({
  supports_worktree_modes: z.boolean().default(true),
  staged: AgentWorkspaceChangeBucketSummarySchema,
  unstaged: AgentWorkspaceChangeBucketSummarySchema,
  conflicted: AgentWorkspaceConflictSummarySchema.optional(),
  repair_state: AgentWorkspaceRepairStateSchema.optional(),
});

export const AgentWorkspaceContextSourceSchema = z.enum([
  "worktree",
  "local_branch",
  "plan_branch",
  "pull_request_head",
  "github_patch",
  "terminal_pull_request_head",
  "repair_worktree",
]);

export const RemoteAgentWorkspaceReviewResponseSchema = z.object({
  snapshot: AgentWorkspaceReviewResponseSchema.nullable(),
  captured_at: z.string().nullable(),
  cache_version: z.string().nullable(),
  context_source: AgentWorkspaceContextSourceSchema.nullable(),
});

export const RemoteAgentWorkspaceChangeSummaryResponseSchema = z.object({
  snapshot: AgentWorkspaceChangeSummaryResponseSchema.nullable(),
  captured_at: z.string().nullable(),
  cache_version: z.string().nullable(),
  context_source: AgentWorkspaceContextSourceSchema.nullable(),
});
