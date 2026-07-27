import { toast } from "sonner";

import { formatElapsedTime } from "@/lib/formatters";

export type AgentWorkspaceOperationToastKind =
  | "publish"
  | "rebase"
  | "update-from-base";

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
  title: string;
}

export interface AgentWorkspaceOperationToastResultOptions {
  detail?: string | null | undefined;
  duration?: number;
}

const OPERATION_TOAST_INTERVAL_MS = 1_000;
const MAX_OPERATION_ERROR_DETAIL_CHARS = 240;
const MAX_OPERATION_RESULT_DETAIL_CHARS = 140;
const VERBOSE_OPERATION_RESULT_DETAIL = "Full output is available in the workspace.";
const ANSI_ESCAPE = String.fromCharCode(27);
export const AGENT_WORKSPACE_OPERATION_RESULT_DURATION_MS = 8_000;
export const AGENT_WORKSPACE_OPERATION_ERROR_DURATION_MS = 12_000;
type ActiveAgentWorkspaceOperationToastOptions =
  AgentWorkspaceOperationToastOptions & {
    startedAtMs: number;
  };
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

function progressDescription(options: ActiveAgentWorkspaceOperationToastOptions): string | undefined {
  const elapsedSeconds = Math.max(
    0,
    Math.floor((Date.now() - options.startedAtMs) / 1_000),
  );
  const elapsed = formatElapsedTime(elapsedSeconds);
  return agentWorkspaceOperationToastDescription(
    options.conversationTitle,
    options.detail,
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

  const clearTimer = () => {
    if (intervalId !== null) {
      clearInterval(intervalId);
      intervalId = null;
    }
  };

  const registerActiveToast = (id: string) => {
    if (activeToastId && activeToastId !== id) {
      toast.dismiss(activeToastId);
    }
    activeToastId = id;
  };

  const settle = () => {
    if (settled) {
      return;
    }
    settled = true;
    clearTimer();
  };

  const dismiss = () => {
    if (settled || dismissed) {
      return;
    }
    dismissed = true;
    clearTimer();
    if (activeToastId) {
      toast.dismiss(activeToastId);
    }
  };

  const render = () => {
    if (settled || dismissed) {
      return;
    }
    registerActiveToast(currentOptions.id);
    const description = progressDescription(currentOptions);
    toast.loading(currentOptions.title, {
      ...(description ? { description } : {}),
      closeButton: true,
      dismissible: true,
      duration: Infinity,
      id: currentOptions.id,
      onDismiss: () => {
        if (settled) {
          return;
        }
        dismissed = true;
        clearTimer();
      },
    });
  };

  render();
  intervalId = setInterval(render, OPERATION_TOAST_INTERVAL_MS);

  return {
    dismiss,
    error: (message: string, options?: AgentWorkspaceOperationToastResultOptions) => {
      if (settled) {
        return;
      }
      settle();
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
      if (settled) {
        return;
      }
      settle();
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
      if (settled) {
        return;
      }
      settle();
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
