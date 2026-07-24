import type { QueryClient } from "@tanstack/react-query";

import {
  chatApi,
  type AgentConversationWorkspace,
  type AgentConversationWorkspaceMode,
  type SendAgentMessageOptions,
  type SendAgentMessageResult,
} from "@/api/chat";

import { PLAN_IMPLEMENT_DIRECTLY_REQUEST } from "./agentPlanModeActions";
import {
  agentWorkspaceKeys,
  invalidateWorkspaceQueries,
} from "./agentWorkspaceQueries";

type DirectImplementationSendOptions = Omit<
  SendAgentMessageOptions,
  "conversationId" | "suppressUserMessage"
>;

interface ImplementAgentPlanDirectlyParams {
  projectId: string;
  workspace: AgentConversationWorkspace;
  queryClient: QueryClient;
  onConversationModeSwitched?: (
    conversationId: string,
    mode: AgentConversationWorkspaceMode,
    workspace: AgentConversationWorkspace | null,
  ) => void;
  sendOptions?: DirectImplementationSendOptions;
}

export async function implementAgentPlanDirectly({
  projectId,
  workspace,
  queryClient,
  onConversationModeSwitched,
  sendOptions,
}: ImplementAgentPlanDirectlyParams): Promise<SendAgentMessageResult> {
  const conversationId = workspace.conversationId;

  if (workspace.mode !== "edit") {
    if (!workspace.linkedIdeationSessionId) {
      throw new Error("Plan workspace is missing its linked planning session");
    }
    const activatedWorkspace =
      await chatApi.activateAgentPlanDirectImplementation({
      conversationId,
      sessionId: workspace.linkedIdeationSessionId,
    });
    queryClient.setQueryData(
      agentWorkspaceKeys.workspace(conversationId),
      activatedWorkspace,
    );
    onConversationModeSwitched?.(
      conversationId,
      "edit",
      activatedWorkspace,
    );
    void invalidateWorkspaceQueries(queryClient, conversationId);
  } else {
    onConversationModeSwitched?.(conversationId, "edit", workspace);
  }

  return chatApi.sendAgentMessage(
    "project",
    projectId,
    PLAN_IMPLEMENT_DIRECTLY_REQUEST,
    undefined,
    {
      conversationId,
      ...sendOptions,
      suppressUserMessage: true,
    },
  );
}
