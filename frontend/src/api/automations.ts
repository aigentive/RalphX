import {
  TauriVoidSchema,
  typedInvoke,
  typedInvokeWithTransform,
} from "@/lib/tauri";
import { backendFetch } from "@/api/backend";
import {
  getTransportEnvironmentId,
  isRemoteEnvironmentId,
} from "@/lib/remote/active-environment";
import { sourcePullRequestInvokeInput } from "./chat";

import {
  AutomationDetailSchema,
  AutomationListSchema,
  AutomationRunSchema,
  AutomationScheduleResponseSchema,
  AutomationSchema,
  CreateAutomationDraftResponseSchema,
} from "./automations.schemas";
import {
  transformAutomation,
  transformAutomationDetail,
  transformAutomationRun,
  transformAutomationRunScopedInput,
  transformAutomationScheduleResponse,
  transformCreateAutomationDraftResponse,
  transformPauseAutomationInput,
  transformUpdateAutomationSettingsInput,
  transformUpdateAutomationSetupInput,
} from "./automations.transforms";
import type {
  Automation,
  AutomationAuthoringMode,
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
} from "./automations.types";

export type {
  Automation,
  AutomationBaseRefKind,
  AutomationAuthoringMode,
  AutomationChainMode,
  AutomationCompletionSignal,
  AutomationDetail,
  AutomationDecompositionVerificationStatus,
  AutomationJudgeState,
  AutomationPlanApprovalMode,
  AutomationPlanJudgeState,
  AutomationPipelineProgress,
  AutomationPipelineTask,
  AutomationPrMergeMode,
  AutomationPromptAuthor,
  AutomationRun,
  AutomationRunMode,
  AutomationRunScopedInput,
  AutomationRunStatus,
  AutomationScheduleResponse,
  AutomationStatus,
  AutomationUsage,
  CreateAutomationDraftInput,
  CreateAutomationDraftResponse,
  ListAutomationsInput,
  PauseAutomationInput,
  UpdateAutomationSettingsInput,
  UpdateAutomationSetupInput,
} from "./automations.types";

export {
  AutomationBaseRefKindSchema,
  AutomationChainModeSchema,
  AutomationCompletionSignalSchema,
  AutomationDetailSchema,
  AutomationJudgeStateSchema,
  AutomationListSchema,
  AutomationPlanApprovalModeSchema,
  AutomationPlanJudgeStateSchema,
  AutomationPrMergeModeSchema,
  AutomationPromptAuthorSchema,
  AutomationScheduleResponseSchema,
  AutomationRunModeSchema,
  AutomationRunSchema,
  AutomationRunStatusSchema,
  AutomationSchema,
  AutomationStatusSchema,
  CreateAutomationDraftResponseSchema,
} from "./automations.schemas";

export {
  transformAutomation,
  transformAutomationDetail,
  transformAutomationRun,
  transformAutomationRunScopedInput,
  transformAutomationScheduleResponse,
  transformCreateAutomationDraftResponse,
  transformPauseAutomationInput,
  transformUpdateAutomationSettingsInput,
  transformUpdateAutomationSetupInput,
} from "./automations.transforms";

const CALLER_SESSION_ID_HEADER = "x-ralphx-caller-session-id";
const REMOTE_AUTOMATION_CONFIG_VERSION_CONFLICT =
  "REMOTE_AUTOMATION_CONFIG_VERSION_CONFLICT";

export class AutomationConfigVersionConflictError extends Error {
  readonly errorCode = REMOTE_AUTOMATION_CONFIG_VERSION_CONFLICT;

  constructor() {
    super("Automation changed — reload");
    this.name = "AutomationConfigVersionConflictError";
  }
}

function createDraftArgs(input: CreateAutomationDraftInput): {
  projectId: string;
  name?: string;
  authoringMode?: AutomationAuthoringMode;
  baseRefKind?: string;
  baseBranchMode?: string;
  baseRef?: string;
  baseDisplayName?: string;
  baseSourcePullRequest?: ReturnType<typeof sourcePullRequestInvokeInput>;
} {
  return {
    projectId: input.projectId,
    ...(input.name !== undefined && { name: input.name }),
    ...(input.authoringMode !== undefined && {
      authoringMode: input.authoringMode,
    }),
    ...(input.base
      ? {
          baseRefKind: input.base.kind,
          ...(input.base.branchMode
            ? { baseBranchMode: input.base.branchMode }
            : {}),
          baseRef: input.base.ref,
          baseDisplayName: input.base.displayName,
          ...(input.base.sourcePullRequest
            ? {
                baseSourcePullRequest: sourcePullRequestInvokeInput(
                  input.base.sourcePullRequest,
                ),
              }
            : {}),
        }
      : {}),
  };
}

async function postAutomationJson<TRaw, TResult>(
  endpoint: string,
  callerConversationId: string,
  schema: { parse: (value: unknown) => TRaw },
  transform: (raw: TRaw) => TResult,
  body?: Record<string, unknown>,
): Promise<TResult> {
  const response = await backendFetch(endpoint, {
    method: "POST",
    headers: {
      "Content-Type": "application/json",
      [CALLER_SESSION_ID_HEADER]: callerConversationId,
    },
    ...(body !== undefined && { body: JSON.stringify(body) }),
  });
  if (!response.ok) {
    let detail: string | null = null;
    try {
      const raw = (await response.json()) as {
        detail?: string;
        error?: string;
        message?: string;
      };
      detail = raw.detail ?? raw.message ?? raw.error ?? null;
    } catch {
      detail = null;
    }
    throw new Error(
      detail
        ? `Automation request failed: ${response.status} ${response.statusText}: ${detail}`
        : `Automation request failed: ${response.status} ${response.statusText}`,
    );
  }
  const raw = schema.parse(await response.json());
  return transform(raw);
}

export const automationsApi = {
  list: (input: ListAutomationsInput = {}): Promise<Automation[]> =>
    typedInvokeWithTransform(
      "list_automations",
      { input: input.projectId ? { projectId: input.projectId } : null },
      AutomationListSchema,
      (automations) => automations.map(transformAutomation),
    ),

  get: (id: string): Promise<AutomationDetail> =>
    typedInvokeWithTransform(
      "get_automation",
      { input: { id } },
      AutomationDetailSchema,
      transformAutomationDetail,
    ),

  createDraft: (
    input: CreateAutomationDraftInput,
  ): Promise<CreateAutomationDraftResponse> =>
    typedInvokeWithTransform(
      "create_automation_draft",
      { input: createDraftArgs(input) },
      CreateAutomationDraftResponseSchema,
      transformCreateAutomationDraftResponse,
    ),

  updateSettings: (input: UpdateAutomationSettingsInput): Promise<Automation> =>
    typedInvokeWithTransform(
      "update_automation_settings",
      { input: transformUpdateAutomationSettingsInput(input) },
      AutomationSchema,
      transformAutomation,
    ),

  pause: (input: PauseAutomationInput): Promise<Automation> =>
    typedInvokeWithTransform(
      "pause_automation",
      { input: transformPauseAutomationInput(input) },
      AutomationSchema,
      transformAutomation,
    ),

  resume: (id: string): Promise<Automation> =>
    typedInvokeWithTransform(
      "resume_automation",
      { input: { id } },
      AutomationSchema,
      transformAutomation,
    ),

  finalize: (id: string): Promise<Automation> =>
    typedInvokeWithTransform(
      "finalize_automation",
      { input: { id } },
      AutomationSchema,
      transformAutomation,
    ),

  stop: (id: string): Promise<Automation> =>
    typedInvokeWithTransform(
      "stop_automation",
      { input: { id } },
      AutomationSchema,
      transformAutomation,
    ),

  triggerRunNow: (id: string): Promise<AutomationScheduleResponse> =>
    typedInvokeWithTransform(
      "trigger_automation_run_now",
      { input: { id } },
      AutomationScheduleResponseSchema,
      transformAutomationScheduleResponse,
    ),

  restart: (id: string): Promise<AutomationScheduleResponse> =>
    typedInvokeWithTransform(
      "restart_automation",
      { input: { id } },
      AutomationScheduleResponseSchema,
      transformAutomationScheduleResponse,
    ),

  retryJudge: (id: string): Promise<AutomationScheduleResponse> =>
    typedInvokeWithTransform(
      "retry_automation_judge",
      { input: { id } },
      AutomationScheduleResponseSchema,
      transformAutomationScheduleResponse,
    ),

  retryPlanJudge: (id: string): Promise<AutomationScheduleResponse> =>
    typedInvokeWithTransform(
      "retry_automation_plan_judge",
      { input: { id } },
      AutomationScheduleResponseSchema,
      transformAutomationScheduleResponse,
    ),

  skipJudge: (
    input: AutomationRunScopedInput,
  ): Promise<AutomationScheduleResponse> =>
    typedInvokeWithTransform(
      "skip_automation_judge",
      { input: transformAutomationRunScopedInput(input) },
      AutomationScheduleResponseSchema,
      transformAutomationScheduleResponse,
    ),

  cancelRun: (input: AutomationRunScopedInput): Promise<AutomationRun> =>
    typedInvokeWithTransform(
      "cancel_automation_run",
      { input: transformAutomationRunScopedInput(input) },
      AutomationRunSchema,
      transformAutomationRun,
    ),

  deleteRun: async (input: AutomationRunScopedInput): Promise<void> => {
    await typedInvoke(
      "delete_automation_run",
      { input: transformAutomationRunScopedInput(input) },
      TauriVoidSchema,
    );
  },

  resumeRun: async (input: AutomationRunScopedInput): Promise<void> => {
    await typedInvoke(
      "resume_automation_run",
      { input: transformAutomationRunScopedInput(input) },
      TauriVoidSchema,
    );
  },

  delete: async (id: string): Promise<void> => {
    await typedInvoke("delete_automation", { input: { id } }, TauriVoidSchema);
  },

  setupAgent: {
    getAutomation: (callerConversationId: string): Promise<AutomationDetail> =>
      postAutomationJson(
        "get_automation",
        callerConversationId,
        AutomationDetailSchema,
        transformAutomationDetail,
      ),

    updateAutomation: async (
      callerConversationId: string,
      automation: Automation,
      input: UpdateAutomationSetupInput,
    ): Promise<Automation> => {
      const transformed = transformUpdateAutomationSetupInput(input);
      if (isRemoteEnvironmentId(getTransportEnvironmentId())) {
        const hasSettings =
          input.name !== undefined ||
          input.maxRuns !== undefined ||
          input.maxConsecutiveFailures !== undefined;
        const hasConfig =
          input.goalPrompt !== undefined ||
          input.firstRunPrompt !== undefined ||
          input.providerHarness !== undefined ||
          input.modelId !== undefined ||
          input.logicalEffort !== undefined ||
          input.runMode !== undefined ||
          input.baseRefKind !== undefined ||
          input.baseRef !== undefined ||
          input.baseDisplayName !== undefined ||
          input.goalItemsJson !== undefined ||
          input.chainMode !== undefined ||
          input.completionSignal !== undefined ||
          input.setupAnalysisSummary !== undefined ||
          input.specArtifactId !== undefined;
        try {
          return await typedInvokeWithTransform(
            "update_automation_config",
            {
              input: {
                automationId: automation.id,
                expectedUpdatedAt: automation.updatedAt,
                ...(hasSettings && {
                  settings: {
                    ...(input.name !== undefined && { name: input.name }),
                    ...(input.maxRuns !== undefined && { maxRuns: input.maxRuns }),
                    ...(input.maxConsecutiveFailures !== undefined && {
                      maxConsecutiveFailures: input.maxConsecutiveFailures,
                    }),
                  },
                }),
                ...(hasConfig && {
                  config: {
                    ...(input.goalPrompt !== undefined && {
                      goalPrompt: input.goalPrompt,
                    }),
                    ...(input.firstRunPrompt !== undefined && {
                      firstRunPrompt: input.firstRunPrompt,
                    }),
                    ...(input.providerHarness !== undefined && {
                      providerHarness: input.providerHarness,
                    }),
                    ...(input.modelId !== undefined && { modelId: input.modelId }),
                    ...(input.logicalEffort !== undefined && {
                      logicalEffort: input.logicalEffort,
                    }),
                    ...(input.runMode !== undefined && { runMode: input.runMode }),
                    ...(input.baseRefKind !== undefined && {
                      baseRefKind: input.baseRefKind,
                    }),
                    ...(input.baseRef !== undefined && { baseRef: input.baseRef }),
                    ...(input.baseDisplayName !== undefined && {
                      baseDisplayName: input.baseDisplayName,
                    }),
                    ...(input.goalItemsJson !== undefined && {
                      goalItemsJson: input.goalItemsJson,
                    }),
                    ...(input.chainMode !== undefined && {
                      chainMode: input.chainMode,
                    }),
                    ...(input.completionSignal !== undefined && {
                      completionSignal: input.completionSignal,
                    }),
                    ...(input.setupAnalysisSummary !== undefined && {
                      setupAnalysisSummary: input.setupAnalysisSummary,
                    }),
                    ...(input.specArtifactId !== undefined && {
                      specArtifactId: input.specArtifactId,
                    }),
                  },
                }),
              },
            },
            AutomationSchema,
            transformAutomation,
          );
        } catch (error) {
          if (String(error).includes(REMOTE_AUTOMATION_CONFIG_VERSION_CONFLICT)) {
            throw new AutomationConfigVersionConflictError();
          }
          throw error;
        }
      }
      return postAutomationJson(
        "update_automation",
        callerConversationId,
        AutomationSchema,
        transformAutomation,
        transformed,
      );
    },

    finalizeAutomation: (callerConversationId: string): Promise<Automation> =>
      postAutomationJson(
        "finalize_automation",
        callerConversationId,
        AutomationSchema,
        transformAutomation,
      ),
  },
} as const;
