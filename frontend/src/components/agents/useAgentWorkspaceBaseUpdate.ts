import { useMutation, useQueryClient } from "@tanstack/react-query";
import { useCallback, useRef } from "react";

import {
  chatApi,
  type AgentConversationBaseSelection,
  type AgentConversationWorkspace,
} from "@/api/chat";

import {
  agentWorkspaceKeys,
  invalidateWorkspaceQueries,
} from "./agentWorkspaceQueries";
import {
  type AgentWorkspaceOperationToast,
  type AgentWorkspaceOperationToastKind,
  type AgentWorkspaceOperationToastResultOptions,
  agentWorkspaceOperationToastId,
  startAgentWorkspaceOperationToast,
} from "./agentWorkspaceOperationToast";

type WorkspaceBaseUpdateToastKind = Extract<
  AgentWorkspaceOperationToastKind,
  "rebase" | "update-from-base"
>;

export interface RunAgentWorkspaceBaseUpdateInput {
  baseSelection?: AgentConversationBaseSelection | null | undefined;
  conversationId: string;
  detail: string;
  kind: WorkspaceBaseUpdateToastKind;
  title: string;
  workspace?: AgentConversationWorkspace | null | undefined;
}

export function useAgentWorkspaceBaseUpdate({
  conversationTitle,
}: {
  conversationTitle?: string | null | undefined;
}) {
  const queryClient = useQueryClient();
  const progressToastRef = useRef<AgentWorkspaceOperationToast | null>(null);
  const toastConversationTitle = conversationTitle?.trim() || null;
  const { isPending, mutateAsync } = useMutation({
    mutationFn: ({
      baseSelection,
      conversationId,
    }: {
      baseSelection?: AgentConversationBaseSelection | null | undefined;
      conversationId: string;
    }) =>
      baseSelection
        ? chatApi.updateAgentConversationWorkspaceFromBase(
            conversationId,
            baseSelection,
          )
        : chatApi.updateAgentConversationWorkspaceFromBase(conversationId),
  });

  const settleProgressToast = useCallback(
    (
      progressToast: AgentWorkspaceOperationToast,
      outcome: "error" | "info" | "success",
      message: string,
      options?: AgentWorkspaceOperationToastResultOptions,
    ) => {
      if (progressToastRef.current === progressToast) {
        progressToastRef.current = null;
      }
      if (outcome === "success") {
        progressToast.success(message, options);
      } else if (outcome === "info") {
        progressToast.info(message, options);
      } else {
        progressToast.error(message, options);
      }
    },
    [],
  );

  const runUpdateFromBase = useCallback(
    ({
      baseSelection,
      conversationId,
      detail,
      kind,
      title,
      workspace,
    }: RunAgentWorkspaceBaseUpdateInput) => {
      const requestConversationId = conversationId;
      const requestWorkspace = workspace ?? null;
      progressToastRef.current?.dismiss();
      const progressToast = startAgentWorkspaceOperationToast({
        conversationTitle: toastConversationTitle,
        detail,
        id: agentWorkspaceOperationToastId(requestConversationId, kind),
        title,
      });
      progressToastRef.current = progressToast;

      void mutateAsync({ baseSelection, conversationId: requestConversationId })
        .then(async (result) => {
          queryClient.setQueryData(
            agentWorkspaceKeys.workspace(result.workspace.conversationId),
            result.workspace,
          );
          await invalidateWorkspaceQueries(
            queryClient,
            result.workspace.conversationId,
          );
          settleProgressToast(
            progressToast,
            "success",
            result.updated
              ? `Updated from ${result.targetRef}`
              : `Already current with ${result.targetRef}`,
          );
        })
        .catch(async (error) => {
          const errorMessage =
            error instanceof Error ? error.message : "Failed to update from base";
          let refreshedWorkspace: AgentConversationWorkspace | null = null;
          try {
            refreshedWorkspace =
              await chatApi.getAgentConversationWorkspace(requestConversationId);
            if (refreshedWorkspace) {
              queryClient.setQueryData(
                agentWorkspaceKeys.workspace(requestConversationId),
                refreshedWorkspace,
              );
            }
          } catch {
            refreshedWorkspace = null;
          }
          const repairStarted =
            (refreshedWorkspace ?? requestWorkspace)?.publicationPushStatus ===
            "needs_agent";
          settleProgressToast(
            progressToast,
            repairStarted ? "info" : "error",
            repairStarted ? "Repair started" : "Failed to update from base",
            { detail: errorMessage },
          );
          void invalidateWorkspaceQueries(queryClient, requestConversationId);
        });
    },
    [mutateAsync, queryClient, settleProgressToast, toastConversationTitle],
  );

  return {
    isUpdatingFromBase: isPending,
    runUpdateFromBase,
  };
}
