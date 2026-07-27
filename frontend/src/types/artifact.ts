/**
 * Artifact type definitions for the extensibility system
 *
 * Artifacts are typed documents that flow between processes -
 * outputs from one process become inputs to another.
 */

import { z } from "zod";
import {
  AGENT_DEEP_RESEARCHER,
  AGENT_ORCHESTRATOR,
  AGENT_WORKER,
} from "@/constants/agents";

// ============================================
// Artifact Type
// ============================================

/**
 * All 20 artifact types organized by category:
 * - Documents: prd, persona, research_document, design_doc, specification
 * - Code: code_change, diff, test_result
 * - Process: task_spec, review_feedback, approval, findings, recommendations, pr_review
 * - Context: context, previous_work, research_brief
 * - Logs: activity_log, alert, intervention
 */
export const ArtifactTypeSchema = z.enum([
  // Documents
  "prd",
  "persona",
  "research_document",
  "design_doc",
  "specification",
  // Code
  "code_change",
  "diff",
  "test_result",
  // Process
  "task_spec",
  "review_feedback",
  "pr_review",
  "approval",
  "findings",
  "recommendations",
  // Context
  "context",
  "previous_work",
  "research_brief",
  // Logs
  "activity_log",
  "alert",
  "intervention",
]);

export type ArtifactType = z.infer<typeof ArtifactTypeSchema>;

/**
 * All artifact type values as a readonly array
 */
export const ARTIFACT_TYPE_VALUES = ArtifactTypeSchema.options;

/**
 * Document artifact types
 */
export const DOCUMENT_ARTIFACT_TYPES: readonly ArtifactType[] = [
  "prd",
  "persona",
  "research_document",
  "design_doc",
  "specification",
] as const;

/**
 * Code artifact types
 */
export const CODE_ARTIFACT_TYPES: readonly ArtifactType[] = [
  "code_change",
  "diff",
  "test_result",
] as const;

/**
 * Process artifact types
 */
export const PROCESS_ARTIFACT_TYPES: readonly ArtifactType[] = [
  "task_spec",
  "review_feedback",
  "pr_review",
  "approval",
  "findings",
  "recommendations",
] as const;

/**
 * Context artifact types
 */
export const CONTEXT_ARTIFACT_TYPES: readonly ArtifactType[] = [
  "context",
  "previous_work",
  "research_brief",
] as const;

/**
 * Log artifact types
 */
export const LOG_ARTIFACT_TYPES: readonly ArtifactType[] = [
  "activity_log",
  "alert",
  "intervention",
] as const;

/**
 * Check if an artifact type is a document type
 */
export function isDocumentArtifact(type: ArtifactType): boolean {
  return (DOCUMENT_ARTIFACT_TYPES as readonly string[]).includes(type);
}

/**
 * Check if an artifact type is a code type
 */
export function isCodeArtifact(type: ArtifactType): boolean {
  return (CODE_ARTIFACT_TYPES as readonly string[]).includes(type);
}

/**
 * Check if an artifact type is a process type
 */
export function isProcessArtifact(type: ArtifactType): boolean {
  return (PROCESS_ARTIFACT_TYPES as readonly string[]).includes(type);
}

/**
 * Check if an artifact type is a context type
 */
export function isContextArtifact(type: ArtifactType): boolean {
  return (CONTEXT_ARTIFACT_TYPES as readonly string[]).includes(type);
}

/**
 * Check if an artifact type is a log type
 */
export function isLogArtifact(type: ArtifactType): boolean {
  return (LOG_ARTIFACT_TYPES as readonly string[]).includes(type);
}

// ============================================
// Artifact Content
// ============================================

/**
 * Inline content stored directly in the database
 */
export const ArtifactContentInlineSchema = z.object({
  type: z.literal("inline"),
  text: z.string(),
});

export type ArtifactContentInline = z.infer<typeof ArtifactContentInlineSchema>;

/**
 * File content stored at a path
 */
export const ArtifactContentFileSchema = z.object({
  type: z.literal("file"),
  path: z.string(),
});

export type ArtifactContentFile = z.infer<typeof ArtifactContentFileSchema>;

/**
 * Artifact content - either inline text or a file path
 */
export const ArtifactContentSchema = z.discriminatedUnion("type", [
  ArtifactContentInlineSchema,
  ArtifactContentFileSchema,
]);

export type ArtifactContent = z.infer<typeof ArtifactContentSchema>;

// ============================================
// Artifact Metadata
// ============================================

/**
 * Metadata about an artifact
 */
export const ArtifactMetadataSchema = z.object({
  /** When the artifact was created (ISO 8601 string) */
  createdAt: z.string(),
  /** Who created the artifact (agent profile ID or "user") */
  createdBy: z.string(),
  /** Optional associated task ID */
  taskId: z.string().optional(),
  /** Optional associated process ID */
  processId: z.string().optional(),
  /** Version number (starts at 1) */
  version: z.number().int().positive().default(1),
});

export type ArtifactMetadata = z.infer<typeof ArtifactMetadataSchema>;

export const PlanApprovalStatusSchema = z.enum(["draft", "approved"]);
export type PlanApprovalStatus = z.infer<typeof PlanApprovalStatusSchema>;

export const PlanApprovalSchema = z.object({
  status: PlanApprovalStatusSchema,
  approvedArtifactId: z.string().optional(),
  approvedVersion: z.number().int().positive().optional(),
  approvedBlueprintArtifactId: z.string().optional(),
  approvedBlueprintVersion: z.number().int().positive().optional(),
  approvedAt: z.string().optional(),
});

export type PlanApproval = z.infer<typeof PlanApprovalSchema>;

export const PlanComplexityLevelSchema = z.enum([
  "trivial",
  "simple",
  "moderate",
  "complex",
  "very_complex",
]);
export type PlanComplexityLevel = z.infer<typeof PlanComplexityLevelSchema>;

export const PlanComplexityRecommendedActionSchema = z.enum([
  "implement_directly",
  "create_proposals",
]);
export type PlanComplexityRecommendedAction = z.infer<
  typeof PlanComplexityRecommendedActionSchema
>;

export const PlanComplexityAssessmentSchema = z.object({
  id: z.string(),
  sessionId: z.string(),
  artifactId: z.string(),
  artifactVersion: z.number().int().positive(),
  blueprintArtifactId: z.string().optional(),
  blueprintArtifactVersion: z.number().int().positive().optional(),
  level: PlanComplexityLevelSchema,
  score: z.number().int().min(0).max(100),
  recommendedAction: PlanComplexityRecommendedActionSchema,
  confidence: z.number().min(0).max(1),
  reasonSummary: z.string(),
  signals: z.record(z.string(), z.unknown()),
  assessedBy: z.string(),
  createdAt: z.string(),
  updatedAt: z.string(),
});

export type PlanComplexityAssessment = z.infer<
  typeof PlanComplexityAssessmentSchema
>;

// ============================================
// Artifact
// ============================================

/**
 * An artifact - a typed document that flows between processes
 */
const PlanBundleArtifactSchema = z.object({
  id: z.string(),
  type: ArtifactTypeSchema,
  name: z.string(),
  content: ArtifactContentSchema,
  metadata: ArtifactMetadataSchema,
  derivedFrom: z.array(z.string()).default([]),
  bucketId: z.string().optional(),
  artifactRole: z.enum(["overview", "blueprint"]).optional(),
});

export const ArtifactSchema = PlanBundleArtifactSchema.extend({
  /** Unique identifier */
  id: z.string(),
  /** The type of artifact */
  type: ArtifactTypeSchema,
  /** Display name */
  name: z.string(),
  /** The content (inline or file) */
  content: ArtifactContentSchema,
  /** Artifact metadata */
  metadata: ArtifactMetadataSchema,
  /** IDs of artifacts this was derived from */
  derivedFrom: z.array(z.string()).default([]),
  /** Optional bucket ID this artifact belongs to */
  bucketId: z.string().optional(),
  /** Optional Plan-mode approval state for the artifact in its owning session */
  planApproval: PlanApprovalSchema.optional(),
  blueprint: PlanBundleArtifactSchema.optional(),
  planContractVersion: z.union([z.literal(1), z.literal(2)]).optional(),
  planTargetId: z.string().optional(),
});

export type Artifact = z.infer<typeof ArtifactSchema>;

// ============================================
// Artifact Bucket
// ============================================

/**
 * An artifact bucket - organizes artifacts by purpose with access control
 */
export const ArtifactBucketSchema = z.object({
  /** Unique identifier */
  id: z.string(),
  /** Display name */
  name: z.string(),
  /** Artifact types accepted in this bucket */
  acceptedTypes: z.array(ArtifactTypeSchema),
  /** Who can write to this bucket (agent profile IDs, "user", or "system") */
  writers: z.array(z.string()),
  /** Who can read from this bucket (agent profile IDs or "all") */
  readers: z.array(z.string()),
  /** Whether this is a system bucket (cannot be deleted) */
  isSystem: z.boolean().default(false),
});

export type ArtifactBucket = z.infer<typeof ArtifactBucketSchema>;

// ============================================
// Artifact Relation
// ============================================

/**
 * The type of relation between artifacts
 */
export const ArtifactRelationTypeSchema = z.enum(["derived_from", "related_to"]);

export type ArtifactRelationType = z.infer<typeof ArtifactRelationTypeSchema>;

/**
 * All artifact relation type values as a readonly array
 */
export const ARTIFACT_RELATION_TYPE_VALUES = ArtifactRelationTypeSchema.options;

/**
 * A relation between two artifacts
 */
export const ArtifactRelationSchema = z.object({
  /** Unique identifier */
  id: z.string(),
  /** The source artifact ID */
  fromArtifactId: z.string(),
  /** The target artifact ID */
  toArtifactId: z.string(),
  /** The type of relation */
  relationType: ArtifactRelationTypeSchema,
});

export type ArtifactRelation = z.infer<typeof ArtifactRelationSchema>;

// ============================================
// Artifact Flow
// ============================================

/**
 * The event that triggers an artifact flow
 */
export const ArtifactFlowEventSchema = z.enum([
  "artifact_created",
  "task_completed",
  "process_completed",
]);

export type ArtifactFlowEvent = z.infer<typeof ArtifactFlowEventSchema>;

/**
 * All artifact flow event values as a readonly array
 */
export const ARTIFACT_FLOW_EVENT_VALUES = ArtifactFlowEventSchema.options;

/**
 * Filter criteria for artifact flow triggers
 */
export const ArtifactFlowFilterSchema = z.object({
  /** Filter by artifact types */
  artifactTypes: z.array(ArtifactTypeSchema).optional(),
  /** Filter by source bucket */
  sourceBucket: z.string().optional(),
});

export type ArtifactFlowFilter = z.infer<typeof ArtifactFlowFilterSchema>;

/**
 * The trigger configuration for an artifact flow
 */
export const ArtifactFlowTriggerSchema = z.object({
  /** The event that triggers this flow */
  event: ArtifactFlowEventSchema,
  /** Optional filter to narrow the trigger */
  filter: ArtifactFlowFilterSchema.optional(),
});

export type ArtifactFlowTrigger = z.infer<typeof ArtifactFlowTriggerSchema>;

/**
 * A copy step in an artifact flow
 */
export const ArtifactFlowStepCopySchema = z.object({
  type: z.literal("copy"),
  /** The target bucket ID */
  toBucket: z.string(),
});

export type ArtifactFlowStepCopy = z.infer<typeof ArtifactFlowStepCopySchema>;

/**
 * A spawn_process step in an artifact flow
 */
export const ArtifactFlowStepSpawnProcessSchema = z.object({
  type: z.literal("spawn_process"),
  /** The type of process to spawn */
  processType: z.string(),
  /** The agent profile to use for the process */
  agentProfile: z.string(),
});

export type ArtifactFlowStepSpawnProcess = z.infer<
  typeof ArtifactFlowStepSpawnProcessSchema
>;

/**
 * A step in an artifact flow
 */
export const ArtifactFlowStepSchema = z.discriminatedUnion("type", [
  ArtifactFlowStepCopySchema,
  ArtifactFlowStepSpawnProcessSchema,
]);

export type ArtifactFlowStep = z.infer<typeof ArtifactFlowStepSchema>;

/**
 * An artifact flow - automates artifact routing between processes
 */
export const ArtifactFlowSchema = z.object({
  /** Unique identifier */
  id: z.string(),
  /** Display name */
  name: z.string(),
  /** The trigger configuration */
  trigger: ArtifactFlowTriggerSchema,
  /** The steps to execute when triggered */
  steps: z.array(ArtifactFlowStepSchema),
  /** Whether this flow is active */
  isActive: z.boolean().default(true),
  /** When the flow was created (ISO 8601 string) */
  createdAt: z.string(),
});

export type ArtifactFlow = z.infer<typeof ArtifactFlowSchema>;

// ============================================
// System Buckets
// ============================================

/**
 * The 4 system buckets defined in the PRD
 */
export const SYSTEM_BUCKETS: readonly ArtifactBucket[] = [
  {
    id: "research-outputs",
    name: "Research Outputs",
    acceptedTypes: ["research_document", "findings", "recommendations"],
    writers: [AGENT_DEEP_RESEARCHER, AGENT_ORCHESTRATOR],
    readers: ["all"],
    isSystem: true,
  },
  {
    id: "work-context",
    name: "Work Context",
    acceptedTypes: ["context", "task_spec", "previous_work"],
    writers: [AGENT_ORCHESTRATOR, "system"],
    readers: ["all"],
    isSystem: true,
  },
  {
    id: "code-changes",
    name: "Code Changes",
    acceptedTypes: ["code_change", "diff", "test_result"],
    writers: [AGENT_WORKER],
    readers: ["all"],
    isSystem: true,
  },
  {
    id: "prd-library",
    name: "PRD Library",
    acceptedTypes: ["prd", "specification", "design_doc"],
    writers: [AGENT_ORCHESTRATOR, "user"],
    readers: ["all"],
    isSystem: true,
  },
] as const;

/**
 * Get a system bucket by ID
 */
export function getSystemBucket(id: string): ArtifactBucket | undefined {
  return SYSTEM_BUCKETS.find((b) => b.id === id);
}

// ============================================
// Artifact Version Summary
// ============================================

/**
 * Summary of a single artifact version for history display.
 * Fields are snake_case matching backend ArtifactVersionSummaryResponse (no serde rename_all).
 */
export const ArtifactVersionSummarySchema = z.object({
  id: z.string(),
  version: z.number(),
  name: z.string(),
  created_at: z.string(),
  created_by: z.string().default("system"),
  metadata: z.record(z.string(), z.unknown()).nullable().default(null),
});

export type ArtifactVersionSummary = z.infer<typeof ArtifactVersionSummarySchema>;
