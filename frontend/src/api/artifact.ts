// Tauri invoke wrappers for artifact system with type safety

import { invoke } from "@tauri-apps/api/core";
import { z } from "zod";
import type { Artifact, PlanComplexityAssessment } from "@/types/artifact";
import {
  PlanApprovalStatusSchema,
  PlanComplexityLevelSchema,
  PlanComplexityRecommendedActionSchema,
} from "@/types/artifact";
import { ArtifactVersionSummarySchema } from "@/types/artifact";
import type { ArtifactVersionSummary } from "@/types/artifact";
import { backendFetch } from "@/api/backend";

// ============================================================================
// Response Schemas (matching Rust backend serialization with snake_case)
// ============================================================================

const PlanBundleArtifactResponseSchema = z.object({
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
  artifact_role: z.enum(["overview", "blueprint"]).nullable().optional(),
});

export const ArtifactResponseSchema = PlanBundleArtifactResponseSchema.extend({
  plan_approval_status: PlanApprovalStatusSchema.optional(),
  plan_approved_artifact_id: z.string().nullable().optional(),
  plan_approved_version: z.number().int().positive().nullable().optional(),
  plan_approved_blueprint_artifact_id: z.string().nullable().optional(),
  plan_approved_blueprint_version: z.number().int().positive().nullable().optional(),
  plan_approved_at: z.string().nullable().optional(),
  blueprint_artifact: PlanBundleArtifactResponseSchema.nullable().optional(),
  plan_contract_version: z.union([z.literal(1), z.literal(2)]).nullable().optional(),
  plan_target_id: z.string().nullable().optional(),
});

export type ArtifactResponse = z.infer<typeof ArtifactResponseSchema>;

export const PlanComplexityAssessmentResponseSchema = z.object({
  id: z.string(),
  session_id: z.string(),
  artifact_id: z.string(),
  artifact_version: z.number().int().positive(),
  blueprint_artifact_id: z.string().nullable().optional(),
  blueprint_artifact_version: z.number().int().positive().nullable().optional(),
  level: PlanComplexityLevelSchema,
  score: z.number().int().min(0).max(100),
  recommended_action: PlanComplexityRecommendedActionSchema,
  confidence: z.number().min(0).max(1),
  reason_summary: z.string(),
  signals: z.record(z.string(), z.unknown()),
  assessed_by: z.string(),
  created_at: z.string(),
  updated_at: z.string(),
});

export type PlanComplexityAssessmentResponse = z.infer<
  typeof PlanComplexityAssessmentResponseSchema
>;

// ============================================================================
// Transform Functions (snake_case -> camelCase)
// ============================================================================

export function transformArtifactResponse(raw: ArtifactResponse): Artifact {
  const content =
    raw.content_type === "inline"
      ? { type: "inline" as const, text: raw.content }
      : { type: "file" as const, path: raw.content };

  const planApproval = raw.plan_approval_status
    ? {
        status: raw.plan_approval_status,
        ...(raw.plan_approved_artifact_id != null && {
          approvedArtifactId: raw.plan_approved_artifact_id,
        }),
        ...(raw.plan_approved_version != null && {
          approvedVersion: raw.plan_approved_version,
        }),
        ...(raw.plan_approved_blueprint_artifact_id != null && {
          approvedBlueprintArtifactId:
            raw.plan_approved_blueprint_artifact_id,
        }),
        ...(raw.plan_approved_blueprint_version != null && {
          approvedBlueprintVersion: raw.plan_approved_blueprint_version,
        }),
        ...(raw.plan_approved_at != null && {
          approvedAt: raw.plan_approved_at,
        }),
      }
    : undefined;

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
    ...(planApproval !== undefined && { planApproval }),
    ...(raw.artifact_role != null && { artifactRole: raw.artifact_role }),
    ...(raw.blueprint_artifact != null && {
      blueprint: transformPlanBundleMember(raw.blueprint_artifact),
    }),
    ...(raw.plan_contract_version != null && {
      planContractVersion: raw.plan_contract_version,
    }),
    ...(raw.plan_target_id != null && { planTargetId: raw.plan_target_id }),
  };
}

function transformPlanBundleMember(
  raw: z.infer<typeof PlanBundleArtifactResponseSchema>,
): NonNullable<Artifact["blueprint"]> {
  return {
    id: raw.id,
    type: raw.artifact_type as Artifact["type"],
    name: raw.name,
    content:
      raw.content_type === "inline"
        ? { type: "inline", text: raw.content }
        : { type: "file", path: raw.content },
    metadata: {
      createdAt: raw.created_at,
      createdBy: raw.created_by,
      version: raw.version,
      taskId: raw.task_id ?? undefined,
      processId: raw.process_id ?? undefined,
    },
    derivedFrom: raw.derived_from,
    bucketId: raw.bucket_id ?? undefined,
    artifactRole: raw.artifact_role ?? undefined,
  };
}

export function transformPlanComplexityAssessmentResponse(
  raw: PlanComplexityAssessmentResponse,
): PlanComplexityAssessment {
  return {
    id: raw.id,
    sessionId: raw.session_id,
    artifactId: raw.artifact_id,
    artifactVersion: raw.artifact_version,
    blueprintArtifactId: raw.blueprint_artifact_id ?? undefined,
    blueprintArtifactVersion: raw.blueprint_artifact_version ?? undefined,
    level: raw.level,
    score: raw.score,
    recommendedAction: raw.recommended_action,
    confidence: raw.confidence,
    reasonSummary: raw.reason_summary,
    signals: raw.signals,
    assessedBy: raw.assessed_by,
    createdAt: raw.created_at,
    updatedAt: raw.updated_at,
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

async function parseHttpArtifactResponse(response: Response): Promise<Artifact | null> {
  if (!response.ok) {
    let message = response.statusText;
    try {
      const body = await response.json();
      if (typeof body?.message === "string" && body.message.trim()) {
        message = body.message;
      }
    } catch {
      // Keep statusText fallback.
    }
    throw new Error(message || `Request failed with status ${response.status}`);
  }

  const raw = await response.json();
  const parsed = ArtifactResponseSchema.nullable().parse(raw);
  return parsed ? transformArtifactResponse(parsed) : null;
}

// ============================================================================
// API Object
// ============================================================================

/**
 * Artifact API wrappers for Tauri commands
 */
export const artifactApi = {
  /**
   * Get an artifact by ID
   * @param artifactId The artifact ID
   * @returns The artifact or null if not found
   */
  get: async (artifactId: string): Promise<Artifact | null> => {
    const raw = await typedInvoke(
      "get_artifact",
      { id: artifactId },
      ArtifactResponseSchema.nullable()
    );
    return raw ? transformArtifactResponse(raw) : null;
  },

  /**
   * Get the current plan artifact for an ideation/planning session.
   * Planning sessions include backend-owned approval state.
   */
  getSessionPlan: async (sessionId: string): Promise<Artifact | null> => {
    const response = await backendFetch(
      `get_session_plan/${encodeURIComponent(sessionId)}`,
    );
    return parseHttpArtifactResponse(response);
  },

  /**
   * Approve the current Plan-mode artifact version for a planning session.
   */
  approvePlanArtifact: async ({
    sessionId,
    artifactId,
    blueprintArtifactId,
    blueprintArtifactVersion,
  }: {
    sessionId: string;
    artifactId?: string | undefined;
    blueprintArtifactId?: string | undefined;
    blueprintArtifactVersion?: number | undefined;
  }): Promise<Artifact> => {
    const response = await backendFetch("approve_plan_artifact", {
      method: "POST",
      headers: { "Content-Type": "application/json" },
      body: JSON.stringify({
        session_id: sessionId,
        ...(artifactId !== undefined && { artifact_id: artifactId }),
        ...(blueprintArtifactId !== undefined && {
          blueprint_artifact_id: blueprintArtifactId,
        }),
        ...(blueprintArtifactVersion !== undefined && {
          blueprint_artifact_version: blueprintArtifactVersion,
        }),
      }),
    });
    const artifact = await parseHttpArtifactResponse(response);
    if (!artifact) {
      throw new Error("Plan approval did not return an artifact.");
    }
    return artifact;
  },

  /**
   * Get the current approved Plan-mode complexity assessment, if one exists.
   */
  getPlanComplexityAssessment: async (
    sessionId: string,
  ): Promise<PlanComplexityAssessment | null> => {
    const response = await backendFetch(
      `plan_complexity_assessment/${encodeURIComponent(sessionId)}`,
    );
    if (!response.ok) {
      let message = response.statusText;
      try {
        const body = await response.json();
        if (typeof body?.message === "string" && body.message.trim()) {
          message = body.message;
        }
      } catch {
        // Keep statusText fallback.
      }
      throw new Error(message || `Request failed with status ${response.status}`);
    }

    const raw = await response.json();
    const parsed = PlanComplexityAssessmentResponseSchema.nullable().parse(raw);
    return parsed ? transformPlanComplexityAssessmentResponse(parsed) : null;
  },

  /**
   * Get an artifact at a specific version
   * @param artifactId The artifact ID
   * @param version The version number to retrieve
   * @returns The artifact at the specified version or null if not found
   */
  getAtVersion: async (artifactId: string, version: number): Promise<Artifact | null> => {
    const raw = await typedInvoke(
      "get_artifact_at_version",
      { id: artifactId, version },
      ArtifactResponseSchema.nullable()
    );
    return raw ? transformArtifactResponse(raw) : null;
  },

  /**
   * Get all artifacts for a task
   * @param taskId The task ID
   * @returns Array of artifacts
   */
  getByTask: async (taskId: string): Promise<Artifact[]> => {
    const raw = await typedInvoke(
      "get_artifacts_by_task",
      { taskId },
      z.array(ArtifactResponseSchema)
    );
    return raw.map(transformArtifactResponse);
  },

  /**
   * Get all artifacts in a bucket
   * @param bucketId The bucket ID
   * @returns Array of artifacts
   */
  getByBucket: async (bucketId: string): Promise<Artifact[]> => {
    const raw = await typedInvoke(
      "get_artifacts_by_bucket",
      { bucketId },
      z.array(ArtifactResponseSchema)
    );
    return raw.map(transformArtifactResponse);
  },

  /**
   * Get version history for an artifact
   * @param artifactId The artifact ID
   * @returns Array of version summaries ordered newest-first
   */
  getVersionHistory: async (artifactId: string): Promise<ArtifactVersionSummary[]> => {
    return typedInvoke(
      "get_artifact_version_history",
      { id: artifactId },
      z.array(ArtifactVersionSummarySchema)
    );
  },
} as const;
