import { useMutation, useQueryClient } from "@tanstack/react-query";
import { useCallback, useEffect, useRef } from "react";

import {
  chatApi,
  type AgentConversationBaseSelection,
  type AgentConversationWorkspace,
  type AgentConversationWorkspaceFreshness,
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
  agentWorkspaceMaintenanceOperationToastId,
  clearAgentWorkspaceOperationToastDismissal,
  isAgentWorkspaceOperationToastDismissed,
  maintenanceOperationToastLabel,
  startAgentWorkspaceOperationToast,
} from "./agentWorkspaceOperationToast";

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
  const progressToastRef = useRef<AgentWorkspaceOperationToast | null>(null);
  const maintenanceToastRef = useRef<{
    conversationId: string;
    operationId: string;
    toast: AgentWorkspaceOperationToast;
  } | null>(null);
  const settledMaintenanceOperationIdsRef = useRef(new Set<string>());
  const toastConversationTitle = conversationTitle?.trim() || null;
  useEffect(
    () => () => {
      // The update-from-base progress toast stays connected across unmount:
      // the pending mutation closure settles it. Only the poll-driven
      // maintenance toast has no settlement path after unmount.
      maintenanceToastRef.current?.toast.dismiss();
      maintenanceToastRef.current = null;
    },
    [],
  );
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

  const syncMaintenanceOperation = useCallback(
    (
      workspace: AgentConversationWorkspace | null | undefined,
      allowTerminalStart = false,
    ) => {
      const operation = workspace?.maintenanceOperation;
      const current = maintenanceToastRef.current;
      if (!operation) {
        if (current) {
          maintenanceToastRef.current = null;
          void chatApi
            .getAgentConversationWorkspace(current.conversationId)
            .then((refreshedWorkspace) => {
              if (refreshedWorkspace) {
                queryClient.setQueryData(
                  agentWorkspaceKeys.workspace(current.conversationId),
                  refreshedWorkspace,
                );
              }

              const refreshedOperation = refreshedWorkspace?.maintenanceOperation;
              if (refreshedOperation?.status === "ready") {
                settledMaintenanceOperationIdsRef.current.add(
                  `${current.conversationId}:${refreshedOperation.operationId}`,
                );
                current.toast.success("Base updated — ready to publish", {
                  detail: refreshedOperation.blocker ?? refreshedOperation.summary,
                });
                return;
              }
              if (refreshedOperation?.status === "blocked") {
                settledMaintenanceOperationIdsRef.current.add(
                  `${current.conversationId}:${refreshedOperation.operationId}`,
                );
                current.toast.error("Repair blocked", {
                  detail: refreshedOperation.blocker ?? refreshedOperation.summary,
                });
                return;
              }
              if (refreshedOperation?.status === "active") {
                return;
              }

              current.toast.success(
                refreshedWorkspace?.publicationPrNumber
                  ? `Published pull request #${refreshedWorkspace.publicationPrNumber}`
                  : "Workspace operation completed",
              );
            })
            .catch(() => {
              current.toast.info("Couldn't verify workspace operation", {
                detail:
                  "Check the workspace publish panel, then retry after reconnecting.",
              });
            });
        }
        return;
      }

      const operationKey = `${workspace.conversationId}:${operation.operationId}`;
      if (settledMaintenanceOperationIdsRef.current.has(operationKey)) {
        return;
      }
      const id = agentWorkspaceMaintenanceOperationToastId(
        workspace.conversationId,
        operation.operationId,
      );
      const isNewMaintenanceToast =
        !current ||
        current.conversationId !== workspace.conversationId ||
        current.operationId !== operation.operationId;
      if (
        isNewMaintenanceToast &&
        isAgentWorkspaceOperationToastDismissed(id)
      ) {
        if (operation.status === "ready" || operation.status === "blocked") {
          settledMaintenanceOperationIdsRef.current.add(operationKey);
          clearAgentWorkspaceOperationToastDismissal(id);
        }
        return;
      }
      if (!current && operation.status !== "active" && !allowTerminalStart) {
        return;
      }
      const title = maintenanceOperationToastLabel(operation.stage);
      const detail = operation.blocker ?? operation.summary;
      let toast: AgentWorkspaceOperationToast;
      if (isNewMaintenanceToast) {
        current?.toast.dismiss();
        progressToastRef.current?.dismiss();
        toast = startAgentWorkspaceOperationToast({
          conversationTitle: toastConversationTitle,
          detail,
          id,
          startedAtMs: new Date(operation.startedAt).getTime(),
          targetConversation: {
            conversationId: workspace.conversationId,
            projectId: workspace.projectId,
          },
          title,
        });
        maintenanceToastRef.current = {
          conversationId: workspace.conversationId,
          operationId: operation.operationId,
          toast,
        };
      } else {
        toast = current.toast;
        toast.update({ detail, id, title });
      }

      if (operation.status === "ready") {
        settledMaintenanceOperationIdsRef.current.add(operationKey);
        maintenanceToastRef.current = null;
        toast.success("Base updated — ready to publish", { detail });
      } else if (operation.status === "blocked") {
        settledMaintenanceOperationIdsRef.current.add(operationKey);
        maintenanceToastRef.current = null;
        toast.error("Repair blocked", { detail });
      }
    },
    [queryClient, toastConversationTitle],
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
      const progressDetail = baseSelection?.retargetedFromPullRequest
        ? `PR #${baseSelection.retargetedFromPullRequest} merged — rebasing onto ${baseSelection.ref}`
        : detail;
      progressToastRef.current?.dismiss();
      const progressToast = startAgentWorkspaceOperationToast({
        conversationTitle: toastConversationTitle,
        detail: progressDetail,
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
          if (result.repairStarted) {
            settleProgressToast(
              progressToast,
              "info",
              "Repair started",
              {
                detail:
                  "A new workspace repair attempt has started and will continue automatically.",
              },
            );
            if (result.workspace.maintenanceOperation) {
              syncMaintenanceOperation(result.workspace, true);
            }
            return;
          }
          if (result.workspace.maintenanceOperation) {
            syncMaintenanceOperation(result.workspace, true);
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
          const repairWorkspace = refreshedWorkspace ?? requestWorkspace;
          if (repairWorkspace?.maintenanceOperation) {
            syncMaintenanceOperation(repairWorkspace, true);
            void invalidateWorkspaceQueries(queryClient, requestConversationId);
            return;
          }
          const repairStarted = repairWorkspace?.publicationPushStatus === "needs_agent";
          settleProgressToast(
            progressToast,
            repairStarted ? "info" : "error",
            repairStarted ? "Repair started" : "Failed to update from base",
            { detail: errorMessage },
          );
          void invalidateWorkspaceQueries(queryClient, requestConversationId);
        });
    },
    [
      mutateAsync,
      queryClient,
      settleProgressToast,
      syncMaintenanceOperation,
      toastConversationTitle,
    ],
  );

  return {
    isUpdatingFromBase: isPending,
    runUpdateFromBase,
    syncMaintenanceOperation,
  };
}
