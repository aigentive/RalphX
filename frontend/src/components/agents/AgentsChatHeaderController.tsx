import { useCallback, useEffect, useRef, useState } from "react";
import { useMutation, useQuery } from "@tanstack/react-query";
import { toast } from "sonner";

import { chatApi } from "@/api/chat";
import { useIsRemoteEnvironment } from "@/hooks/useActiveEnvironment";
import { isRemotelyAvailable } from "@/lib/remote/agent-gate";
import { useResolvedAgentArtifactState } from "./agentArtifactState";
import {
  cancelDeferredFrameJob,
  scheduleDeferredFrameJob,
  type DeferredFrameJob,
} from "./agentDeferredFrame";
import {
  AgentsChatHeader,
  type AgentsChatHeaderProps,
} from "./AgentsChatHeader";
import { preloadAgentTerminalExperience } from "./agentTerminalPreload";
import { useAgentTerminalStore } from "./agentTerminalStore";

const WORKSPACE_OPEN_MIN_VISIBLE_MS = 2500;

function workspaceOpenErrorMessage(error: unknown): string {
  if (typeof error === "string" && error.trim()) {
    return error;
  }
  if (error instanceof Error && error.message.trim()) {
    return error.message;
  }
  return "Failed to open workspace";
}

interface AgentsChatHeaderControllerProps
  extends Omit<
    AgentsChatHeaderProps,
    | "artifactOpen"
    | "activeArtifactTab"
    | "onToggleArtifacts"
    | "terminalOpen"
    | "onToggleTerminal"
    | "onPreloadTerminal"
  > {
  hasAutoOpenArtifacts: boolean;
  onToggleArtifacts: (conversationId: string) => void;
}

export function AgentsChatHeaderController({
  conversation,
  workspace,
  hasAutoOpenArtifacts,
  terminalArchivedReason = null,
  terminalUnavailableReason = null,
  onToggleArtifacts,
  ...props
}: AgentsChatHeaderControllerProps) {
  const { artifactState, artifactPaneOpen } = useResolvedAgentArtifactState(
    conversation?.id ?? null,
    hasAutoOpenArtifacts,
  );
  const isRemoteEnvironment = useIsRemoteEnvironment();
  const workspaceConversationId =
    workspace?.conversationId ?? conversation?.id ?? null;
  const terminalOpen = useAgentTerminalStore((state) =>
    workspaceConversationId
      ? state.openByConversationId[workspaceConversationId] ?? false
      : false,
  );
  const toggleTerminalOpen = useAgentTerminalStore((state) => state.toggleOpen);
  const registerTerminalConversation = useAgentTerminalStore(
    (state) => state.registerConversation,
  );
  const terminalPreloadJobRef = useRef<DeferredFrameJob | null>(null);
  const workspaceOpenStartedAtRef = useRef(0);
  const workspaceOpenClearTimerRef = useRef<number | null>(null);
  const [visibleWorkspaceOpeningTargetId, setVisibleWorkspaceOpeningTargetId] =
    useState<string | null>(null);
  const workspaceOpenTargetsQuery = useQuery({
    queryKey: ["workspace-open-targets"],
    queryFn: chatApi.listWorkspaceOpenTargets,
    staleTime: Infinity,
    enabled: Boolean(
      conversation &&
        workspace &&
        (!isRemoteEnvironment || isRemotelyAvailable("list_workspace_open_targets")),
    ),
  });
  const clearWorkspaceOpenTimer = useCallback(() => {
    if (workspaceOpenClearTimerRef.current !== null) {
      window.clearTimeout(workspaceOpenClearTimerRef.current);
      workspaceOpenClearTimerRef.current = null;
    }
  }, []);
  const clearVisibleWorkspaceOpeningTarget = useCallback(() => {
    clearWorkspaceOpenTimer();
    setVisibleWorkspaceOpeningTargetId(null);
  }, [clearWorkspaceOpenTimer]);
  const openWorkspaceMutation = useMutation({
    mutationFn: (targetId: string) => {
      if (!workspaceConversationId) {
        return Promise.resolve();
      }
      return chatApi.openAgentConversationWorkspace(
        workspaceConversationId,
        targetId,
      );
    },
    onMutate: (targetId) => {
      clearWorkspaceOpenTimer();
      workspaceOpenStartedAtRef.current = window.performance.now();
      setVisibleWorkspaceOpeningTargetId(targetId);
    },
    onSuccess: () => {
      const elapsedMs = window.performance.now() - workspaceOpenStartedAtRef.current;
      const remainingMs = Math.max(0, WORKSPACE_OPEN_MIN_VISIBLE_MS - elapsedMs);
      workspaceOpenClearTimerRef.current = window.setTimeout(() => {
        workspaceOpenClearTimerRef.current = null;
        setVisibleWorkspaceOpeningTargetId(null);
      }, remainingMs);
    },
    onError: (error) => {
      clearVisibleWorkspaceOpeningTarget();
      toast.error(workspaceOpenErrorMessage(error));
    },
  });

  const cancelTerminalPreloadJob = useCallback(() => {
    cancelDeferredFrameJob(terminalPreloadJobRef.current);
    terminalPreloadJobRef.current = null;
  }, []);

  useEffect(
    () => () => {
      cancelTerminalPreloadJob();
      clearWorkspaceOpenTimer();
    },
    [cancelTerminalPreloadJob, clearWorkspaceOpenTimer],
  );

  useEffect(() => {
    if (!conversation || !workspace || !workspaceConversationId) {
      return;
    }

    registerTerminalConversation({
      conversationId: workspaceConversationId,
      projectId: workspace.projectId,
      title: conversation.title ?? null,
      branchName: workspace.branchName,
      worktreePath: workspace.worktreePath,
      updatedAt: workspace.updatedAt,
    });
  }, [
    conversation,
    registerTerminalConversation,
    workspace,
    workspaceConversationId,
  ]);

  const handlePreloadTerminal = useCallback(() => {
    cancelTerminalPreloadJob();
    preloadAgentTerminalExperience();
  }, [cancelTerminalPreloadJob]);

  const scheduleTerminalPreload = useCallback(() => {
    cancelTerminalPreloadJob();
    terminalPreloadJobRef.current = scheduleDeferredFrameJob(() => {
      terminalPreloadJobRef.current = null;
      preloadAgentTerminalExperience();
    });
  }, [cancelTerminalPreloadJob]);

  const handleToggleTerminal = useCallback(() => {
    if (!workspaceConversationId || terminalUnavailableReason) {
      return;
    }
    const nextOpen = !terminalOpen;
    toggleTerminalOpen(workspaceConversationId);
    if (nextOpen && !terminalArchivedReason) {
      scheduleTerminalPreload();
    } else {
      cancelTerminalPreloadJob();
    }
  }, [
    cancelTerminalPreloadJob,
    scheduleTerminalPreload,
    terminalArchivedReason,
    terminalOpen,
    terminalUnavailableReason,
    toggleTerminalOpen,
    workspaceConversationId,
  ]);
  const handleToggleArtifacts = useCallback(() => {
    if (!conversation) {
      return;
    }
    onToggleArtifacts(conversation.id);
  }, [conversation, onToggleArtifacts]);
  const handleOpenWorkspaceTarget = useCallback(
    (targetId: string) => {
      if (!workspaceConversationId || visibleWorkspaceOpeningTargetId) {
        return;
      }
      openWorkspaceMutation.mutate(targetId);
    },
    [
      openWorkspaceMutation,
      visibleWorkspaceOpeningTargetId,
      workspaceConversationId,
    ],
  );

  return (
    <AgentsChatHeader
      {...props}
      conversation={conversation}
      workspace={workspace}
      artifactOpen={artifactPaneOpen}
      activeArtifactTab={artifactState.activeTab}
      terminalOpen={terminalOpen}
      terminalArchivedReason={terminalArchivedReason}
      terminalUnavailableReason={terminalUnavailableReason}
      onToggleTerminal={handleToggleTerminal}
      onPreloadTerminal={handlePreloadTerminal}
      workspaceOpenTargets={
        isRemoteEnvironment && !isRemotelyAvailable("list_workspace_open_targets")
          ? [{ id: "host", label: "Host workspace", kind: "editor" }]
          : (workspaceOpenTargetsQuery.data ?? [])
      }
      openingWorkspaceTargetId={visibleWorkspaceOpeningTargetId}
      onOpenWorkspaceTarget={handleOpenWorkspaceTarget}
      onToggleArtifacts={handleToggleArtifacts}
    />
  );
}
