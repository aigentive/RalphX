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
    const result = await chatApi.switchAgentConversationMode({
      conversationId,
      mode: "edit",
    });
    if (result.workspace) {
      queryClient.setQueryData(
        agentWorkspaceKeys.workspace(conversationId),
        result.workspace,
      );
    }
    onConversationModeSwitched?.(
      conversationId,
      "edit",
      result.workspace ?? null,
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
    undefined,
    {
      conversationId,
      ...sendOptions,
      suppressUserMessage: true,
    },
  );
}
