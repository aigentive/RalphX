// Task context types and Zod schemas
// Used by workers to fetch rich context about tasks before implementation

import { z } from "zod";
import { ArtifactTypeSchema } from "./artifact";
import { TaskSchema } from "./task";

// ============================================================================
// Task Proposal Summary
// ============================================================================

/**
 * Summary of the proposal that created a task
 * Contains key information for worker context without full proposal details
 */
export const TaskProposalSummarySchema = z.object({
  id: z.string().min(1),
  title: z.string().min(1),
  description: z.string().nullable(),
  acceptanceCriteria: z.array(z.string()),
  implementationNotes: z.string().nullable(),
  /** Version of the plan artifact when proposal was created */
  planVersionAtCreation: z.number().int().nullable(),
});

export type TaskProposalSummary = z.infer<typeof TaskProposalSummarySchema>;

// ============================================================================
// Artifact Summary
// ============================================================================

/**
 * Summary of an artifact with content preview
 * Full content requires separate get_artifact call to avoid context bloat
 */
export const ArtifactSummarySchema = z.object({
  id: z.string().min(1),
  title: z.string().min(1),
  artifactType: ArtifactTypeSchema,
  currentVersion: z.number().int().positive(),
  /** First ~500 chars of content as preview */
  contentPreview: z.string(),
});

export type ArtifactSummary = z.infer<typeof ArtifactSummarySchema>;

// ============================================================================
// Task Context
// ============================================================================

/**
 * Rich context returned by get_task_context MCP tool
 * Provides workers with all relevant information before implementation
 */
export const TaskContextSchema = z.object({
  /** The task being executed (full Task object) */
  task: TaskSchema,
  /** Source proposal if task was created from ideation */
  sourceProposal: TaskProposalSummarySchema.nullable(),
  /** Implementation plan artifact (summary, not full content) */
  planArtifact: ArtifactSummarySchema.nullable(),
  /** Exact implementation blueprint used for execution/review. */
  blueprintArtifact: ArtifactSummarySchema.nullable(),
  /** Other artifacts related to the plan */
  relatedArtifacts: z.array(ArtifactSummarySchema),
  /** Hints for worker about what context might be useful */
  contextHints: z.array(z.string()),
});

export type TaskContext = z.infer<typeof TaskContextSchema>;
