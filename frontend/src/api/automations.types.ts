import type { AgentConversationBaseSelection } from "./chat";

export type AutomationStatus =
  | "draft"
  | "active"
  | "paused"
  | "completed"
  | "stopped";

export type AutomationRunStatus =
  | "pending"
  | "provisioning"
  | "running"
  | "awaiting_plan_approval"
  | "published"
  | "completed"
  | "merged"
  | "pr_closed"
  | "agent_failed"
  | "cancelled";

export type AutomationJudgeState =
  | "none"
  | "in_progress"
  | "done"
  | "failed"
  | "skipped";

export type AutomationPlanApprovalMode = "manual" | "automatic";

export type AutomationPrMergeMode = "manual" | "automatic";

export type AutomationAuthoringMode = "reviewed" | "trusted_auto_finalize";

export type AutomationDecompositionVerificationStatus =
  | "unverified"
  | "verified"
  | "needs_revision"
  | "failed";

export type AutomationPlanJudgeState =
  | "none"
  | "in_progress"
  | "done"
  | "failed";

export type AutomationPromptAuthor =
  | "setup_agent"
  | "judge"
  | "skip_judge_template";

export type AutomationRunMode = "edit" | "plan" | "ideation";

export type AutomationBaseRefKind =
  | "project_default"
  | "current_branch"
  | "local_branch"
  | "pull_request";

export type AutomationChainMode = "merged_base" | "pr_head_stacked";

export type AutomationCompletionSignal =
  | "pr_merged"
  | "agent_completed"
  | "ideation_finalized";

export interface Automation {
  id: string;
  projectId: string;
  name: string;
  status: AutomationStatus;
  pausedReasonCode: string | null;
  pausedReasonDetail: string | null;
  goalPrompt: string;
  setupConversationId: string | null;
  specArtifactId: string | null;
  authoringMode: AutomationAuthoringMode;
  decompositionVerificationStatus: AutomationDecompositionVerificationStatus;
  decompositionVerificationVerdictJson: string | null;
  providerHarness: string;
  modelId: string;
  logicalEffort: string | null;
  runMode: AutomationRunMode;
  baseRefKind: AutomationBaseRefKind;
  baseRef: string;
  baseDisplayName: string | null;
  // Final merge target (fork point, e.g. `main`) when the automation runs on its own
  // integration branch (`baseRef`); populated on the detail response only.
  baseTargetRef?: string;
  baseTargetDisplayName?: string;
  baseSourcePullRequestJson: string | null;
  goalItemsJson: string | null;
  chainMode: AutomationChainMode;
  completionSignal: AutomationCompletionSignal;
  planApprovalMode: AutomationPlanApprovalMode;
  prMergeMode: AutomationPrMergeMode;
  planDeepVerification: boolean;
  maxRuns: number;
  maxConsecutiveFailures: number;
  firstRunPrompt: string | null;
  setupAnalysisSummary: string | null;
  createdAt: string;
  updatedAt: string;
}

export interface AutomationRun {
  id: string;
  automationId: string;
  runIndex: number;
  status: AutomationRunStatus;
  judgeState: AutomationJudgeState;
  judgeLeaseExpiresAt: string | null;
  planJudgeState: AutomationPlanJudgeState;
  planRevisionRound: number;
  planRevisionPending: boolean;
  planPhase: boolean;
  planArtifactId: string | null;
  planBlueprintArtifactId: string | null;
  parkedPlanArtifactId: string | null;
  parkedPlanBlueprintArtifactId: string | null;
  planApprovedBy: string | null;
  planApprovedArtifactVersion: number | null;
  planApprovedAt: string | null;
  conversationId: string | null;
  runPrompt: string;
  promptAuthor: AutomationPromptAuthor;
  baseRefKind: AutomationBaseRefKind;
  baseRefUsed: string;
  baseFromRunId: string | null;
  goalItemId: string | null;
  branchName: string | null;
  prNumber: number | null;
  prUrl: string | null;
  prTitle: string | null;
  prHeadRefName: string | null;
  prBaseRefName: string | null;
  prMergedAt: string | null;
  mergeCommitSha: string | null;
  diffStatsJson: string | null;
  agentSummary: string | null;
  judgeVerdictJson: string | null;
  judgeModelId: string | null;
  errorCode: string | null;
  errorDetail: string | null;
  signalCheckFailures: number;
  startedAt: string | null;
  finishedAt: string | null;
  createdAt: string;
  updatedAt: string;
}

export interface AutomationUsage {
  inputTokens: number;
  outputTokens: number;
  cacheCreationTokens: number;
  cacheReadTokens: number;
  estimatedUsd: number | null;
}

export interface AutomationPipelineTask {
  id: string;
  title: string;
  status: string;
  blockedBy: string[];
}

export interface AutomationPipelineProgress {
  deliverable: "task_graph";
  status: "authoring" | "executing" | "completed" | "attention";
  ideationSessionId: string;
  planArtifactId: string | null;
  proposalCount: number;
  taskTotal: number;
  taskMerged: number;
  taskTerminal: number;
  tasks: AutomationPipelineTask[];
}

export interface AutomationDetail {
  automation: Automation;
  runs: AutomationRun[];
  usage: AutomationUsage;
  pipeline?: AutomationPipelineProgress | null;
}

export interface CreateAutomationDraftResponse {
  automation: Automation;
  setupConversationId: string | null;
}

export interface AutomationScheduleResponse {
  scheduled: boolean;
  reason: string | null;
}

export interface ListAutomationsInput {
  projectId?: string | null | undefined;
}

export interface CreateAutomationDraftInput {
  projectId: string;
  name?: string | undefined;
  authoringMode?: AutomationAuthoringMode | undefined;
  base?: AgentConversationBaseSelection | undefined;
}

export interface UpdateAutomationSettingsInput {
  id: string;
  name?: string | undefined;
  maxRuns?: number | undefined;
  maxConsecutiveFailures?: number | undefined;
  planApprovalMode?: AutomationPlanApprovalMode | undefined;
  prMergeMode?: AutomationPrMergeMode | undefined;
  planDeepVerification?: boolean | undefined;
}

export interface PauseAutomationInput {
  id: string;
  reasonCode?: string | undefined;
  reasonDetail?: string | undefined;
}

export interface AutomationRunScopedInput {
  id: string;
  runId: string;
}

export interface UpdateAutomationSetupInput {
  name?: string | undefined;
  maxRuns?: number | undefined;
  maxConsecutiveFailures?: number | undefined;
  goalPrompt?: string | undefined;
  firstRunPrompt?: string | undefined;
  providerHarness?: string | undefined;
  modelId?: string | undefined;
  logicalEffort?: string | null | undefined;
  runMode?: AutomationRunMode | undefined;
  baseRefKind?: AutomationBaseRefKind | undefined;
  baseRef?: string | undefined;
  baseDisplayName?: string | undefined;
  goalItemsJson?: string | undefined;
  chainMode?: AutomationChainMode | undefined;
  completionSignal?: AutomationCompletionSignal | undefined;
  setupAnalysisSummary?: string | undefined;
  // No `| null`: the backend `update_config` uses COALESCE and cannot clear a
  // linked spec. v1 re-authoring replaces (new version), it does not unlink.
  specArtifactId?: string | undefined;
}
