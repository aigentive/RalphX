import type {
  Automation,
  AutomationDetail,
  AutomationRun,
  AutomationRunScopedInput,
  AutomationScheduleResponse,
  CreateAutomationDraftInput,
  CreateAutomationDraftResponse,
  ListAutomationsInput,
  PauseAutomationInput,
  UpdateAutomationSettingsInput,
  UpdateAutomationSetupInput,
} from "@/api/automations";

function mockAutomation(overrides: Partial<Automation> = {}): Automation {
  const now = new Date(0).toISOString();
  return {
    id: "mock-automation-1",
    projectId: "mock-project",
    name: "Mock automation",
    status: "draft",
    pausedReasonCode: null,
    pausedReasonDetail: null,
    goalPrompt: "",
    setupConversationId: null,
    providerHarness: "claude",
    modelId: "sonnet",
    logicalEffort: null,
    runMode: "edit",
    baseRefKind: "project_default",
    baseRef: "",
    baseDisplayName: null,
    baseSourcePullRequestJson: null,
    goalItemsJson: null,
    chainMode: "merged_base",
    completionSignal: "pr_merged",
    maxRuns: 25,
    maxConsecutiveFailures: 3,
    firstRunPrompt: null,
    setupAnalysisSummary: null,
    createdAt: now,
    updatedAt: now,
    ...overrides,
  };
}

function mockDetail(automation: Automation = mockAutomation()): AutomationDetail {
  return { automation, runs: [] };
}

function mockRun(overrides: Partial<AutomationRun> = {}): AutomationRun {
  const now = new Date(0).toISOString();
  return {
    id: "mock-automation-run-1",
    automationId: "mock-automation-1",
    runIndex: 1,
    status: "pending",
    judgeState: "none",
    judgeLeaseExpiresAt: null,
    conversationId: null,
    runPrompt: "Mock automation run",
    promptAuthor: "setup_agent",
    baseRefKind: "project_default",
    baseRefUsed: "",
    baseFromRunId: null,
    branchName: null,
    prNumber: null,
    prUrl: null,
    prTitle: null,
    prHeadRefName: null,
    prBaseRefName: null,
    prMergedAt: null,
    mergeCommitSha: null,
    diffStatsJson: null,
    agentSummary: null,
    judgeVerdictJson: null,
    judgeModelId: null,
    errorCode: null,
    errorDetail: null,
    signalCheckFailures: 0,
    startedAt: null,
    finishedAt: null,
    createdAt: now,
    updatedAt: now,
    ...overrides,
  };
}

function deferredSchedule(reason: string): AutomationScheduleResponse {
  return { scheduled: false, reason };
}

export const mockAutomationsApi = {
  list: async (_input: ListAutomationsInput = {}): Promise<Automation[]> => [],

  get: async (id: string): Promise<AutomationDetail> =>
    mockDetail(mockAutomation({ id })),

  createDraft: async (
    input: CreateAutomationDraftInput,
  ): Promise<CreateAutomationDraftResponse> => {
    const automation = mockAutomation({
      projectId: input.projectId,
      ...(input.name !== undefined && { name: input.name }),
    });
    return { automation, setupConversationId: automation.setupConversationId };
  },

  updateSettings: async (
    input: UpdateAutomationSettingsInput,
  ): Promise<Automation> =>
    mockAutomation({
      id: input.id,
      ...(input.name !== undefined && { name: input.name }),
      ...(input.maxRuns !== undefined && { maxRuns: input.maxRuns }),
      ...(input.maxConsecutiveFailures !== undefined && {
        maxConsecutiveFailures: input.maxConsecutiveFailures,
      }),
    }),

  pause: async (input: PauseAutomationInput): Promise<Automation> =>
    mockAutomation({
      id: input.id,
      status: "paused",
      pausedReasonCode: input.reasonCode ?? "user_paused",
      pausedReasonDetail: input.reasonDetail ?? null,
    }),

  resume: async (id: string): Promise<Automation> =>
    mockAutomation({ id, status: "active" }),

  stop: async (id: string): Promise<Automation> =>
    mockAutomation({ id, status: "stopped" }),

  triggerRunNow: async (_id: string): Promise<AutomationScheduleResponse> =>
    deferredSchedule(
      "automation run-now scheduling is implemented in a later scheduler phase",
    ),

  skipJudge: async (
    _input: AutomationRunScopedInput,
  ): Promise<AutomationScheduleResponse> =>
    deferredSchedule("skip-judge successor scheduling is implemented in the judge phase"),

  cancelRun: async (input: AutomationRunScopedInput): Promise<AutomationRun> =>
    mockRun({
      id: input.runId,
      automationId: input.id,
      status: "cancelled",
    }),

  delete: async (_id: string): Promise<void> => {},

  setupAgent: {
    getAutomation: async (_callerConversationId: string): Promise<AutomationDetail> =>
      mockDetail(),

    updateAutomation: async (
      _callerConversationId: string,
      input: UpdateAutomationSetupInput,
    ): Promise<Automation> =>
      mockAutomation({
        ...(input.name !== undefined && { name: input.name }),
        ...(input.maxRuns !== undefined && { maxRuns: input.maxRuns }),
        ...(input.maxConsecutiveFailures !== undefined && {
          maxConsecutiveFailures: input.maxConsecutiveFailures,
        }),
      }),

    finalizeAutomation: async (_callerConversationId: string): Promise<Automation> =>
      mockAutomation({ status: "active" }),
  },
} as const;
