import type {
  Automation,
  AutomationDetail,
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
