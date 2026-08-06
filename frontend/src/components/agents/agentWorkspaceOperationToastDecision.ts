/**
 * Pure precedence/message-mapping logic for the global workspace-operation
 * toast driver (`useAgentWorkspaceOperationToasts`). Kept separate so the
 * hook stays under the file size limit and the precedence rules stay
 * directly unit-testable without mounting React.
 */

import type { AgentConversationWorkspace } from "@/api/chat";

import type { WatchedAgentWorkspaceOperation } from "./agentWorkspaceOperationRegistry";
import {
  agentWorkspacePublishDismissalKey,
  agentWorkspaceRepairDismissalKey,
} from "./agentWorkspaceOperationDismissals";
import {
  AGENT_WORKSPACE_OPERATION_ERROR_DURATION_MS,
  AGENT_WORKSPACE_OPERATION_RESULT_DURATION_MS,
  agentWorkspaceMaintenanceOperationToastId,
  agentWorkspaceOperationResultDetail,
  agentWorkspaceOperationToastDescription,
  agentWorkspaceOperationToastId,
  publishPipelineToastLabel,
  type AgentWorkspaceOperationToastView,
} from "./agentWorkspaceOperationToast";
import type { AgentWorkspaceOperationResult } from "./agentWorkspacePublishAttempt";
import {
  getAgentWorkspaceMaintenancePresentation,
  getAgentWorkspaceTerminalPublicationStatus,
  isAgentWorkspacePublishActive,
} from "./agentWorkspacePublishState";

export interface DeriveAgentWorkspaceOperationToastDecisionInput {
  workspace: AgentConversationWorkspace | null;
  entry: WatchedAgentWorkspaceOperation;
  pendingResult: AgentWorkspaceOperationResult | null;
  consecutiveFetchFailures: number;
  /**
   * True while a session-started operation may still report its result. Keeps
   * the driver from unwatching (and orphaning the eventual mailbox entry)
   * before a still-pending `reportAgentWorkspaceOperationResult` call lands.
   */
  awaitingSessionResult: boolean;
}

export type AgentWorkspaceOperationToastDecision =
  | { kind: "progress"; view: AgentWorkspaceOperationToastView }
  | { kind: "announce"; view: AgentWorkspaceOperationToastView; stateKey: string }
  | { kind: "idle"; unwatch: boolean };

function targetConversationFor(
  entry: WatchedAgentWorkspaceOperation,
  workspace: AgentConversationWorkspace | null,
): { conversationId: string; projectId: string | null } {
  return {
    conversationId: entry.conversationId,
    projectId: entry.projectId ?? workspace?.projectId ?? null,
  };
}

function toastIdForAttempt(entry: WatchedAgentWorkspaceOperation): string {
  return agentWorkspaceOperationToastId(
    entry.conversationId,
    entry.kind === "base-update" ? "update-from-base" : "publish",
  );
}

function dismissalKeyForAttempt(entry: WatchedAgentWorkspaceOperation): string {
  return agentWorkspacePublishDismissalKey(entry.conversationId, entry.publishAttemptKey);
}

function resultToastPresentation(
  result: AgentWorkspaceOperationResult,
): { title: string; tone: AgentWorkspaceOperationToastView["tone"] } {
  switch (result.kind) {
    case "success":
      return {
        title: result.workspace.publicationPrNumber
          ? `Published #${result.workspace.publicationPrNumber}`
          : result.workspace.publicationPrUrl
            ? `Published ${result.workspace.publicationPrUrl}`
            : "Published branch",
        tone: "success",
      };
    case "needs_agent":
      return { title: "Publish failed. Sent the error to the agent to fix.", tone: "error" };
    case "failure":
      return { title: "Failed to publish branch", tone: "error" };
    case "no_changes":
      return { title: "No changes to publish", tone: "info" };
    case "ready":
      return { title: "Base updated — ready to publish", tone: "info" };
    case "blocked":
      return { title: "Repair blocked", tone: "error" };
    case "terminal":
      return {
        title:
          result.status === "merged"
            ? "Publish stopped because the pull request was merged"
            : "Publish stopped because the pull request was closed",
        tone: "info",
      };
    case "base-updated":
      return { title: `Updated from ${result.targetRef}`, tone: "success" };
    case "base-already-current":
      return { title: `Already current with ${result.targetRef}`, tone: "success" };
    case "base-update-failed":
      return { title: "Failed to update from base", tone: "error" };
    case "repair-started":
      return { title: "Repair started", tone: "info" };
  }
}

function durationForTone(tone: AgentWorkspaceOperationToastView["tone"]): number {
  return tone === "error"
    ? AGENT_WORKSPACE_OPERATION_ERROR_DURATION_MS
    : AGENT_WORKSPACE_OPERATION_RESULT_DURATION_MS;
}

export function deriveAgentWorkspaceOperationToastDecision({
  workspace,
  entry,
  pendingResult,
  consecutiveFetchFailures,
  awaitingSessionResult,
}: DeriveAgentWorkspaceOperationToastDecisionInput): AgentWorkspaceOperationToastDecision {
  const targetConversation = targetConversationFor(entry, workspace);

  if (pendingResult) {
    const { title, tone } = resultToastPresentation(pendingResult);
    return {
      kind: "announce",
      stateKey: `result:${pendingResult.kind}`,
      view: {
        id: toastIdForAttempt(entry),
        dismissalKey: dismissalKeyForAttempt(entry),
        title,
        description: agentWorkspaceOperationToastDescription(
          entry.conversationTitle,
          agentWorkspaceOperationResultDetail(pendingResult.detail ?? null),
        ),
        startedAtMs: null,
        targetConversation,
        tone,
        durationMs: durationForTone(tone),
      },
    };
  }

  const terminalStatus = getAgentWorkspaceTerminalPublicationStatus(workspace);
  if (terminalStatus) {
    return {
      kind: "announce",
      stateKey: `pr:${terminalStatus}`,
      view: {
        id: toastIdForAttempt(entry),
        dismissalKey: dismissalKeyForAttempt(entry),
        title:
          terminalStatus === "merged"
            ? "Publish stopped because the pull request was merged"
            : "Publish stopped because the pull request was closed",
        description: agentWorkspaceOperationToastDescription(entry.conversationTitle),
        startedAtMs: null,
        targetConversation,
        tone: "info",
        durationMs: AGENT_WORKSPACE_OPERATION_RESULT_DURATION_MS,
      },
    };
  }

  if (consecutiveFetchFailures >= 3) {
    return { kind: "idle", unwatch: true };
  }

  const operation = workspace?.maintenanceOperation ?? null;
  const presentation = operation
    ? getAgentWorkspaceMaintenancePresentation(workspace)
    : null;
  if (operation && presentation) {
    const baseView = {
      id: agentWorkspaceMaintenanceOperationToastId(entry.conversationId, operation.operationId),
      dismissalKey: agentWorkspaceRepairDismissalKey(
        entry.conversationId,
        operation.operationId,
        operation.generation,
      ),
      title: presentation.title,
      description: agentWorkspaceOperationToastDescription(
        entry.conversationTitle,
        agentWorkspaceOperationResultDetail(presentation.summary),
      ),
      targetConversation,
    };
    const stateKey = `repair:${operation.operationId}:${operation.generation}:${operation.status}:${operation.holdReason ?? "none"}`;
    if (operation.status === "active") {
      return {
        kind: "progress",
        view: {
          ...baseView,
          startedAtMs: Date.parse(operation.startedAt),
          tone: "loading",
          durationMs: Infinity,
        },
      };
    }
    const tone = operation.status === "ready" ? "success" : operation.status === "blocked" ? "error" : "info";
    return {
      kind: "announce",
      stateKey,
      view: {
        ...baseView,
        startedAtMs: null,
        tone,
        durationMs: durationForTone(tone),
      },
    };
  }

  if (isAgentWorkspacePublishActive(workspace)) {
    return {
      kind: "progress",
      view: {
        id: agentWorkspaceOperationToastId(entry.conversationId, "publish"),
        dismissalKey: dismissalKeyForAttempt(entry),
        title: "Publishing workspace",
        description: agentWorkspaceOperationToastDescription(
          entry.conversationTitle,
          publishPipelineToastLabel(workspace?.publicationPushStatus ?? null),
        ),
        startedAtMs: entry.startedAtMs,
        targetConversation,
        tone: "loading",
        durationMs: Infinity,
      },
    };
  }

  // A session-started operation that hasn't reported its result yet (and is
  // still inside its grace window) must not be unwatched here — that would
  // orphan the mailbox entry the caller is about to fill in.
  return { kind: "idle", unwatch: !awaitingSessionResult };
}
