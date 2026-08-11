import { useMutation, useQueryClient } from "@tanstack/react-query";
import { useCallback } from "react";

import {
  chatApi,
  type AgentConversationBaseSelection,
  type AgentConversationWorkspace,
  type AgentConversationWorkspaceFreshness,
} from "@/api/chat";

import type { AgentWorkspaceOperationToastKind } from "./agentWorkspaceOperationToast";
import {
  reportAgentWorkspaceOperationResult,
  watchAgentWorkspaceOperation,
} from "./agentWorkspaceOperationRegistry";
import {
  agentWorkspaceKeys,
  invalidateWorkspaceQueries,
} from "./agentWorkspaceQueries";

type WorkspaceBaseUpdateToastKind = Extract<
  AgentWorkspaceOperationToastKind,
  "rebase" | "update-from-base"
>;

export interface RetargetedAgentWorkspaceBaseSelection
  extends AgentConversationBaseSelection {
  retargetedFromPullRequest?: number | undefined;
}

export interface RunAgentWorkspaceBaseUpdateInput {
  baseSelection?: RetargetedAgentWorkspaceBaseSelection | null | undefined;
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
  const toastConversationTitle = conversationTitle?.trim() || null;
  const { isPending, mutateAsync } = useMutation({
    mutationFn: ({
      baseSelection,
      conversationId,
    }: {
      baseSelection?: RetargetedAgentWorkspaceBaseSelection | null | undefined;
      conversationId: string;
    }) =>
      baseSelection
        ? chatApi.updateAgentConversationWorkspaceFromBase(
            conversationId,
            baseSelection,
          )
        : chatApi.updateAgentConversationWorkspaceFromBase(conversationId),
  });

  const runUpdateFromBase = useCallback(
    (input: RunAgentWorkspaceBaseUpdateInput) => {
      const { baseSelection, conversationId, workspace } = input;
      const requestConversationId = conversationId;
      const requestWorkspace = workspace ?? null;

      watchAgentWorkspaceOperation({
        conversationId: requestConversationId,
        projectId: workspace?.projectId ?? null,
        conversationTitle: toastConversationTitle,
        kind: "base-update",
        startedAtMs: Date.now(),
      });

      void mutateAsync({ baseSelection, conversationId: requestConversationId })
        .then(async (result) => {
          queryClient.setQueryData(
            agentWorkspaceKeys.workspace(result.workspace.conversationId),
            result.workspace,
          );

          // Report before the awaited invalidate below yields control — the
          // driver's poll effect can observe "nothing in flight" and unwatch
          // this conversation in that window, orphaning the result if it were
          // reported afterward.
          if (result.repairStarted) {
            // A repair took over; if it is still live in workspace state, let
            // the driver keep rendering its progress instead of announcing a
            // terminal result over it.
            if (!result.workspace.maintenanceOperation) {
              reportAgentWorkspaceOperationResult(requestConversationId, {
                kind: "repair-started",
              });
            }
          } else if (!result.workspace.maintenanceOperation) {
            reportAgentWorkspaceOperationResult(requestConversationId, {
              kind: result.updated ? "base-updated" : "base-already-current",
              targetRef: result.targetRef,
            });
          }

          await invalidateWorkspaceQueries(
            queryClient,
            result.workspace.conversationId,
          );

          if (result.repairStarted || result.workspace.maintenanceOperation) {
            return;
          }

          for (const scope of ["full", "local"] as const) {
            queryClient.setQueryData<AgentConversationWorkspaceFreshness>(
              agentWorkspaceKeys.scopedFreshness(
                result.workspace.conversationId,
                scope,
              ),
              (previous) => {
                if (
                  !previous ||
                  previous.conversationId !== result.workspace.conversationId
                ) {
                  return previous;
                }
                return {
                  ...previous,
                  isBaseAhead: false,
                  capturedBaseCommit: result.baseCommit,
                  targetBaseCommit: result.baseCommit,
                  targetRef: result.targetRef,
                  baseStatus: result.baseStatus,
                };
              },
            );
          }
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
          const repairWorkspace = refreshedWorkspace ?? requestWorkspace;
          if (repairWorkspace?.maintenanceOperation) {
            // A repair took over; the driver keeps rendering its live
            // progress, so no terminal result is announced here.
            void invalidateWorkspaceQueries(queryClient, requestConversationId);
            return;
          }
          const repairStarted =
            repairWorkspace?.publicationPushStatus === "needs_agent";
          reportAgentWorkspaceOperationResult(
            requestConversationId,
            repairStarted
              ? { kind: "repair-started", detail: errorMessage }
              : { kind: "base-update-failed", detail: errorMessage },
          );
          void invalidateWorkspaceQueries(queryClient, requestConversationId);
        });
    },
    [mutateAsync, queryClient, toastConversationTitle],
  );

  return {
    isUpdatingFromBase: isPending,
    runUpdateFromBase,
  };
}
