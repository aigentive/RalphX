// Tauri invoke wrappers for task context system with type safety
// Workers use these APIs to fetch rich context about tasks before implementation

import { invoke } from "@tauri-apps/api/core";
import { z } from "zod";
import { type TaskContext, type ArtifactSummary } from "@/types/task-context";
import { type Artifact, type ArtifactRelation } from "@/types/artifact";

// ============================================================================
// Response Schemas (matching Rust backend serialization with snake_case)
// ============================================================================

const TaskProposalSummaryResponseSchema = z.object({
  id: z.string(),
  title: z.string(),
  description: z.string().nullable(),
  acceptance_criteria: z.array(z.string()),
  implementation_notes: z.string().nullable(),
  plan_version_at_creation: z.number().int().nullable(),
});

const ArtifactSummaryResponseSchema = z.object({
  id: z.string(),
  title: z.string(),
  artifact_type: z.string(),
  current_version: z.number().int(),
  content_preview: z.string(),
});

const TaskContextResponseSchema = z.object({
  task: z.any(), // Task object from backend
  source_proposal: TaskProposalSummaryResponseSchema.nullable(),
  plan_artifact: ArtifactSummaryResponseSchema.nullable(),
  blueprint_artifact: ArtifactSummaryResponseSchema.nullable(),
  related_artifacts: z.array(ArtifactSummaryResponseSchema),
  context_hints: z.array(z.string()),
});

type TaskContextResponse = z.infer<typeof TaskContextResponseSchema>;
type TaskProposalSummaryResponse = z.infer<
  typeof TaskProposalSummaryResponseSchema
>;
type ArtifactSummaryResponse = z.infer<typeof ArtifactSummaryResponseSchema>;

// For artifact responses (full content)
const ArtifactResponseSchema = z.object({
  id: z.string(),
  name: z.string(),
  artifact_type: z.string(),
  content_type: z.string(),
  content: z.string(),
  created_at: z.string(),
  created_by: z.string(),
  version: z.number(),
  bucket_id: z.string().nullable(),
  task_id: z.string().nullable(),
  process_id: z.string().nullable(),
  derived_from: z.array(z.string()),
});

type ArtifactResponse = z.infer<typeof ArtifactResponseSchema>;

// For artifact relations
const ArtifactRelationResponseSchema = z.object({
  id: z.string(),
  from_artifact_id: z.string(),
  to_artifact_id: z.string(),
  relation_type: z.string(),
});

type ArtifactRelationResponse = z.infer<typeof ArtifactRelationResponseSchema>;

// ============================================================================
// Transform Functions (snake_case -> camelCase)
// ============================================================================

function transformTaskProposalSummary(
  raw: TaskProposalSummaryResponse
): TaskContext["sourceProposal"] {
  return {
    id: raw.id,
    title: raw.title,
    description: raw.description,
    acceptanceCriteria: raw.acceptance_criteria,
    implementationNotes: raw.implementation_notes,
    planVersionAtCreation: raw.plan_version_at_creation,
  };
}

function transformArtifactSummary(
  raw: ArtifactSummaryResponse
): ArtifactSummary {
  return {
    id: raw.id,
    title: raw.title,
    artifactType: raw.artifact_type as ArtifactSummary["artifactType"],
    currentVersion: raw.current_version,
    contentPreview: raw.content_preview,
  };
}

function transformTaskContext(raw: TaskContextResponse): TaskContext {
  return {
    task: raw.task, // Task is already properly formatted by backend
    sourceProposal: raw.source_proposal
      ? transformTaskProposalSummary(raw.source_proposal)
      : null,
    planArtifact: raw.plan_artifact
      ? transformArtifactSummary(raw.plan_artifact)
      : null,
    blueprintArtifact: raw.blueprint_artifact
      ? transformArtifactSummary(raw.blueprint_artifact)
      : null,
    relatedArtifacts: raw.related_artifacts.map(transformArtifactSummary),
    contextHints: raw.context_hints,
  };
}

function transformArtifact(raw: ArtifactResponse): Artifact {
  const content =
    raw.content_type === "inline"
      ? { type: "inline" as const, text: raw.content }
      : { type: "file" as const, path: raw.content };

  return {
    id: raw.id,
    type: raw.artifact_type as Artifact["type"],
    name: raw.name,
    content,
    metadata: {
      createdAt: raw.created_at,
      createdBy: raw.created_by,
      version: raw.version,
      taskId: raw.task_id ?? undefined,
      processId: raw.process_id ?? undefined,
    },
    derivedFrom: raw.derived_from,
    bucketId: raw.bucket_id ?? undefined,
  };
}

function transformArtifactRelation(
  raw: ArtifactRelationResponse
): ArtifactRelation {
  return {
    id: raw.id,
    fromArtifactId: raw.from_artifact_id,
    toArtifactId: raw.to_artifact_id,
    relationType: raw.relation_type as ArtifactRelation["relationType"],
  };
}

// ============================================================================
// Typed Invoke Helpers
// ============================================================================

async function typedInvoke<T>(
  cmd: string,
  args: Record<string, unknown>,
  schema: z.ZodType<T>
): Promise<T> {
  const result = await invoke(cmd, args);
  return schema.parse(result);
}

// ============================================================================
// API Object
// ============================================================================

/**
 * Task context API wrappers for Tauri commands
 * Used by workers to fetch context before implementation
 */
export const taskContextApi = {
  /**
   * Get rich context for a task including source proposal, implementation plan, and related artifacts
   * Workers should ALWAYS call this first before implementing any task
   *
   * @param taskId The task ID to get context for
   * @returns TaskContext with task, proposal, plan, and related artifacts
   */
  async getTaskContext(taskId: string): Promise<TaskContext> {
    const raw = await typedInvoke(
      "get_task_context",
      { taskId },
      TaskContextResponseSchema
    );
    return transformTaskContext(raw);
  },

  /**
   * Fetch the full content of an artifact by ID
   * Use after get_task_context reveals a plan_artifact_id
   *
   * @param artifactId The artifact ID to fetch
   * @returns The full artifact with content
   */
  async getArtifactFull(artifactId: string): Promise<Artifact> {
    const raw = await typedInvoke(
      "get_artifact_full",
      { artifactId },
      ArtifactResponseSchema
    );
    return transformArtifact(raw);
  },

  /**
   * Fetch a specific version of an artifact
   * Useful for accessing historical versions (e.g., plan_version_at_creation)
   *
   * @param artifactId The artifact ID
   * @param version The version number to fetch
   * @returns The artifact at the specified version
   */
  async getArtifactVersion(
    artifactId: string,
    version: number
  ): Promise<Artifact> {
    const raw = await typedInvoke(
      "get_artifact_version",
      { artifactId, version },
      ArtifactResponseSchema
    );
    return transformArtifact(raw);
  },

  /**
   * Get artifacts related to a specific artifact
   * E.g., research docs related to a plan
   *
   * @param artifactId The artifact ID to find relations for
   * @returns Array of artifact relations
   */
  async getRelatedArtifacts(artifactId: string): Promise<ArtifactRelation[]> {
    const raw = await typedInvoke(
      "get_related_artifacts",
      { artifactId },
      z.array(ArtifactRelationResponseSchema)
    );
    return raw.map(transformArtifactRelation);
  },

  /**
   * Search for artifacts in the project by query and optional type filter
   *
   * @param projectId The project ID to search within
   * @param query Search query (matches title, content)
   * @param artifactTypes Optional filter by artifact types
   * @returns Array of matching artifact summaries
   */
  async searchArtifacts(
    projectId: string,
    query: string,
    artifactTypes?: string[]
  ): Promise<ArtifactSummary[]> {
    const raw = await typedInvoke(
      "search_artifacts",
      { projectId, query, artifactTypes: artifactTypes ?? null },
      z.array(ArtifactSummaryResponseSchema)
    );
    return raw.map(transformArtifactSummary);
  },
};
