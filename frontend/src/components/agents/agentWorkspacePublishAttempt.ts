import type { QueryClient } from "@tanstack/react-query";

import { chatApi } from "@/api/chat";
import type {
  AgentConversationWorkspace,
  AgentConversationWorkspacePublicationEvent,
} from "@/api/chat";

import {
  classifyAgentWorkspacePublishTerminalEvent,
  getAgentWorkspaceTerminalPublicationStatus,
  getPostBaselinePublicationEvents,
} from "./agentWorkspacePublishState";
import {
  type AgentWorkspaceOperationToast,
  agentWorkspaceMaintenanceOperationToastId,
  agentWorkspaceOperationToastId,
  maintenanceOperationToastLabel,
  publishPipelineToastLabel,
  startAgentWorkspaceOperationToast,
} from "./agentWorkspaceOperationToast";
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
  controller: AgentWorkspaceOperationToast;
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

export function createAgentWorkspacePublishAttempt({
  conversationId,
  conversationTitle,
  projectId,
  startedAtMs,
  token,
}: {
  conversationId: string;
  conversationTitle: string | null;
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
    controller: startAgentWorkspaceOperationToast({
      conversationTitle,
      detail: publishPipelineToastLabel(null),
      id: agentWorkspaceOperationToastId(conversationId, "publish"),
      startedAtMs,
      title: "Publishing workspace",
    }),
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
  const latestEvent = suffix?.[suffix.length - 1];
  if (latestEvent) {
    attempt.controller.update({ detail: publishPipelineToastLabel(latestEvent.step) });
  }
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
    attempt.controller.update({
      detail: maintenanceOperation.blocker ?? maintenanceOperation.summary,
      id: agentWorkspaceMaintenanceOperationToastId(
        workspace.conversationId,
        maintenanceOperation.operationId,
      ),
      startedAtMs: new Date(maintenanceOperation.startedAt).getTime(),
      title: maintenanceOperationToastLabel(maintenanceOperation.stage),
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

export function renderAgentWorkspacePublishResult(
  attempt: ActivePublishAttempt,
  result: PublishFinalResult,
): void {
  if (result.kind === "success") {
    const message = result.workspace.publicationPrNumber
      ? `Published #${result.workspace.publicationPrNumber}`
      : result.workspace.publicationPrUrl
        ? `Published ${result.workspace.publicationPrUrl}`
        : "Published branch";
    attempt.controller.success(message, { detail: result.detail ?? null });
  } else if (result.kind === "needs_agent") {
    attempt.controller.error("Publish failed. Sent the error to the agent to fix.", {
      detail: result.detail ?? null,
    });
  } else if (result.kind === "failure") {
    attempt.controller.error("Failed to publish branch", {
      detail: result.detail ?? null,
    });
  } else if (result.kind === "no_changes") {
    attempt.controller.info("No changes to publish", {
      detail: result.detail ?? null,
    });
  } else if (result.kind === "ready") {
    attempt.controller.info("Base updated — ready to publish", {
      detail: result.detail ?? null,
    });
  } else if (result.kind === "blocked") {
    attempt.controller.error("Repair blocked", { detail: result.detail ?? null });
  } else {
    attempt.controller.info(
      result.status === "merged"
        ? "Publish stopped because the pull request was merged"
        : "Publish stopped because the pull request was closed",
      { detail: result.detail ?? null },
    );
  }
}
