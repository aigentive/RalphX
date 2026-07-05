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
  CreateAutomationDraftResponseSchema,
} from "./automations.schemas";

type RawAutomation = z.infer<typeof AutomationSchema>;
type RawAutomationRun = z.infer<typeof AutomationRunSchema>;
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
    providerHarness: raw.provider_harness,
    modelId: raw.model_id,
    logicalEffort: raw.logical_effort,
    runMode: raw.run_mode,
    baseRefKind: raw.base_ref_kind,
    baseRef: raw.base_ref,
    baseDisplayName: raw.base_display_name,
    baseSourcePullRequestJson: raw.base_source_pull_request_json,
    goalItemsJson: raw.goal_items_json,
    chainMode: raw.chain_mode,
    completionSignal: raw.completion_signal,
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
    conversationId: raw.conversation_id,
    runPrompt: raw.run_prompt,
    promptAuthor: raw.prompt_author,
    baseRefKind: raw.base_ref_kind,
    baseRefUsed: raw.base_ref_used,
    baseFromRunId: raw.base_from_run_id,
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

export function transformAutomationDetail(
  raw: RawAutomationDetail,
): AutomationDetail {
  return {
    automation: transformAutomation(raw.automation),
    runs: raw.runs.map(transformAutomationRun),
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
  };
}
