import { toast } from "sonner";

import { formatElapsedTime } from "@/lib/formatters";
import { navigateToAgentConversation } from "@/lib/navigation";
import { useAgentSessionStore } from "@/stores/agentSessionStore";

export type AgentWorkspaceOperationToastKind =
  | "local-commit"
  | "publish"
  | "rebase"
  | "update-from-base"
  | "workspace-review";

export interface AgentWorkspaceOperationToast {
  dismiss: () => void;
  error: (message: string, options?: AgentWorkspaceOperationToastResultOptions) => void;
  info: (message: string, options?: AgentWorkspaceOperationToastResultOptions) => void;
  success: (message: string, options?: AgentWorkspaceOperationToastResultOptions) => void;
  update: (options: Partial<AgentWorkspaceOperationToastOptions>) => void;
}

export interface AgentWorkspaceOperationToastOptions {
  conversationTitle?: string | null | undefined;
  detail?: string | null | undefined;
  id: string;
  startedAtMs?: number;
  targetConversation?:
    | {
        conversationId: string;
        projectId: string | null;
      }
    | null
    | undefined;
  title: string;
}

export interface AgentWorkspaceOperationToastResultOptions {
  detail?: string | null | undefined;
  duration?: number;
}

const OPERATION_TOAST_INTERVAL_MS = 1_000;
const MAX_OPERATION_ERROR_DETAIL_CHARS = 240;
const MAX_OPERATION_PROGRESS_DETAIL_CHARS = 80;
const MAX_OPERATION_RESULT_DETAIL_CHARS = 140;
const VERBOSE_OPERATION_RESULT_DETAIL = "Full output is available in the workspace.";
const ANSI_ESCAPE = String.fromCharCode(27);
export const AGENT_WORKSPACE_OPERATION_RESULT_DURATION_MS = 8_000;
export const AGENT_WORKSPACE_OPERATION_ERROR_DURATION_MS = 12_000;
type ActiveAgentWorkspaceOperationToastOptions =
  AgentWorkspaceOperationToastOptions & {
    startedAtMs: number;
  };

interface ActiveAgentWorkspaceOperationToastController {
  supersede: () => void;
}

const activeToastControllers = new Map<
  string,
  ActiveAgentWorkspaceOperationToastController
>();
const dismissedToastIds = new Set<string>();

export function isAgentWorkspaceOperationToastDismissed(id: string): boolean {
  return dismissedToastIds.has(id);
}

export function clearAgentWorkspaceOperationToastDismissal(id: string): void {
  dismissedToastIds.delete(id);
}

export function resetAgentWorkspaceOperationToastStateForTests() {
  for (const controller of activeToastControllers.values()) {
    controller.supersede();
  }
  activeToastControllers.clear();
  dismissedToastIds.clear();
}

export function agentWorkspaceOperationToastId(
  conversationId: string,
  kind: AgentWorkspaceOperationToastKind,
): string {
  return `agent-workspace-operation:${conversationId}:${kind}`;
}

export function agentWorkspaceMaintenanceOperationToastId(
  conversationId: string,
  operationId: string,
): string {
  return `agent-workspace-maintenance:${conversationId}:${operationId}`;
}

export function maintenanceOperationToastLabel(stage: string): string {
  if (stage === "updating_base") return "Updating base";
  if (stage === "repairing") return "Repairing workspace";
  if (stage === "validating") return "Validating repair";
  if (stage === "reviewing") return "Workspace Review in progress";
  if (stage === "publishing") return "Publishing workspace";
  if (stage === "ready") return "Base updated — ready to publish";
  if (stage === "blocked") return "Repair blocked";
  return "Continuing workspace operation";
}

export function publishPipelineToastLabel(status: string | null): string {
  if (status === "committing") {
    return "Commit changes";
  }
  if (status === "refreshing" || status === "refreshed") {
    return "Refresh branch";
  }
  if (status === "describing") {
    return "Draft PR description";
  }
  if (status === "pushing") {
    return "Push branch";
  }
  if (status === "pushed" || status === "published") {
    return "Open draft PR";
  }
  if (status === "description_failed") {
    return "PR description failed";
  }
  if (status === "needs_agent") {
    return "Repair needed";
  }
  if (status === "failed") {
    return "Publish failed";
  }
  return "Check workspace";
}

export function agentWorkspaceOperationToastDescription(
  ...parts: Array<string | null | undefined>
): string | undefined {
  const description = parts
    .map((part) => part?.trim())
    .filter((part): part is string => Boolean(part))
    .join(" • ");
  return description || undefined;
}

function cleanAgentWorkspaceOperationDetail(detail: string): string {
  return stripControlCharacters(stripAnsiEscapeSequences(detail))
    .replace(/\s*Raw output:\s*[\s\S]*$/i, "")
    .replace(/\s+/g, " ")
    .trim();
}

function stripAnsiEscapeSequences(detail: string): string {
  let output = "";
  for (let index = 0; index < detail.length; index += 1) {
    if (detail[index] !== ANSI_ESCAPE || detail[index + 1] !== "[") {
      output += detail[index] ?? "";
      continue;
    }
    index += 2;
    while (index < detail.length) {
      const code = detail.charCodeAt(index);
      if (code >= 64 && code <= 126) {
        break;
      }
      index += 1;
    }
  }
  return output;
}

function stripControlCharacters(detail: string): string {
  return Array.from(detail)
    .map((character) => {
      const code = character.charCodeAt(0);
      return code < 32 || code === 127 ? " " : character;
    })
    .join("");
}

export function agentWorkspaceOperationErrorDetail(
  error: unknown,
  fallback: string,
): string {
  const raw =
    error instanceof Error
      ? error.message
      : typeof error === "string"
        ? error
        : fallback;
  const compact = cleanAgentWorkspaceOperationDetail(raw) || fallback;
  if (compact.length <= MAX_OPERATION_ERROR_DETAIL_CHARS) {
    return compact;
  }
  return `${compact.slice(0, MAX_OPERATION_ERROR_DETAIL_CHARS - 3).trimEnd()}...`;
}

export function agentWorkspaceOperationResultDetail(
  detail: string | null | undefined,
): string | null {
  if (!detail) {
    return null;
  }
  const compact = cleanAgentWorkspaceOperationDetail(detail);
  if (!compact) {
    return null;
  }
  if (compact.length <= MAX_OPERATION_RESULT_DETAIL_CHARS) {
    return compact;
  }
  return VERBOSE_OPERATION_RESULT_DETAIL;
}

function agentWorkspaceOperationProgressDetail(
  detail: string | null | undefined,
): string | null {
  if (!detail) {
    return null;
  }
  const compact = cleanAgentWorkspaceOperationDetail(detail);
  if (!compact) {
    return null;
  }
  if (compact.length <= MAX_OPERATION_PROGRESS_DETAIL_CHARS) {
    return compact;
  }
  return `${compact.slice(0, MAX_OPERATION_PROGRESS_DETAIL_CHARS - 1).trimEnd()}…`;
}

function isTargetConversationVisible(
  targetConversation: AgentWorkspaceOperationToastOptions["targetConversation"],
): boolean {
  return (
    targetConversation !== null &&
    targetConversation !== undefined &&
    targetConversation.conversationId ===
      useAgentSessionStore.getState().visibleAgentScope?.visibleConversationId
  );
}

function targetConversationToastAction(
  targetConversation: AgentWorkspaceOperationToastOptions["targetConversation"],
) {
  if (!targetConversation) {
    return {};
  }
  return {
    action: {
      label: "Open conversation",
      onClick: () =>
        navigateToAgentConversation(
          targetConversation.projectId,
          targetConversation.conversationId,
        ),
    },
  };
}

function progressDescription(options: ActiveAgentWorkspaceOperationToastOptions): string | undefined {
  const elapsedSeconds = Math.max(
    0,
    Math.floor((Date.now() - options.startedAtMs) / 1_000),
  );
  const elapsed = formatElapsedTime(elapsedSeconds);
  return agentWorkspaceOperationToastDescription(
    options.conversationTitle,
    agentWorkspaceOperationProgressDetail(options.detail),
    elapsed,
  );
}

function resultDescription(
  options: ActiveAgentWorkspaceOperationToastOptions,
  resultOptions?: AgentWorkspaceOperationToastResultOptions,
): string | undefined {
  const detail =
    resultOptions && Object.prototype.hasOwnProperty.call(resultOptions, "detail")
      ? agentWorkspaceOperationResultDetail(resultOptions.detail)
      : options.detail;
  return agentWorkspaceOperationToastDescription(
    options.conversationTitle,
    detail,
  );
}

function resultToastOptions(
  options: ActiveAgentWorkspaceOperationToastOptions,
  resultOptions: AgentWorkspaceOperationToastResultOptions | undefined,
  fallbackDuration: number,
) {
  const description = resultDescription(options, resultOptions);
  return {
    ...(description ? { description } : {}),
    duration: resultOptions?.duration ?? fallbackDuration,
    id: options.id,
    ...targetConversationToastAction(options.targetConversation),
  };
}

export function startAgentWorkspaceOperationToast(
  options: AgentWorkspaceOperationToastOptions,
): AgentWorkspaceOperationToast {
  let currentOptions: ActiveAgentWorkspaceOperationToastOptions = {
    ...options,
    startedAtMs: options.startedAtMs ?? Date.now(),
  };
  let intervalId: ReturnType<typeof setInterval> | null = null;
  let activeToastId: string | null = null;
  let dismissed = false;
  let settled = false;
  let userDismissed = false;
  const programmaticDismissalIds = new Set<string>();

  const controller: ActiveAgentWorkspaceOperationToastController = {
    supersede: () => {
      if (settled || dismissed) {
        return;
      }
      settled = true;
      clearTimer();
      releaseActiveToast();
    },
  };

  const clearTimer = () => {
    if (intervalId !== null) {
      clearInterval(intervalId);
      intervalId = null;
    }
  };

  const releaseActiveToast = (id = activeToastId) => {
    if (id && activeToastControllers.get(id) === controller) {
      activeToastControllers.delete(id);
    }
  };

  const dismissToast = (id: string) => {
    programmaticDismissalIds.add(id);
    toast.dismiss(id);
  };

  const suppressActiveToast = () => {
    if (!activeToastId) {
      return;
    }
    const toastId = activeToastId;
    activeToastId = null;
    dismissToast(toastId);
    releaseActiveToast(toastId);
  };

  const registerActiveToast = (id: string) => {
    if (activeToastId && activeToastId !== id) {
      dismissToast(activeToastId);
      releaseActiveToast();
    }
    const existing = activeToastControllers.get(id);
    if (existing && existing !== controller) {
      existing.supersede();
    }
    activeToastId = id;
    activeToastControllers.set(id, controller);
  };

  const settle = () => {
    if (settled) {
      return false;
    }
    const shouldShowResult =
      !userDismissed &&
      !isAgentWorkspaceOperationToastDismissed(currentOptions.id) &&
      !isTargetConversationVisible(currentOptions.targetConversation);
    settled = true;
    clearTimer();
    releaseActiveToast();
    clearAgentWorkspaceOperationToastDismissal(currentOptions.id);
    if (!shouldShowResult) suppressActiveToast();
    return shouldShowResult;
  };

  const dismiss = () => {
    if (settled || dismissed) {
      return;
    }
    dismissed = true;
    clearTimer();
    suppressActiveToast();
  };

  const render = () => {
    if (settled || dismissed) {
      return;
    }
    if (
      currentOptions.targetConversation &&
      isAgentWorkspaceOperationToastDismissed(currentOptions.id)
    ) {
      dismissed = true;
      userDismissed = true;
      clearTimer();
      suppressActiveToast();
      return;
    }
    if (isTargetConversationVisible(currentOptions.targetConversation)) {
      suppressActiveToast();
      return;
    }
    registerActiveToast(currentOptions.id);
    const toastId = currentOptions.id;
    const persistsDismissal = Boolean(currentOptions.targetConversation);
    const description = progressDescription(currentOptions);
    toast.loading(currentOptions.title, {
      ...(description ? { description } : {}),
      closeButton: true,
      dismissible: true,
      duration: Infinity,
      id: toastId,
      ...targetConversationToastAction(currentOptions.targetConversation),
      onDismiss: () => {
        if (programmaticDismissalIds.delete(toastId)) {
          return;
        }
        if (settled) {
          return;
        }
        dismissed = true;
        if (persistsDismissal) {
          userDismissed = true;
          dismissedToastIds.add(toastId);
        }
        clearTimer();
        releaseActiveToast(toastId);
        if (activeToastId === toastId) {
          activeToastId = null;
        }
      },
    });
  };

  render();
  if (!settled && !dismissed) {
    intervalId = setInterval(render, OPERATION_TOAST_INTERVAL_MS);
  }

  return {
    dismiss,
    error: (message: string, options?: AgentWorkspaceOperationToastResultOptions) => {
      if (!settle()) {
        return;
      }
      toast.error(message, {
        ...resultToastOptions(
          currentOptions,
          options,
          AGENT_WORKSPACE_OPERATION_ERROR_DURATION_MS,
        ),
        closeButton: true,
        dismissible: true,
      });
    },
    info: (message: string, options?: AgentWorkspaceOperationToastResultOptions) => {
      if (!settle()) {
        return;
      }
      toast.info(message, {
        ...resultToastOptions(
          currentOptions,
          options,
          AGENT_WORKSPACE_OPERATION_RESULT_DURATION_MS,
        ),
        dismissible: true,
      });
    },
    success: (message: string, options?: AgentWorkspaceOperationToastResultOptions) => {
      if (!settle()) {
        return;
      }
      toast.success(
        message,
        resultToastOptions(
          currentOptions,
          options,
          AGENT_WORKSPACE_OPERATION_RESULT_DURATION_MS,
        ),
      );
    },
    update: (nextOptions: Partial<AgentWorkspaceOperationToastOptions>) => {
      if (settled || dismissed) {
        return;
      }
      currentOptions = {
        ...currentOptions,
        ...nextOptions,
        id: nextOptions.id ?? currentOptions.id,
        startedAtMs: nextOptions.startedAtMs ?? currentOptions.startedAtMs,
        title: nextOptions.title ?? currentOptions.title,
      };
      render();
    },
  };
}
