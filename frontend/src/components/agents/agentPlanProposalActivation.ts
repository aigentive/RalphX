import type { QueryClient } from "@tanstack/react-query";

import {
  chatApi,
  type AgentConversationWorkspace,
  type AgentConversationWorkspaceMode,
  type SendAgentMessageResult,
} from "@/api/chat";
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
}: ActivateAgentPlanProposalsParams): Promise<SendAgentMessageResult> {
  const conversationId = workspace?.conversationId ?? null;
  const ownsSession =
    Boolean(conversationId) &&
    (workspace?.taskPipelineSessionId === sessionId ||
      workspace?.linkedIdeationSessionId === sessionId);
  let workspaceIsTasks = workspace?.mode === "tasks";

  if (
    canPromoteWorkspace &&
    ownsSession &&
    workspace &&
    workspace.mode !== "tasks" &&
    conversationId
  ) {
    const activatedWorkspace = await chatApi.activateAgentTaskPipeline({
      conversationId,
      sessionId,
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
  } else if (ownsSession && workspaceIsTasks && conversationId) {
    onConversationModeSwitched?.(conversationId, "tasks", workspace);
  }

  if (ownsSession && workspaceIsTasks && conversationId) {
    onFocusIdeationSessionForConversation?.(conversationId, sessionId);
  }

  const sendResult = await chatApi.sendAgentMessage(
    "ideation",
    sessionId,
    PLAN_TO_PROPOSALS_REQUEST,
  );
  pinIdeationConversation(queryClient, sessionId, sendResult.conversationId);
  return sendResult;
}
