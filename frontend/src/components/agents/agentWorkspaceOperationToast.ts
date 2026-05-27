import { toast } from "sonner";

import { formatElapsedTime } from "@/lib/formatters";

export type AgentWorkspaceOperationToastKind =
  | "publish"
  | "rebase"
  | "update-from-base";

export interface AgentWorkspaceOperationToast {
  dispose: () => void;
  error: (message: string) => void;
  success: (message: string) => void;
  update: (options: Partial<AgentWorkspaceOperationToastOptions>) => void;
}

export interface AgentWorkspaceOperationToastOptions {
  detail?: string | null;
  id: string;
  startedAtMs?: number;
  title: string;
}

const OPERATION_TOAST_INTERVAL_MS = 1_000;
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

function progressMessage(options: ActiveAgentWorkspaceOperationToastOptions): string {
  const elapsedSeconds = Math.max(
    0,
    Math.floor((Date.now() - options.startedAtMs) / 1_000),
  );
  const elapsed = formatElapsedTime(elapsedSeconds);
  const detail = options.detail?.trim();
  return detail
    ? `${options.title} - ${detail} - ${elapsed}`
    : `${options.title} - ${elapsed}`;
}

export function startAgentWorkspaceOperationToast(
  options: AgentWorkspaceOperationToastOptions,
): AgentWorkspaceOperationToast {
  let currentOptions: ActiveAgentWorkspaceOperationToastOptions = {
    ...options,
    startedAtMs: options.startedAtMs ?? Date.now(),
  };
  let intervalId: ReturnType<typeof setInterval> | null = null;
  let settled = false;

  const clearTimer = () => {
    if (intervalId !== null) {
      clearInterval(intervalId);
      intervalId = null;
    }
  };

  const render = () => {
    if (settled) {
      return;
    }
    toast.loading(progressMessage(currentOptions), {
      duration: Infinity,
      id: currentOptions.id,
    });
  };

  render();
  intervalId = setInterval(render, OPERATION_TOAST_INTERVAL_MS);

  return {
    dispose: () => {
      clearTimer();
    },
    error: (message: string) => {
      settled = true;
      clearTimer();
      toast.error(message, { id: currentOptions.id });
    },
    success: (message: string) => {
      settled = true;
      clearTimer();
      toast.success(message, { id: currentOptions.id });
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
