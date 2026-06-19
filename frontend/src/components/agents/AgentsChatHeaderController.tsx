import { useCallback, useEffect, useRef, useState } from "react";
import { useMutation, useQuery } from "@tanstack/react-query";
import { toast } from "sonner";

import { chatApi } from "@/api/chat";
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
    | "onToggleIssuePane"
  > {
  hasAutoOpenArtifacts: boolean;
  issuePaneOpen: boolean;
  onToggleArtifacts: (conversationId: string) => void;
  onToggleIssuePane: (conversationId: string) => void;
}

export function AgentsChatHeaderController({
  conversation,
  workspace,
  hasAutoOpenArtifacts,
  issuePaneOpen,
  terminalUnavailableReason = null,
  onToggleArtifacts,
  onToggleIssuePane,
  ...props
}: AgentsChatHeaderControllerProps) {
  const { artifactState, artifactPaneOpen } = useResolvedAgentArtifactState(
    conversation?.id ?? null,
    hasAutoOpenArtifacts,
  );
  const terminalOpen = useAgentTerminalStore((state) =>
    conversation?.id ? state.openByConversationId[conversation.id] ?? false : false,
  );
  const toggleTerminalOpen = useAgentTerminalStore((state) => state.toggleOpen);
  const terminalPreloadJobRef = useRef<DeferredFrameJob | null>(null);
  const workspaceOpenStartedAtRef = useRef(0);
  const workspaceOpenClearTimerRef = useRef<number | null>(null);
  const [visibleWorkspaceOpeningTargetId, setVisibleWorkspaceOpeningTargetId] =
    useState<string | null>(null);
  const workspaceOpenTargetsQuery = useQuery({
    queryKey: ["workspace-open-targets"],
    queryFn: chatApi.listWorkspaceOpenTargets,
    staleTime: Infinity,
    enabled: Boolean(conversation && workspace),
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
      if (!conversation) {
        return Promise.resolve();
      }
      return chatApi.openAgentConversationWorkspace(conversation.id, targetId);
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
    if (!conversation || terminalUnavailableReason) {
      return;
    }
    const nextOpen = !terminalOpen;
    toggleTerminalOpen(conversation.id);
    if (nextOpen) {
      scheduleTerminalPreload();
    } else {
      cancelTerminalPreloadJob();
    }
  }, [
    cancelTerminalPreloadJob,
    conversation,
    scheduleTerminalPreload,
    terminalOpen,
    terminalUnavailableReason,
    toggleTerminalOpen,
  ]);
  const handleToggleArtifacts = useCallback(() => {
    if (!conversation) {
      return;
    }
    onToggleArtifacts(conversation.id);
  }, [conversation, onToggleArtifacts]);
  const handleToggleIssuePane = useCallback(() => {
    if (!conversation) {
      return;
    }
    onToggleIssuePane(conversation.id);
  }, [conversation, onToggleIssuePane]);
  const handleOpenWorkspaceTarget = useCallback(
    (targetId: string) => {
      if (!conversation || visibleWorkspaceOpeningTargetId) {
        return;
      }
      openWorkspaceMutation.mutate(targetId);
    },
    [conversation, openWorkspaceMutation, visibleWorkspaceOpeningTargetId],
  );

  return (
    <AgentsChatHeader
      {...props}
      conversation={conversation}
      workspace={workspace}
      artifactOpen={artifactPaneOpen}
      activeArtifactTab={artifactState.activeTab}
      issuePaneOpen={issuePaneOpen}
      terminalOpen={terminalOpen}
      terminalUnavailableReason={terminalUnavailableReason}
      onToggleTerminal={handleToggleTerminal}
      onPreloadTerminal={handlePreloadTerminal}
      workspaceOpenTargets={workspaceOpenTargetsQuery.data ?? []}
      openingWorkspaceTargetId={visibleWorkspaceOpeningTargetId}
      onOpenWorkspaceTarget={handleOpenWorkspaceTarget}
      onToggleArtifacts={handleToggleArtifacts}
      onToggleIssuePane={handleToggleIssuePane}
    />
  );
}
