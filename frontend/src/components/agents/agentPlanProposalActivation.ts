import type { QueryClient } from "@tanstack/react-query";

import {
  chatApi,
  type AgentConversationWorkspace,
  type AgentConversationWorkspaceMode,
  type SendAgentMessageResult,
} from "@/api/chat";
import type { ManualRoleRuntimeSelection } from "@/api/manual-role-defaults.types";
import {
  chatKeys,
  invalidateConversationDataQueries,
} from "@/hooks/useChat";
import { ideationKeys } from "@/hooks/useIdeation";
import { buildStoreKey } from "@/lib/chat-context-registry";
import { useChatStore } from "@/stores/chatStore";

import { PLAN_TO_PROPOSALS_REQUEST } from "./agentPlanModeActions";
import {
  agentWorkspaceKeys,
  invalidateWorkspaceQueries,
} from "./agentWorkspaceQueries";

interface ActivateAgentPlanProposalsParams {
  sessionId: string;
  workspace: AgentConversationWorkspace | null;
  queryClient: QueryClient;
  canPromoteWorkspace: boolean;
  onConversationModeSwitched?: (
    conversationId: string,
    mode: AgentConversationWorkspaceMode,
    workspace: AgentConversationWorkspace | null
  ) => void;
  onFocusIdeationSessionForConversation?: (
    conversationId: string,
    sessionId: string
  ) => void;
  runtimeOverride?: ManualRoleRuntimeSelection;
  workspaceActivationCompleted?: boolean;
  onWorkspaceActivated?: () => void;
}

export class PlanContinuationCommittedError extends Error {
  constructor(message: string) {
    super(message);
    this.name = "PlanContinuationCommittedError";
  }
}

/**
 * Derives the appended failure detail for plan-continuation retry dialogs.
 *
 * Tauri rejects with plain strings, so an `error instanceof Error` check alone
 * silently drops the backend's message and leaves the retry dialog unexplained.
 */
export function planContinuationFailureDetail(error: unknown): string {
  if (error instanceof Error) return ` ${error.message}`;
  if (typeof error === "string" && error.trim()) return ` ${error.trim()}`;
  return "";
}

export async function refreshTransitionedAgentWorkspace({
  queryClient,
  conversationId,
  onConversationModeSwitched,
}: {
  queryClient: QueryClient;
  conversationId: string;
  onConversationModeSwitched?: (
    conversationId: string,
    mode: AgentConversationWorkspaceMode,
    workspace: AgentConversationWorkspace | null,
  ) => void;
}): Promise<AgentConversationWorkspace | null> {
  try {
    const workspace = await chatApi.getAgentConversationWorkspace(conversationId);
    if (!workspace) return null;
    queryClient.setQueryData(agentWorkspaceKeys.workspace(conversationId), workspace);
    onConversationModeSwitched?.(conversationId, workspace.mode, workspace);
    return workspace;
  } catch {
    return null;
  } finally {
    await invalidateWorkspaceQueries(queryClient, conversationId);
  }
}

function pinIdeationConversation(
  queryClient: QueryClient,
  sessionId: string,
  conversationId: string,
) {
  useChatStore
    .getState()
    .setActiveConversation(buildStoreKey("ideation", sessionId), conversationId);
  void queryClient.invalidateQueries({
    queryKey: chatKeys.conversationList("ideation", sessionId),
  });
  invalidateConversationDataQueries(queryClient, conversationId);
  void queryClient.invalidateQueries({
    queryKey: ideationKeys.sessionWithData(sessionId),
  });
}

export async function activateAgentPlanProposals({
  sessionId,
  workspace,
  queryClient,
  canPromoteWorkspace,
  onConversationModeSwitched,
  onFocusIdeationSessionForConversation,
  runtimeOverride,
  workspaceActivationCompleted = false,
  onWorkspaceActivated,
}: ActivateAgentPlanProposalsParams): Promise<SendAgentMessageResult> {
  const conversationId = workspace?.conversationId ?? null;
  const ownsSession =
    Boolean(conversationId) &&
    (workspace?.taskPipelineSessionId === sessionId ||
      workspace?.linkedIdeationSessionId === sessionId);
  let workspaceIsTasks = workspaceActivationCompleted || workspace?.mode === "tasks";

  if (
    canPromoteWorkspace &&
    ownsSession &&
    workspace &&
    !workspaceActivationCompleted &&
    workspace.mode !== "tasks" &&
    conversationId
  ) {
    const activatedWorkspace = await chatApi.activateAgentTaskPipeline({
      conversationId,
      sessionId,
      ...(runtimeOverride ? { runtimeOverride } : {}),
    });
    queryClient.setQueryData(
      agentWorkspaceKeys.workspace(conversationId),
      activatedWorkspace,
    );
    onConversationModeSwitched?.(
      conversationId,
      "tasks",
      activatedWorkspace,
    );
    void invalidateWorkspaceQueries(queryClient, conversationId);
    workspaceIsTasks = activatedWorkspace.mode === "tasks";
    if (workspaceIsTasks) onWorkspaceActivated?.();
  } else if (
    ownsSession &&
    workspaceIsTasks &&
    workspace?.mode === "tasks" &&
    conversationId
  ) {
    onConversationModeSwitched?.(conversationId, "tasks", workspace);
  }

  if (ownsSession && workspaceIsTasks && conversationId) {
    onFocusIdeationSessionForConversation?.(conversationId, sessionId);
  }

  let sendResult: SendAgentMessageResult;
  try {
    sendResult = await chatApi.sendAgentMessage(
      "ideation",
      sessionId,
      PLAN_TO_PROPOSALS_REQUEST,
      undefined,
      runtimeOverride ? { runtimeOverride } : undefined,
    );
  } catch (error) {
    if (ownsSession && workspaceIsTasks && conversationId) {
      await refreshTransitionedAgentWorkspace({
        queryClient,
        conversationId,
        ...(onConversationModeSwitched ? { onConversationModeSwitched } : {}),
      });
      const detail = planContinuationFailureDetail(error);
      throw new PlanContinuationCommittedError(
        `Tasks mode is active, but proposal launch failed. Retry will only send the proposal request; it will not activate Tasks again.${detail}`,
      );
    }
    throw error;
  }
  pinIdeationConversation(queryClient, sessionId, sendResult.conversationId);
  return sendResult;
}
