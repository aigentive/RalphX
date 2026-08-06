import { createElement } from "react";
import { toast } from "sonner";

import { ActionToast, type ActionToastAction } from "@/components/ui/action-toast";
import { navigateToAgentConversation } from "@/lib/navigation";

import { AgentWorkspaceOperationElapsed } from "./AgentWorkspaceOperationElapsed";

/**
 * Split of responsibilities: durable, poll-driven operations (repair, publish,
 * base update) belong to `useAgentWorkspaceOperationToasts`; only bounded
 * request/response flows with no durable backend state to poll may use
 * `startAgentWorkspaceOperationToast`.
 */

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

const MAX_OPERATION_ERROR_DETAIL_CHARS = 240;
const MAX_OPERATION_PROGRESS_DETAIL_CHARS = 80;
const MAX_OPERATION_RESULT_DETAIL_CHARS = 140;
const VERBOSE_OPERATION_RESULT_DETAIL = "Full output is available in the workspace.";
const ANSI_ESCAPE = String.fromCharCode(27);
export const AGENT_WORKSPACE_OPERATION_RESULT_DURATION_MS = 8_000;
export const AGENT_WORKSPACE_OPERATION_ERROR_DURATION_MS = 12_000;

export function resetAgentWorkspaceOperationToastStateForTests() {
  // No module-level session state remains to reset; kept so existing test
  // setups that call this between cases keep compiling.
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
  if (stage === "held") return "Repair paused";
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

export interface AgentWorkspaceOperationToastView {
  /** Stable Sonner id. */
  id: string;
  dismissalKey: string;
  title: string;
  description?: string | undefined;
  /** Drives the elapsed meter; null = no meter. */
  startedAtMs: number | null;
  targetConversation?:
    | {
        conversationId: string;
        projectId: string | null;
      }
    | undefined;
  tone: "loading" | "success" | "error" | "info";
  /** Infinity for loading. */
  durationMs: number;
}

function buildAgentWorkspaceOperationToastContent(
  view: AgentWorkspaceOperationToastView,
  handlers: { onDismiss: () => void },
) {
  const actions: ActionToastAction[] = [];
  const targetConversation = view.targetConversation;
  if (targetConversation) {
    actions.push({
      label: "Open conversation",
      accent: true,
      onClick: () =>
        navigateToAgentConversation(
          targetConversation.projectId,
          targetConversation.conversationId,
        ),
    });
  }
  return createElement(ActionToast, {
    title: view.title,
    description: view.description,
    ...(view.startedAtMs !== null && {
      meta: createElement(AgentWorkspaceOperationElapsed, {
        startedAtMs: view.startedAtMs,
      }),
    }),
    actions,
    onDismiss: () => {
      handlers.onDismiss();
      dismissAgentWorkspaceOperationToast(view.id);
    },
  });
}

export function renderAgentWorkspaceOperationToast(
  view: AgentWorkspaceOperationToastView,
  handlers: { onDismiss: () => void },
): void {
  const content = buildAgentWorkspaceOperationToastContent(view, handlers);
  const options = {
    id: view.id,
    duration: view.durationMs,
    dismissible: true,
    closeButton: false,
    onDismiss: handlers.onDismiss,
  };
  if (view.tone === "loading") {
    toast.loading(content, options);
    return;
  }
  if (view.tone === "success") {
    toast.success(content, options);
    return;
  }
  if (view.tone === "error") {
    toast.error(content, options);
    return;
  }
  toast.info(content, options);
}

export function dismissAgentWorkspaceOperationToast(id: string): void {
  toast.dismiss(id);
}

export function startAgentWorkspaceOperationToast(
  options: AgentWorkspaceOperationToastOptions,
): AgentWorkspaceOperationToast {
  let currentOptions: AgentWorkspaceOperationToastOptions & { startedAtMs: number } = {
    ...options,
    startedAtMs: options.startedAtMs ?? Date.now(),
  };
  let settled = false;
  let dismissed = false;

  // Sonner already removed the toast when the user dismissed it; only record it.
  const handleDismiss = () => {
    dismissed = true;
  };

  // Caller-initiated dismissal must actually remove the visible toast, which
  // renders with `duration: Infinity` until the caller settles it.
  const dismiss = () => {
    if (settled || dismissed) {
      return;
    }
    dismissed = true;
    dismissAgentWorkspaceOperationToast(currentOptions.id);
  };

  const render = () => {
    if (settled || dismissed) {
      return;
    }
    renderAgentWorkspaceOperationToast(
      {
        id: currentOptions.id,
        dismissalKey: currentOptions.id,
        title: currentOptions.title,
        description: agentWorkspaceOperationToastDescription(
          currentOptions.conversationTitle,
          agentWorkspaceOperationProgressDetail(currentOptions.detail),
        ),
        startedAtMs: currentOptions.startedAtMs,
        targetConversation: currentOptions.targetConversation ?? undefined,
        tone: "loading",
        durationMs: Infinity,
      },
      { onDismiss: handleDismiss },
    );
  };

  const settle = (): boolean => {
    if (settled || dismissed) {
      return false;
    }
    settled = true;
    return true;
  };

  const renderResult = (
    tone: "success" | "error" | "info",
    message: string,
    resultOptions: AgentWorkspaceOperationToastResultOptions | undefined,
    fallbackDuration: number,
  ) => {
    const detail =
      resultOptions && Object.prototype.hasOwnProperty.call(resultOptions, "detail")
        ? agentWorkspaceOperationResultDetail(resultOptions.detail)
        : (currentOptions.detail ?? null);
    renderAgentWorkspaceOperationToast(
      {
        id: currentOptions.id,
        dismissalKey: currentOptions.id,
        title: message,
        description: agentWorkspaceOperationToastDescription(
          currentOptions.conversationTitle,
          detail,
        ),
        startedAtMs: null,
        targetConversation: currentOptions.targetConversation ?? undefined,
        tone,
        durationMs: resultOptions?.duration ?? fallbackDuration,
      },
      { onDismiss: () => undefined },
    );
  };

  render();

  return {
    dismiss,
    error: (message, resultOptions) => {
      if (!settle()) return;
      renderResult(
        "error",
        message,
        resultOptions,
        AGENT_WORKSPACE_OPERATION_ERROR_DURATION_MS,
      );
    },
    info: (message, resultOptions) => {
      if (!settle()) return;
      renderResult(
        "info",
        message,
        resultOptions,
        AGENT_WORKSPACE_OPERATION_RESULT_DURATION_MS,
      );
    },
    success: (message, resultOptions) => {
      if (!settle()) return;
      renderResult(
        "success",
        message,
        resultOptions,
        AGENT_WORKSPACE_OPERATION_RESULT_DURATION_MS,
      );
    },
    update: (nextOptions) => {
      if (settled || dismissed) {
        return;
      }
      const previousId = currentOptions.id;
      const nextId = nextOptions.id ?? previousId;
      if (nextId !== previousId) {
        // The superseded toast renders with `duration: Infinity`; without this
        // it would stay on screen next to its replacement.
        dismissAgentWorkspaceOperationToast(previousId);
      }
      currentOptions = {
        ...currentOptions,
        ...nextOptions,
        id: nextId,
        startedAtMs: nextOptions.startedAtMs ?? currentOptions.startedAtMs,
        title: nextOptions.title ?? currentOptions.title,
      };
      render();
    },
  };
}
