import { typedInvokeWithTransform } from "@/lib/tauri";
import { backendApiUrl } from "@/api/backend";

import {
  AutomationDetailSchema,
  AutomationListSchema,
  AutomationSchema,
  CreateAutomationDraftResponseSchema,
} from "./automations.schemas";
import {
  transformAutomation,
  transformAutomationDetail,
  transformCreateAutomationDraftResponse,
  transformPauseAutomationInput,
  transformUpdateAutomationSettingsInput,
  transformUpdateAutomationSetupInput,
} from "./automations.transforms";
import type {
  Automation,
  AutomationDetail,
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
  AutomationChainMode,
  AutomationCompletionSignal,
  AutomationDetail,
  AutomationJudgeState,
  AutomationPromptAuthor,
  AutomationRun,
  AutomationRunMode,
  AutomationRunStatus,
  AutomationStatus,
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
  AutomationPromptAuthorSchema,
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
  transformCreateAutomationDraftResponse,
  transformPauseAutomationInput,
  transformUpdateAutomationSettingsInput,
  transformUpdateAutomationSetupInput,
} from "./automations.transforms";

const CALLER_SESSION_ID_HEADER = "x-ralphx-caller-session-id";

function createDraftArgs(input: CreateAutomationDraftInput): {
  projectId: string;
  name?: string;
} {
  return {
    projectId: input.projectId,
    ...(input.name !== undefined && { name: input.name }),
  };
}

async function postAutomationJson<TRaw, TResult>(
  endpoint: string,
  callerConversationId: string,
  schema: { parse: (value: unknown) => TRaw },
  transform: (raw: TRaw) => TResult,
  body?: Record<string, unknown>,
): Promise<TResult> {
  const response = await fetch(backendApiUrl(endpoint), {
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

  stop: (id: string): Promise<Automation> =>
    typedInvokeWithTransform(
      "stop_automation",
      { input: { id } },
      AutomationSchema,
      transformAutomation,
    ),

  setupAgent: {
    getAutomation: (callerConversationId: string): Promise<AutomationDetail> =>
      postAutomationJson(
        "get_automation",
        callerConversationId,
        AutomationDetailSchema,
        transformAutomationDetail,
      ),

    updateAutomation: (
      callerConversationId: string,
      input: UpdateAutomationSetupInput,
    ): Promise<Automation> =>
      postAutomationJson(
        "update_automation",
        callerConversationId,
        AutomationSchema,
        transformAutomation,
        transformUpdateAutomationSetupInput(input),
      ),

    finalizeAutomation: (callerConversationId: string): Promise<Automation> =>
      postAutomationJson(
        "finalize_automation",
        callerConversationId,
        AutomationSchema,
        transformAutomation,
      ),
  },
} as const;
