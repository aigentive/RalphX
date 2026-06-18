import { useCallback, useState } from "react";
import type { QueryClient } from "@tanstack/react-query";
import { toast } from "sonner";

import { chatApi } from "@/api/chat";
import type { AgentConversationWorkspace } from "@/api/chat";
import { invalidateConversationDataQueries } from "@/hooks/useChat";

import type { AgentConversation } from "./agentConversations";
import {
  AGENT_WORKSPACE_OPERATION_ERROR_DURATION_MS,
  AGENT_WORKSPACE_OPERATION_RESULT_DURATION_MS,
  agentWorkspaceOperationErrorDetail,
  agentWorkspaceOperationToastDescription,
  agentWorkspaceOperationToastId,
  markAgentWorkspaceOperationToastSettled,
} from "./agentWorkspaceOperationToast";
import { invalidateWorkspaceQueries } from "./agentWorkspaceQueries";

interface UseAgentWorkspacePublisherArgs {
  activeWorkspace: AgentConversationWorkspace | null;
  findConversationById: (conversationId: string) => AgentConversation | null;
  invalidateProjectConversations: (targetProjectId: string) => Promise<unknown>;
  optimisticWorkspacesByConversationId: Record<string, AgentConversationWorkspace>;
  queryClient: QueryClient;
  selectedConversationId: string | null;
}

export function useAgentWorkspacePublisher({
  activeWorkspace,
  findConversationById,
  invalidateProjectConversations,
  optimisticWorkspacesByConversationId,
  queryClient,
  selectedConversationId,
}: UseAgentWorkspacePublisherArgs) {
  const [publishingConversationId, setPublishingConversationId] = useState<string | null>(null);
  const handlePublishWorkspace = useCallback(
    async (conversationId: string) => {
      const conversation = findConversationById(conversationId);
      const workspace =
        selectedConversationId === conversationId
          ? activeWorkspace
          : optimisticWorkspacesByConversationId[conversationId] ?? null;
      const conversationTitle = conversation?.title?.trim() || null;
      setPublishingConversationId(conversationId);
      const publishToastId = agentWorkspaceOperationToastId(conversationId, "publish");
      try {
        const result = await chatApi.publishAgentConversationWorkspace(conversationId);
        const prLabel = result.prNumber ? `#${result.prNumber}` : result.prUrl;
        queryClient.setQueryData(
          ["agents", "conversation-workspace", conversationId],
          result.workspace
        );
        markAgentWorkspaceOperationToastSettled(publishToastId);
        toast.success(prLabel ? `Published ${prLabel}` : "Published branch", {
          ...(conversationTitle ? { description: conversationTitle } : {}),
          duration: AGENT_WORKSPACE_OPERATION_RESULT_DURATION_MS,
          id: publishToastId,
        });
        void Promise.all([
          invalidateWorkspaceQueries(queryClient, conversationId),
          conversation?.projectId
            ? invalidateProjectConversations(conversation.projectId)
            : Promise.resolve(),
        ]).catch(() => undefined);
      } catch (err) {
        const errorMessage = agentWorkspaceOperationErrorDetail(
          err,
          "Failed to publish branch",
        );
        let refreshedWorkspace: AgentConversationWorkspace | null = null;
        try {
          refreshedWorkspace = await chatApi.getAgentConversationWorkspace(conversationId);
          void invalidateWorkspaceQueries(queryClient, conversationId);
          if (refreshedWorkspace) {
            queryClient.setQueryData(
              ["agents", "conversation-workspace", conversationId],
              refreshedWorkspace
            );
          }
        } catch {
          refreshedWorkspace = null;
        }
        const publishFailureNeedsAgent =
          (refreshedWorkspace ?? workspace)?.publicationPushStatus === "needs_agent";

        if (publishFailureNeedsAgent) {
          const description = agentWorkspaceOperationToastDescription(
            conversationTitle,
            errorMessage,
          );
          markAgentWorkspaceOperationToastSettled(publishToastId);
          toast.error("Publish failed. Sent the error to the agent to fix.", {
            closeButton: true,
            ...(description ? { description } : {}),
            dismissible: true,
            duration: AGENT_WORKSPACE_OPERATION_ERROR_DURATION_MS,
            id: publishToastId,
          });
          if (conversation?.projectId) {
            await invalidateProjectConversations(conversation.projectId);
          }
          invalidateConversationDataQueries(queryClient, conversationId);
        } else {
          const description = agentWorkspaceOperationToastDescription(
            conversationTitle,
            errorMessage,
          );
          markAgentWorkspaceOperationToastSettled(publishToastId);
          toast.error("Failed to publish branch", {
            closeButton: true,
            ...(description ? { description } : {}),
            dismissible: true,
            duration: AGENT_WORKSPACE_OPERATION_ERROR_DURATION_MS,
            id: publishToastId,
          });
        }
      } finally {
        setPublishingConversationId(null);
      }
    },
    [
      activeWorkspace,
      findConversationById,
      invalidateProjectConversations,
      optimisticWorkspacesByConversationId,
      queryClient,
      selectedConversationId,
    ]
  );

  return {
    handlePublishWorkspace,
    publishingConversationId,
  };
}
