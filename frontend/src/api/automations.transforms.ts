import type { z } from "zod";

import type {
  Automation,
  AutomationDetail,
  AutomationRun,
  AutomationRunScopedInput,
  AutomationScheduleResponse,
  CreateAutomationDraftResponse,
  PauseAutomationInput,
  UpdateAutomationSettingsInput,
  UpdateAutomationSetupInput,
} from "./automations.types";
import {
  AutomationDetailSchema,
  AutomationRunSchema,
  AutomationScheduleResponseSchema,
  AutomationSchema,
  AutomationUsageSchema,
  CreateAutomationDraftResponseSchema,
} from "./automations.schemas";

type RawAutomation = z.infer<typeof AutomationSchema>;
type RawAutomationRun = z.infer<typeof AutomationRunSchema>;
type RawAutomationUsage = z.infer<typeof AutomationUsageSchema>;
type RawAutomationDetail = z.infer<typeof AutomationDetailSchema>;
type RawCreateAutomationDraftResponse = z.infer<
  typeof CreateAutomationDraftResponseSchema
>;
type RawAutomationScheduleResponse = z.infer<
  typeof AutomationScheduleResponseSchema
>;

export function transformAutomation(raw: RawAutomation): Automation {
  return {
    id: raw.id,
    projectId: raw.project_id,
    name: raw.name,
    status: raw.status,
    pausedReasonCode: raw.paused_reason_code,
    pausedReasonDetail: raw.paused_reason_detail,
    goalPrompt: raw.goal_prompt,
    setupConversationId: raw.setup_conversation_id,
    specArtifactId: raw.spec_artifact_id,
    authoringMode: raw.authoring_mode,
    decompositionVerificationStatus: raw.decomposition_verification_status,
    decompositionVerificationVerdictJson:
      raw.decomposition_verification_verdict_json,
    providerHarness: raw.provider_harness,
    modelId: raw.model_id,
    logicalEffort: raw.logical_effort,
    runMode: raw.run_mode,
    baseRefKind: raw.base_ref_kind,
    baseRef: raw.base_ref,
    baseDisplayName: raw.base_display_name,
    ...(raw.base_target_ref != null && { baseTargetRef: raw.base_target_ref }),
    ...(raw.base_target_display_name != null && {
      baseTargetDisplayName: raw.base_target_display_name,
    }),
    baseSourcePullRequestJson: raw.base_source_pull_request_json,
    goalItemsJson: raw.goal_items_json,
    chainMode: raw.chain_mode,
    completionSignal: raw.completion_signal,
    planApprovalMode: raw.plan_approval_mode,
    prMergeMode: raw.pr_merge_mode,
    planDeepVerification: raw.plan_deep_verification,
    maxRuns: raw.max_runs,
    maxConsecutiveFailures: raw.max_consecutive_failures,
    firstRunPrompt: raw.first_run_prompt,
    setupAnalysisSummary: raw.setup_analysis_summary,
    createdAt: raw.created_at,
    updatedAt: raw.updated_at,
  };
}

export function transformAutomationRun(raw: RawAutomationRun): AutomationRun {
  return {
    id: raw.id,
    automationId: raw.automation_id,
    runIndex: raw.run_index,
    status: raw.status,
    judgeState: raw.judge_state,
    judgeLeaseExpiresAt: raw.judge_lease_expires_at,
    planJudgeState: raw.plan_judge_state,
    planRevisionRound: raw.plan_revision_round,
    planRevisionPending: raw.plan_revision_pending,
    planPhase: raw.plan_phase,
    planArtifactId: raw.plan_artifact_id,
    planBlueprintArtifactId: raw.plan_blueprint_artifact_id,
    parkedPlanArtifactId: raw.parked_plan_artifact_id,
    parkedPlanBlueprintArtifactId: raw.parked_plan_blueprint_artifact_id,
    planApprovedBy: raw.plan_approved_by,
    planApprovedArtifactVersion: raw.plan_approved_artifact_version,
    planApprovedAt: raw.plan_approved_at,
    conversationId: raw.conversation_id,
    runPrompt: raw.run_prompt,
    promptAuthor: raw.prompt_author,
    baseRefKind: raw.base_ref_kind,
    baseRefUsed: raw.base_ref_used,
    baseFromRunId: raw.base_from_run_id,
    goalItemId: raw.goal_item_id,
    branchName: raw.branch_name,
    prNumber: raw.pr_number,
    prUrl: raw.pr_url,
    prTitle: raw.pr_title,
    prHeadRefName: raw.pr_head_ref_name,
    prBaseRefName: raw.pr_base_ref_name,
    prMergedAt: raw.pr_merged_at,
    mergeCommitSha: raw.merge_commit_sha,
    diffStatsJson: raw.diff_stats_json,
    agentSummary: raw.agent_summary,
    judgeVerdictJson: raw.judge_verdict_json,
    judgeModelId: raw.judge_model_id,
    errorCode: raw.error_code,
    errorDetail: raw.error_detail,
    signalCheckFailures: raw.signal_check_failures,
    startedAt: raw.started_at,
    finishedAt: raw.finished_at,
    createdAt: raw.created_at,
    updatedAt: raw.updated_at,
  };
}

function transformAutomationUsage(raw: RawAutomationUsage) {
  return {
    inputTokens: raw.input_tokens,
    outputTokens: raw.output_tokens,
    cacheCreationTokens: raw.cache_creation_tokens,
    cacheReadTokens: raw.cache_read_tokens,
    estimatedUsd: raw.estimated_usd,
  };
}

function transformAutomationPipeline(
  raw: NonNullable<RawAutomationDetail["pipeline"]>,
) {
  return {
    deliverable: raw.deliverable,
    status: raw.status,
    ideationSessionId: raw.ideation_session_id,
    planArtifactId: raw.plan_artifact_id,
    proposalCount: raw.proposal_count,
    taskTotal: raw.task_total,
    taskMerged: raw.task_merged,
    taskTerminal: raw.task_terminal,
    tasks: raw.tasks.map((task) => ({
      id: task.id,
      title: task.title,
      status: task.status,
      blockedBy: task.blocked_by,
    })),
  };
}

export function transformAutomationDetail(
  raw: RawAutomationDetail,
): AutomationDetail {
  return {
    automation: transformAutomation(raw.automation),
    runs: raw.runs.map(transformAutomationRun),
    usage: transformAutomationUsage(raw.usage),
    pipeline: raw.pipeline ? transformAutomationPipeline(raw.pipeline) : null,
  };
}

export function transformCreateAutomationDraftResponse(
  raw: RawCreateAutomationDraftResponse,
): CreateAutomationDraftResponse {
  return {
    automation: transformAutomation(raw.automation),
    setupConversationId: raw.setup_conversation_id,
  };
}

export function transformAutomationScheduleResponse(
  raw: RawAutomationScheduleResponse,
): AutomationScheduleResponse {
  return {
    scheduled: raw.scheduled,
    reason: raw.reason,
  };
}

export function transformUpdateAutomationSettingsInput(
  input: UpdateAutomationSettingsInput,
): Record<string, unknown> {
  return {
    id: input.id,
    ...(input.name !== undefined && { name: input.name }),
    ...(input.maxRuns !== undefined && { maxRuns: input.maxRuns }),
    ...(input.maxConsecutiveFailures !== undefined && {
      maxConsecutiveFailures: input.maxConsecutiveFailures,
    }),
    ...(input.planApprovalMode !== undefined && {
      planApprovalMode: input.planApprovalMode,
    }),
    ...(input.prMergeMode !== undefined && { prMergeMode: input.prMergeMode }),
    ...(input.planDeepVerification !== undefined && {
      planDeepVerification: input.planDeepVerification,
    }),
  };
}

export function transformAutomationRunScopedInput(
  input: AutomationRunScopedInput,
): Record<string, unknown> {
  return {
    id: input.id,
    runId: input.runId,
  };
}

export function transformPauseAutomationInput(
  input: PauseAutomationInput,
): Record<string, unknown> {
  return {
    id: input.id,
    ...(input.reasonCode !== undefined && { reasonCode: input.reasonCode }),
    ...(input.reasonDetail !== undefined && { reasonDetail: input.reasonDetail }),
  };
}

export function transformUpdateAutomationSetupInput(
  input: UpdateAutomationSetupInput,
): Record<string, unknown> {
  return {
    ...(input.name !== undefined && { name: input.name }),
    ...(input.maxRuns !== undefined && { max_runs: input.maxRuns }),
    ...(input.maxConsecutiveFailures !== undefined && {
      max_consecutive_failures: input.maxConsecutiveFailures,
    }),
    ...(input.goalPrompt !== undefined && { goal_prompt: input.goalPrompt }),
    ...(input.firstRunPrompt !== undefined && {
      first_run_prompt: input.firstRunPrompt,
    }),
    ...(input.providerHarness !== undefined && {
      provider_harness: input.providerHarness,
    }),
    ...(input.modelId !== undefined && { model_id: input.modelId }),
    ...(input.logicalEffort !== undefined && {
      logical_effort: input.logicalEffort,
    }),
    ...(input.runMode !== undefined && { run_mode: input.runMode }),
    ...(input.baseRefKind !== undefined && { base_ref_kind: input.baseRefKind }),
    ...(input.baseRef !== undefined && { base_ref: input.baseRef }),
    ...(input.baseDisplayName !== undefined && {
      base_display_name: input.baseDisplayName,
    }),
    ...(input.goalItemsJson !== undefined && {
      goal_items_json: input.goalItemsJson,
    }),
    ...(input.chainMode !== undefined && { chain_mode: input.chainMode }),
    ...(input.completionSignal !== undefined && {
      completion_signal: input.completionSignal,
    }),
    ...(input.setupAnalysisSummary !== undefined && {
      setup_analysis_summary: input.setupAnalysisSummary,
    }),
    ...(input.specArtifactId !== undefined && {
      spec_artifact_id: input.specArtifactId,
    }),
  };
}
