import type { QueryClient } from "@tanstack/react-query";

import { chatApi } from "@/api/chat";
import type {
  AgentConversationWorkspace,
  AgentConversationWorkspacePublicationEvent,
} from "@/api/chat";

import { watchAgentWorkspaceOperation } from "./agentWorkspaceOperationRegistry";
import {
  classifyAgentWorkspacePublishTerminalEvent,
  getAgentWorkspaceTerminalPublicationStatus,
  getPostBaselinePublicationEvents,
} from "./agentWorkspacePublishState";
import { agentWorkspaceKeys } from "./agentWorkspaceQueries";

export interface AgentWorkspacePublishAttempt {
  conversationId: string;
  startedAtMs: number;
}

export type PublishEventBaseline =
  | { state: "available"; lastEventId: string | null }
  | { state: "loading" }
  | { state: "unavailable" };

export interface PublishAttemptState extends AgentWorkspacePublishAttempt {
  baseline: PublishEventBaseline;
  token: number;
}

export interface ActivePublishAttempt extends PublishAttemptState {
  completion: Promise<void>;
  projectId: string | null;
  reconciling: boolean;
  resolve: () => void;
  settled: boolean;
}

export type PublishFinalResult =
  | { detail?: string; kind: "blocked" }
  | { detail?: string; kind: "failure" }
  | { detail?: string; kind: "needs_agent" }
  | { detail?: string; kind: "no_changes" }
  | { detail?: string; kind: "ready" }
  | { detail?: string; kind: "success"; workspace: AgentConversationWorkspace }
  | { detail?: string; kind: "terminal"; status: "closed" | "merged" };

export type AgentWorkspaceOperationResult =
  | PublishFinalResult
  | { detail?: string; kind: "base-updated"; targetRef: string }
  | { detail?: string; kind: "base-already-current"; targetRef: string }
  | { detail?: string; kind: "base-update-failed" }
  | { detail?: string; kind: "repair-started" };

export function createAgentWorkspacePublishAttempt({
  conversationId,
  projectId,
  startedAtMs,
  token,
}: {
  conversationId: string;
  projectId: string | null;
  startedAtMs: number;
  token: number;
}): ActivePublishAttempt {
  let resolve!: () => void;
  const completion = new Promise<void>((promiseResolve) => {
    resolve = promiseResolve;
  });
  return {
    baseline: { state: "loading" },
    completion,
    conversationId,
    projectId,
    reconciling: false,
    resolve,
    settled: false,
    startedAtMs,
    token,
  };
}

export async function readAgentWorkspacePublishBaseline(
  queryClient: QueryClient,
  conversationId: string,
): Promise<PublishEventBaseline> {
  try {
    const events = await queryClient.fetchQuery({
      queryKey: agentWorkspaceKeys.publicationEvents(conversationId),
      queryFn: () =>
        chatApi.listAgentConversationWorkspacePublicationEvents(conversationId),
      staleTime: 0,
    });
    return {
      state: "available",
      lastEventId: events[events.length - 1]?.id ?? null,
    };
  } catch {
    return { state: "unavailable" };
  }
}

export async function readAgentWorkspaceDurablePublishResult(
  queryClient: QueryClient,
  attempt: ActivePublishAttempt,
  events: AgentConversationWorkspacePublicationEvent[],
): Promise<PublishFinalResult | null> {
  if (attempt.baseline.state !== "available") {
    return null;
  }
  const suffix = getPostBaselinePublicationEvents(
    events,
    attempt.baseline.lastEventId,
    attempt.startedAtMs,
  );
  const workspace = await queryClient.fetchQuery({
    queryKey: agentWorkspaceKeys.workspace(attempt.conversationId),
    queryFn: () => chatApi.getAgentConversationWorkspace(attempt.conversationId),
    staleTime: 0,
  });
  if (!workspace) {
    return null;
  }
  const terminalStatus = getAgentWorkspaceTerminalPublicationStatus(workspace);
  if (terminalStatus) {
    return { kind: "terminal", status: terminalStatus };
  }
  const maintenanceOperation = workspace.maintenanceOperation;
  if (maintenanceOperation?.status === "active") {
    watchAgentWorkspaceOperation({
      conversationId: workspace.conversationId,
      projectId: workspace.projectId,
      conversationTitle: null,
      kind: "observed",
      startedAtMs: null,
    });
    return null;
  }
  if (maintenanceOperation?.status === "ready") {
    return {
      kind: "ready",
      ...(maintenanceOperation.summary
        ? { detail: maintenanceOperation.summary }
        : {}),
    };
  }
  if (maintenanceOperation?.status === "blocked") {
    const detail = maintenanceOperation.blocker ?? maintenanceOperation.summary;
    return {
      kind: "blocked",
      ...(detail ? { detail } : {}),
    };
  }
  if (suffix === null) {
    return null;
  }
  let freshness;
  if (
    suffix.some(
      (event) =>
        (event.step === "published" && event.status === "succeeded") ||
        (event.step === "metadata_settled" &&
          event.status === "succeeded" &&
          (event.classification === "applied" ||
            event.classification === "reconciled")),
    )
  ) {
    try {
      freshness = await queryClient.fetchQuery({
        queryKey: agentWorkspaceKeys.scopedFreshness(
          attempt.conversationId,
          "full",
        ),
        queryFn: () =>
          chatApi.getAgentConversationWorkspaceFreshness(attempt.conversationId, {
            scope: "full",
          }),
        staleTime: 0,
      });
    } catch {
      return null;
    }
  }
  const terminal = classifyAgentWorkspacePublishTerminalEvent(
    suffix,
    workspace,
    freshness,
  );
  if (!terminal) {
    return null;
  }
  return terminal.kind === "success"
    ? { kind: "success", workspace }
    : { detail: terminal.event.summary, kind: terminal.kind };
}

export function agentWorkspaceOperationResultView(
  result: AgentWorkspaceOperationResult,
): { tone: "success" | "error" | "info"; message: string; detail: string | null } {
  const detail = result.detail ?? null;
  switch (result.kind) {
    case "success": {
      const message = result.workspace.publicationPrNumber
        ? `Published #${result.workspace.publicationPrNumber}`
        : result.workspace.publicationPrUrl
          ? `Published ${result.workspace.publicationPrUrl}`
          : "Published branch";
      return { tone: "success", message, detail };
    }
    case "needs_agent":
      return {
        tone: "error",
        message: "Publish failed. Sent the error to the agent to fix.",
        detail,
      };
    case "failure":
      return { tone: "error", message: "Failed to publish branch", detail };
    case "no_changes":
      return { tone: "info", message: "No changes to publish", detail };
    case "ready":
      return { tone: "info", message: "Base updated — ready to publish", detail };
    case "blocked":
      return { tone: "error", message: "Repair blocked", detail };
    case "terminal":
      return {
        tone: "info",
        message:
          result.status === "merged"
            ? "Publish stopped because the pull request was merged"
            : "Publish stopped because the pull request was closed",
        detail,
      };
    case "base-updated":
      return { tone: "success", message: `Updated from ${result.targetRef}`, detail };
    case "base-already-current":
      return {
        tone: "success",
        message: `Already current with ${result.targetRef}`,
        detail,
      };
    case "base-update-failed":
      return { tone: "error", message: "Failed to update from base", detail };
    case "repair-started":
      return { tone: "info", message: "Repair started", detail };
    default: {
      const exhaustive: never = result;
      return exhaustive;
    }
  }
}
