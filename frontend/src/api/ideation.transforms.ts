// Transform functions for converting snake_case API responses to camelCase frontend types

import { z } from "zod";
import type {
  IdeationSettings,
  IdeationSettingsResponse,
} from "../types/ideation-config";
import type {
  IdeationSessionStatus,
  SessionListResponse,
  TaskProposal,
  Priority,
  Complexity,
  ProposalStatus,
} from "../types/ideation";
import type {
  IdeationSessionResponse,
  TaskProposalResponse,
  ChatMessageResponse,
  SessionWithDataResponse,
  PriorityAssessmentResponse,
  DependencyGraphResponse,
  ApplyProposalsResultResponse,
  RestartImplementationResultResponse,
  CreateChildSessionResponse,
  ParentSessionContextResponse,
} from "./ideation.types";
import {
  IdeationSessionResponseSchema,
  TaskProposalResponseSchema,
  ChatMessageResponseSchema,
  SessionWithDataResponseSchema,
  PriorityAssessmentResponseSchema,
  DependencyGraphResponseSchema,
  ApplyProposalsResultResponseSchema,
  RestartImplementationResultResponseSchema,
  CreateChildSessionResponseSchema,
  ParentSessionContextResponseSchema,
  SessionListResponseSchema,
} from "./ideation.schemas";

export function transformSession(raw: z.infer<typeof IdeationSessionResponseSchema>): IdeationSessionResponse {
  return {
    id: raw.id,
    projectId: raw.project_id,
    title: raw.title,
    titleSource: raw.title_source ?? null,
    status: raw.status as IdeationSessionStatus,
    planArtifactId: raw.plan_artifact_id,
    inheritedPlanArtifactId: raw.inherited_plan_artifact_id ?? null,
    seedTaskId: raw.seed_task_id ?? null,
    parentSessionId: raw.parent_session_id,
    createdAt: raw.created_at,
    updatedAt: raw.updated_at,
    archivedAt: raw.archived_at,
    convertedAt: raw.converted_at,
    verificationStatus: (raw.verification_status ?? "unverified") as import("../types/ideation").VerificationStatus,
    verificationInProgress: raw.verification_in_progress ?? false,
    gapScore: raw.gap_score ?? null,
    sourceProjectId: raw.source_project_id ?? null,
    sourceSessionId: raw.source_session_id ?? null,
    sourceTaskId: raw.source_task_id ?? null,
    sourceContextType: raw.source_context_type ?? null,
    sourceContextId: raw.source_context_id ?? null,
    spawnReason: raw.spawn_reason ?? null,
    blockerFingerprint: raw.blocker_fingerprint ?? null,
    sessionPurpose: raw.session_purpose ?? "general",
    sessionFlow: raw.session_flow ?? "ideation",
    acceptanceStatus: raw.acceptance_status ?? null,
    analysisBaseRefKind: raw.analysis_base_ref_kind ?? null,
    analysisBaseRef: raw.analysis_base_ref ?? null,
    analysisBaseDisplayName: raw.analysis_base_display_name ?? null,
    analysisWorkspaceKind: raw.analysis_workspace_kind ?? "project_root",
    analysisWorkspacePath: raw.analysis_workspace_path ?? null,
    analysisBaseCommit: raw.analysis_base_commit ?? null,
    analysisBaseLockedAt: raw.analysis_base_locked_at ?? null,
    lastEffectiveModel: raw.last_effective_model ?? null,
  };
}

export function transformSessionList(
  raw: z.infer<typeof SessionListResponseSchema>
): SessionListResponse {
  return {
    sessions: raw.sessions.map((item) => ({
      ...transformSession(item),
      progress: item.progress
        ? {
            idle: item.progress.idle,
            active: item.progress.active,
            done: item.progress.done,
            total: item.progress.total,
          }
        : null,
      parentSessionTitle: item.parentSessionTitle,
      verificationChildCount: item.verificationChildCount,
      hasPendingPrompt: item.hasPendingPrompt,
    })),
    total: raw.total,
    hasMore: raw.hasMore,
    offset: raw.offset,
  };
}

export function transformProposal(raw: z.infer<typeof TaskProposalResponseSchema>): TaskProposalResponse {
  return {
    id: raw.id,
    sessionId: raw.session_id,
    title: raw.title,
    description: raw.description,
    category: raw.category,
    steps: raw.steps,
    acceptanceCriteria: raw.acceptance_criteria,
    suggestedPriority: raw.suggested_priority,
    priorityScore: raw.priority_score,
    priorityReason: raw.priority_reason,
    estimatedComplexity: raw.estimated_complexity,
    userPriority: raw.user_priority,
    userModified: raw.user_modified,
    status: raw.status,
    createdTaskId: raw.created_task_id,
    planArtifactId: raw.plan_artifact_id,
    planVersionAtCreation: raw.plan_version_at_creation,
    blueprintArtifactId: raw.blueprint_artifact_id ?? null,
    blueprintVersionAtCreation: raw.blueprint_version_at_creation ?? null,
    sortOrder: raw.sort_order,
    createdAt: raw.created_at,
    updatedAt: raw.updated_at,
  };
}

/**
 * Convert TaskProposalResponse to TaskProposal (store type)
 *
 * This function properly types the enum fields that come as strings from the API
 * to the branded enum types expected by the store.
 */
export function toTaskProposal(response: TaskProposalResponse): TaskProposal {
  return {
    id: response.id,
    sessionId: response.sessionId,
    title: response.title,
    description: response.description,
    category: response.category,
    steps: response.steps,
    acceptanceCriteria: response.acceptanceCriteria,
    suggestedPriority: response.suggestedPriority as Priority,
    priorityScore: response.priorityScore,
    priorityReason: response.priorityReason,
    estimatedComplexity: response.estimatedComplexity as Complexity,
    userPriority: response.userPriority as Priority | null,
    userModified: response.userModified,
    status: response.status as ProposalStatus,
    createdTaskId: response.createdTaskId,
    planArtifactId: response.planArtifactId,
    planVersionAtCreation: response.planVersionAtCreation,
    sortOrder: response.sortOrder,
    createdAt: response.createdAt,
    updatedAt: response.updatedAt,
  };
}

export function transformMessage(raw: z.infer<typeof ChatMessageResponseSchema>): ChatMessageResponse {
  return {
    id: raw.id,
    sessionId: raw.session_id,
    projectId: raw.project_id,
    taskId: raw.task_id,
    role: raw.role,
    content: raw.content,
    metadata: raw.metadata,
    toolCalls: raw.tool_calls,
    parentMessageId: raw.parent_message_id,
    createdAt: raw.created_at,
  };
}

export function transformSessionWithData(raw: z.infer<typeof SessionWithDataResponseSchema>): SessionWithDataResponse {
  return {
    session: transformSession(raw.session),
    proposals: raw.proposals.map(transformProposal),
    messages: raw.messages.map(transformMessage),
  };
}

export function transformPriorityAssessment(raw: z.infer<typeof PriorityAssessmentResponseSchema>): PriorityAssessmentResponse {
  return {
    proposalId: raw.proposal_id,
    priority: raw.priority,
    score: raw.score,
    reason: raw.reason,
  };
}

export function transformDependencyGraph(raw: z.infer<typeof DependencyGraphResponseSchema>): DependencyGraphResponse {
  return {
    nodes: raw.nodes.map((n) => ({
      proposalId: n.proposal_id,
      title: n.title,
      inDegree: n.in_degree,
      outDegree: n.out_degree,
    })),
    edges: raw.edges.map((e) => ({
      from: e.from,
      to: e.to,
      reason: e.reason ?? null,
    })),
    criticalPath: raw.critical_path,
    hasCycles: raw.has_cycles,
    cycles: raw.cycles,
    ...(raw.message !== undefined && { message: raw.message }),
    ...(raw.summary !== undefined && {
      summary: raw.summary
        ? {
            totalProposals: raw.summary.total_proposals,
            rootCount: raw.summary.root_count,
            leafCount: raw.summary.leaf_count,
            maxDepth: raw.summary.max_depth,
          }
        : null,
    }),
  };
}

export function transformApplyResult(raw: z.infer<typeof ApplyProposalsResultResponseSchema>): ApplyProposalsResultResponse {
  return {
    createdTaskIds: raw.created_task_ids,
    dependenciesCreated: raw.dependencies_created,
    ...(raw.tasks_created !== undefined && { tasksCreated: raw.tasks_created }),
    warnings: raw.warnings,
    sessionConverted: raw.session_converted,
    executionPlanId: raw.execution_plan_id ?? null,
    ...(raw.message !== undefined && { message: raw.message }),
  };
}

export function transformRestartImplementationResult(
  raw: z.infer<typeof RestartImplementationResultResponseSchema>,
): RestartImplementationResultResponse {
  return {
    sessionId: raw.session_id,
    oldExecutionPlanId: raw.old_execution_plan_id,
    executionPlanId: raw.execution_plan_id,
    archivedTaskCount: raw.archived_task_count,
    createdTaskIds: raw.created_task_ids,
  };
}

export function transformNullableBool(value: number | null | undefined): boolean | null {
  if (value === null || value === undefined) return null;
  return value !== 0;
}

export function transformIdeationSettings(raw: IdeationSettingsResponse): IdeationSettings {
  return {
    tasksEnabled: raw.tasks_enabled,
    autoVerifyDraftPlans: raw.auto_verify_draft_plans,
    tasksFeatureState: raw.tasks_feature_state,
    autoVerifyPlans: raw.auto_verify_plans,
    requireAcceptForFinalize: raw.require_accept_for_finalize,
    requireVerificationForAccept: raw.require_verification_for_accept,
    externalOverrides: {
      autoVerifyPlans: raw.external_overrides.auto_verify_plans,
      requireVerificationForAccept: raw.external_overrides.require_verification_for_accept,
      requireAcceptForFinalize: raw.external_overrides.require_accept_for_finalize,
    },
  };
}

export function transformParentSessionContext(
  raw: z.infer<typeof ParentSessionContextResponseSchema>
): ParentSessionContextResponse {
  return {
    parentSession: {
      id: raw.parent_session.id,
      title: raw.parent_session.title,
      status: raw.parent_session.status,
    },
    planContent: raw.plan_content,
    proposals: raw.proposals.map((p) => ({
      id: p.id,
      title: p.title,
      category: p.category,
      priority: p.priority,
      status: p.status,
      acceptanceCriteria: p.acceptance_criteria,
    })),
  };
}

export function transformCreateChildSession(
  raw: z.infer<typeof CreateChildSessionResponseSchema>
): CreateChildSessionResponse {
  return {
    sessionId: raw.session_id,
    parentSessionId: raw.parent_session_id,
    title: raw.title,
    status: raw.status,
    createdAt: raw.created_at,
    ...(raw.generation !== undefined && { generation: raw.generation }),
    parentContext: raw.parent_context ? transformParentSessionContext(raw.parent_context) : undefined,
  };
}
