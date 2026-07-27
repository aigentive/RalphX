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

  if (!workspace.linkedIdeationSessionId) {
    throw new Error("Plan workspace is missing its linked planning session");
  }
  const activation = await chatApi.activateAgentPlanDirectImplementation({
    conversationId,
    sessionId: workspace.linkedIdeationSessionId,
    retry: workspace.mode === "edit",
  });
  queryClient.setQueryData(
    agentWorkspaceKeys.workspace(conversationId),
    activation.workspace,
  );
  onConversationModeSwitched?.(
    conversationId,
    "edit",
    activation.workspace,
  );
  void invalidateWorkspaceQueries(queryClient, conversationId);

  return chatApi.sendAgentMessage(
    "project",
    projectId,
    PLAN_IMPLEMENT_DIRECTLY_REQUEST,
    undefined,
    {
      conversationId,
      ...sendOptions,
      composerArtifactReferences: activation.artifactReferences,
      suppressUserMessage: true,
    },
  );
}
