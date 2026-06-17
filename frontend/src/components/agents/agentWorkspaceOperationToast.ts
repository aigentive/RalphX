import { toast } from "sonner";

import { formatElapsedTime } from "@/lib/formatters";

export type AgentWorkspaceOperationToastKind =
  | "publish"
  | "rebase"
  | "update-from-base";

export interface AgentWorkspaceOperationToast {
  dispose: () => void;
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
  const withoutRawOutput = raw.replace(/\s*Raw output:\s*[\s\S]*$/i, "");
  const compact = (withoutRawOutput.trim() || fallback)
    .replace(/\s+/g, " ")
    .trim();
  if (compact.length <= MAX_OPERATION_ERROR_DETAIL_CHARS) {
    return compact;
  }
  return `${compact.slice(0, MAX_OPERATION_ERROR_DETAIL_CHARS - 3).trimEnd()}...`;
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
  return agentWorkspaceOperationToastDescription(
    options.conversationTitle,
    resultOptions?.detail ?? options.detail,
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
  let dismissed = false;
  let settled = false;

  const clearTimer = () => {
    if (intervalId !== null) {
      clearInterval(intervalId);
      intervalId = null;
    }
  };

  const render = () => {
    if (settled || dismissed) {
      return;
    }
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
    dispose: () => {
      clearTimer();
    },
    error: (message: string, options?: AgentWorkspaceOperationToastResultOptions) => {
      settled = true;
      clearTimer();
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
      settled = true;
      clearTimer();
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
      settled = true;
      clearTimer();
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
      if (settled) {
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
