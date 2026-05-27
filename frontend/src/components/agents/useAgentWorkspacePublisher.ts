import { useCallback, useState } from "react";
import type { QueryClient } from "@tanstack/react-query";
import { toast } from "sonner";

import { chatApi } from "@/api/chat";
import type { AgentConversationWorkspace } from "@/api/chat";
import { invalidateConversationDataQueries } from "@/hooks/useChat";

import type { AgentConversation } from "./agentConversations";
import { agentWorkspaceOperationToastId } from "./agentWorkspaceOperationToast";
import { invalidateWorkspaceQueries } from "./agentWorkspaceQueries";

function getErrorMessage(error: unknown, fallback: string): string {
  if (error instanceof Error) {
    return error.message;
  }
  if (typeof error === "string" && error.trim()) {
    return error;
  }
  return fallback;
}

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
      setPublishingConversationId(conversationId);
      const publishToastId = agentWorkspaceOperationToastId(conversationId, "publish");
      try {
        const result = await chatApi.publishAgentConversationWorkspace(conversationId);
        const prLabel = result.prNumber ? `#${result.prNumber}` : result.prUrl;
        queryClient.setQueryData(
          ["agents", "conversation-workspace", conversationId],
          result.workspace
        );
        toast.success(prLabel ? `Published ${prLabel}` : "Published branch", {
          id: publishToastId,
        });
        void Promise.all([
          invalidateWorkspaceQueries(queryClient, conversationId),
          conversation?.projectId
            ? invalidateProjectConversations(conversation.projectId)
            : Promise.resolve(),
        ]).catch(() => undefined);
      } catch (err) {
        const errorMessage = getErrorMessage(err, "Failed to publish branch");
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
          toast.error("Publish failed. Sent the error to the agent to fix.", {
            id: publishToastId,
          });
          if (conversation?.projectId) {
            await invalidateProjectConversations(conversation.projectId);
          }
          invalidateConversationDataQueries(queryClient, conversationId);
        } else {
          toast.error(errorMessage, { id: publishToastId });
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
