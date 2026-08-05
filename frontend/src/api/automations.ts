import { z } from "zod";

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
const REMOTE_AUTOMATION_POLL_INTERVAL_MS = 400;
const REMOTE_AUTOMATION_MAX_POLLS = 60;
const REMOTE_AUTOMATION_TERMINAL_STATUSES = [
  "completed",
  "failed",
  "failedStale",
] as const;

const RemoteAutomationRequestStatusSchema = z.enum([
  "pending",
  "starting",
  ...REMOTE_AUTOMATION_TERMINAL_STATUSES,
]);
const RemoteAutomationRunResultSchema = z
  .object({
    scheduled: z.boolean(),
    reason: z.string().nullable().optional(),
    code: z.string().optional(),
    benign: z.boolean().optional(),
  })
  .passthrough();
const RequestRemoteAutomationRunResponseSchema = z
  .object({
    requestId: z.string(),
    status: RemoteAutomationRequestStatusSchema,
    deduplicated: z.boolean(),
    createdAt: z.string(),
  })
  .strict();
const GetRemoteAutomationRunRequestResponseSchema = z
  .object({
    requestId: z.string(),
    status: RemoteAutomationRequestStatusSchema,
    errorCode: z.string().nullable(),
    result: RemoteAutomationRunResultSchema.nullable(),
    createdAt: z.string(),
    updatedAt: z.string(),
  })
  .strict();
const RequestRemoteAutomationDraftResponseSchema = z
  .object({
    requestId: z.string(),
    automationId: z.string(),
    status: RemoteAutomationRequestStatusSchema,
    deduplicated: z.boolean(),
    createdAt: z.string(),
  })
  .strict();
const GetRemoteAutomationDraftRequestResponseSchema = z
  .object({
    requestId: z.string(),
    automationId: z.string(),
    status: RemoteAutomationRequestStatusSchema,
    errorCode: z.string().nullable(),
    result: z.object({ automationId: z.string() }).passthrough().nullable(),
    createdAt: z.string(),
    updatedAt: z.string(),
  })
  .strict();

function isRemoteAutomationTerminal(status: string): boolean {
  return (REMOTE_AUTOMATION_TERMINAL_STATUSES as readonly string[]).includes(
    status,
  );
}

function remoteErrorCode(error: unknown): string | null {
  return typeof error === "string"
    ? error
    : error instanceof Error
      ? error.message
      : null;
}

async function pollRemoteAutomationRequest<T>(
  command: string,
  requestId: string,
  initialStatus: z.infer<typeof RemoteAutomationRequestStatusSchema>,
  schema: z.ZodType<T & { status: string }>,
): Promise<T & { status: string }> {
  let request: (T & { status: string }) | null = null;
  for (let attempt = 0; attempt < REMOTE_AUTOMATION_MAX_POLLS; attempt += 1) {
    if (isRemoteAutomationTerminal(request?.status ?? initialStatus)) break;
    await new Promise((resolve) =>
      setTimeout(resolve, REMOTE_AUTOMATION_POLL_INTERVAL_MS),
    );
    request = await typedInvoke(command, { requestId }, schema);
  }
  if (!request || !isRemoteAutomationTerminal(request.status)) {
    throw new Error(
      "Timed out waiting for the host to settle the automation request. Refresh to check its state.",
    );
  }
  return request;
}

const PLAN_GATE_PAUSED_MESSAGE =
  "This automation is paused at the plan gate. Review the run plan and approve it from the plan artifact pane.";

async function getRemoteAutomationDetail(id: string): Promise<AutomationDetail> {
  return typedInvokeWithTransform(
    "get_automation",
    { input: { id } },
    AutomationDetailSchema,
    transformAutomationDetail,
  );
}

async function runRemoteAutomationAction(
  id: string,
  kind: "runNow" | "retryJudge",
): Promise<AutomationScheduleResponse> {
  const detail = await getRemoteAutomationDetail(id);
  const latestRun = detail.runs.reduce<AutomationRun | null>(
    (latest, run) => (!latest || run.runIndex > latest.runIndex ? run : latest),
    null,
  );
  let requested: z.infer<typeof RequestRemoteAutomationRunResponseSchema>;
  try {
    requested = await typedInvoke(
      "request_remote_automation_run",
      {
        automationId: id,
        kind,
        expectedRunId: latestRun?.id ?? null,
      },
      RequestRemoteAutomationRunResponseSchema,
    );
  } catch (error) {
    const code = remoteErrorCode(error);
    if (
      code === "REMOTE_AUTOMATION_RUN_ALREADY_SETTLED" &&
      kind === "retryJudge"
    ) {
      await getRemoteAutomationDetail(id);
      return { scheduled: false, reason: "latest judge is not failed" };
    }
    if (code === "REMOTE_AUTOMATION_RUN_RUN_CHANGED") {
      await getRemoteAutomationDetail(id);
      throw new Error("The automation moved on — refresh");
    }
    if (code === "REMOTE_AUTOMATION_RUN_PLAN_GATE_PAUSED") {
      throw new Error(PLAN_GATE_PAUSED_MESSAGE);
    }
    throw error;
  }
  const request = await pollRemoteAutomationRequest(
    "get_remote_automation_run_request",
    requested.requestId,
    requested.status,
    GetRemoteAutomationRunRequestResponseSchema,
  );
  if (request.status === "completed" && request.result) {
    if (
      request.result.code === "REMOTE_AUTOMATION_RUN_ALREADY_SETTLED" &&
      kind === "retryJudge"
    ) {
      await getRemoteAutomationDetail(id);
    }
    return {
      scheduled: request.result.scheduled,
      reason: request.result.reason ?? null,
    };
  }
  if (request.errorCode === "REMOTE_AUTOMATION_RUN_RUN_CHANGED") {
    await getRemoteAutomationDetail(id);
    throw new Error("The automation moved on — refresh");
  }
  if (request.errorCode === "REMOTE_AUTOMATION_RUN_PLAN_GATE_PAUSED") {
    throw new Error(PLAN_GATE_PAUSED_MESSAGE);
  }
  throw new Error(
    request.errorCode ?? "The host could not run the automation action",
  );
}

async function createRemoteAutomationDraft(
  input: CreateAutomationDraftInput,
): Promise<CreateAutomationDraftResponse> {
  const requested = await typedInvoke(
    "request_remote_automation_draft",
    {
      projectId: input.projectId,
      name: input.name ?? "Automation setup",
      authoringMode: input.authoringMode ?? "reviewed",
      baseRefKind: input.base?.kind ?? "project_default",
      baseBranchMode: input.base?.branchMode ?? "isolated",
      baseBranch: input.base?.ref || null,
    },
    RequestRemoteAutomationDraftResponseSchema,
  );
  const request = await pollRemoteAutomationRequest(
    "get_remote_automation_draft_request",
    requested.requestId,
    requested.status,
    GetRemoteAutomationDraftRequestResponseSchema,
  );
  if (request.status !== "completed") {
    throw new Error(
      request.errorCode ?? "The host could not create the automation draft",
    );
  }
  const automationId = request.result?.automationId ?? request.automationId;
  const detail = await getRemoteAutomationDetail(automationId);
  return {
    automation: detail.automation,
    setupConversationId: detail.automation.setupConversationId,
  };
}

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
  ): Promise<CreateAutomationDraftResponse> => {
    if (isRemoteEnvironmentId(getTransportEnvironmentId())) {
      return createRemoteAutomationDraft(input);
    }
    return typedInvokeWithTransform(
      "create_automation_draft",
      { input: createDraftArgs(input) },
      CreateAutomationDraftResponseSchema,
      transformCreateAutomationDraftResponse,
    );
  },

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

  triggerRunNow: (id: string): Promise<AutomationScheduleResponse> => {
    if (isRemoteEnvironmentId(getTransportEnvironmentId())) {
      return runRemoteAutomationAction(id, "runNow");
    }
    return typedInvokeWithTransform(
      "trigger_automation_run_now",
      { input: { id } },
      AutomationScheduleResponseSchema,
      transformAutomationScheduleResponse,
    );
  },

  restart: (id: string): Promise<AutomationScheduleResponse> =>
    typedInvokeWithTransform(
      "restart_automation",
      { input: { id } },
      AutomationScheduleResponseSchema,
      transformAutomationScheduleResponse,
    ),

  retryJudge: (id: string): Promise<AutomationScheduleResponse> => {
    if (isRemoteEnvironmentId(getTransportEnvironmentId())) {
      return runRemoteAutomationAction(id, "retryJudge");
    }
    return typedInvokeWithTransform(
      "retry_automation_judge",
      { input: { id } },
      AutomationScheduleResponseSchema,
      transformAutomationScheduleResponse,
    );
  },

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
