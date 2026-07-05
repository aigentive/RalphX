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
  | "published"
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

export type AutomationCompletionSignal = "pr_merged";

export interface Automation {
  id: string;
  projectId: string;
  name: string;
  status: AutomationStatus;
  pausedReasonCode: string | null;
  pausedReasonDetail: string | null;
  goalPrompt: string;
  setupConversationId: string | null;
  providerHarness: string;
  modelId: string;
  logicalEffort: string | null;
  runMode: AutomationRunMode;
  baseRefKind: AutomationBaseRefKind;
  baseRef: string;
  baseDisplayName: string | null;
  baseSourcePullRequestJson: string | null;
  goalItemsJson: string | null;
  chainMode: AutomationChainMode;
  completionSignal: AutomationCompletionSignal;
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
  conversationId: string | null;
  runPrompt: string;
  promptAuthor: AutomationPromptAuthor;
  baseRefKind: AutomationBaseRefKind;
  baseRefUsed: string;
  baseFromRunId: string | null;
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

export interface AutomationDetail {
  automation: Automation;
  runs: AutomationRun[];
  usage: AutomationUsage;
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
}

export interface UpdateAutomationSettingsInput {
  id: string;
  name?: string | undefined;
  maxRuns?: number | undefined;
  maxConsecutiveFailures?: number | undefined;
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
}
