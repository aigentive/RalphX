import { useCallback, useEffect, useMemo, useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";

import { ideationApi } from "@/api/ideation";
import { useAgentModels } from "@/hooks/useAgentModels";
import { useProjects } from "@/hooks/useProjects";
import { useAgentArtifactController } from "./useAgentArtifactController";
import { useAgentConversationTitleEvents } from "./useAgentConversationTitleEvents";
import { useAgentArtifactResize } from "./useAgentArtifactResize";
import { useAgentsSelectionModel } from "./useAgentsSelectionModel";
import { useAgentsWorkspaceModel } from "./useAgentsWorkspaceModel";
import { useAgentsAttachedIdeation } from "./useAgentsAttachedIdeation";
import { useAgentsAutoTitle } from "./useAgentsAutoTitle";
import { useAgentsActiveComposerControls } from "./useAgentsActiveComposerControls";
import { useAgentWorkspacePublisher } from "./useAgentWorkspacePublisher";
import { useStartAgentConversation } from "./useStartAgentConversation";
import { useAgentConversationLookup } from "./useAgentConversationLookup";
import { useAgentConversationActions } from "./useAgentConversationActions";
import { useAgentArtifactActions } from "./useAgentArtifactActions";
import { useAgentConversationInvalidation } from "./useAgentConversationInvalidation";
import { useAgentUserMessageAutoTitle } from "./useAgentUserMessageAutoTitle";
import { useAgentsSessionBindings } from "./useAgentsSessionBindings";
import { useSyncedAgentProjectFocus } from "./useSyncedAgentProjectFocus";
import { useAgentsOptimisticState } from "./useAgentsOptimisticState";
import { useAgentsTerminalDocks } from "./useAgentsTerminalDocks";
import { useAgentsSidebarState } from "./useAgentsSidebarState";
import { useAgentsSidebarProps } from "./useAgentsSidebarProps";
import { normalizeRuntimeSelection } from "./agentOptions";
import {
  getFocusedArtifactIdeationSessionId,
  type AgentsChatFocus,
  type AgentsChatFocusSwitchOption,
  type AgentsChatFocusType,
} from "./agentChatFocus";
import type { AgentRuntimeSelection } from "@/stores/agentSessionStore";

interface UseAgentsViewControllerParams {
  projectId: string;
  onCreateProject: () => void;
}

export function useAgentsViewController({
  projectId,
  onCreateProject,
}: UseAgentsViewControllerParams) {
  const queryClient = useQueryClient();
  const [chatFocus, setChatFocus] = useState<AgentsChatFocus>({ type: "workspace" });
  const [lastVerificationFocus, setLastVerificationFocus] = useState<Extract<
    AgentsChatFocus,
    { type: "verification" }
  > | null>(null);
  const {
    closeSidebarOverlay,
    isSidebarCollapsed,
    isSidebarOverlayOpen,
    setShowArchived,
    showArchived,
    sidebarWidth,
    suppressSidebarTransition,
    toggleSidebarCollapse,
  } = useAgentsSidebarState();
  const {
    optimisticConversationsById,
    optimisticSelectedConversationId,
    optimisticWorkspacesByConversationId,
    setOptimisticConversationsById,
    setOptimisticSelectedConversationId,
    setOptimisticWorkspacesByConversationId,
  } = useAgentsOptimisticState();
  const {
    artifactWidthCss,
    handleArtifactResizeReset,
    handleArtifactResizeStart,
    isArtifactResizing,
    splitContainerRef,
  } = useAgentArtifactResize();
  const { data: projects = [], isLoading: isLoadingProjects } = useProjects();
  const { registry: modelRegistry } = useAgentModels();
  const {
    clearAgentConversationSelection,
    focusedProjectId,
    lastRuntimeByProjectId,
    runtimeByConversationId,
    selectConversation,
    selectedProjectId,
    setActiveConversation,
    setFocusedProject,
    setLastRuntimeForProject,
    setRuntimeForConversation,
    storedSelectedConversationId,
  } = useAgentsSessionBindings({
    setOptimisticSelectedConversationId,
  });
  const {
    setTerminalChatDockElement,
    setTerminalPanelDockElement,
    terminalChatDockElement,
    terminalPanelDockElement,
  } = useAgentsTerminalDocks();
  const {
    activeConversation,
    activeProjectId,
    defaultProjectId,
    focusedConversations,
    selectedConversationFallback,
    selectedConversationId,
    selectedConversationMessages,
  } = useAgentsSelectionModel({
    clearAgentConversationSelection,
    focusedProjectId,
    optimisticConversationsById,
    optimisticSelectedConversationId,
    projectId,
    projects,
    selectedProjectId,
    showArchived,
    storedSelectedConversationId,
  });
  useEffect(() => {
    setChatFocus({ type: "workspace" });
    setLastVerificationFocus(null);
  }, [selectedConversationId]);
  const focusedArtifactIdeationSessionId =
    getFocusedArtifactIdeationSessionId(chatFocus);
  const handleFocusIdeationSession = useCallback((sessionId: string) => {
    setChatFocus((current) =>
      current.type === "ideation" && current.sessionId === sessionId
        ? current
        : { type: "ideation", sessionId },
    );
  }, []);
  const handleFocusVerificationSession = useCallback(
    (parentSessionId: string, childSessionId: string) => {
      const nextFocus: Extract<AgentsChatFocus, { type: "verification" }> = {
        type: "verification",
        parentSessionId,
        childSessionId,
      };
      setLastVerificationFocus(nextFocus);
      setChatFocus((current) =>
        current.type === "verification" &&
        current.parentSessionId === parentSessionId &&
        current.childSessionId === childSessionId
          ? current
          : nextFocus,
      );
    },
    [],
  );
  const handleReturnToWorkspaceChat = useCallback(() => {
    setChatFocus({ type: "workspace" });
  }, []);
  const {
    activeConversationMode,
    activeConversationModeLocked,
    activeWorkspace,
    normalizedActiveRuntime,
    publishShortcutLabel,
    terminalUnavailableReason,
  } = useAgentsWorkspaceModel({
    activeConversation,
    modelRegistry,
    optimisticWorkspacesByConversationId,
    runtimeByConversationId,
    selectedConversationId,
  });
  useAgentConversationTitleEvents(activeProjectId);
  useSyncedAgentProjectFocus(projectId, setFocusedProject);

  const findConversationById = useAgentConversationLookup({
    focusedConversations,
    selectedConversationFallback,
  });

  const invalidateProjectConversations = useAgentConversationInvalidation(queryClient);
  const {
    attachedIdeationSessionId,
    availableArtifactTabs,
    hasAutoOpenArtifacts,
  } = useAgentsAttachedIdeation({
    activeConversation,
    activeConversationMode,
    activeWorkspace,
    invalidateProjectConversations,
    selectedConversationMessages,
  });
  const knownFocusIdeationSessionId =
    focusedArtifactIdeationSessionId ?? attachedIdeationSessionId ?? null;
  const latestVerificationChildQuery = useQuery({
    queryKey: [
      "agents",
      "chat-focus",
      "latest-child-session-id",
      knownFocusIdeationSessionId,
      "verification",
    ],
    queryFn: () =>
      ideationApi.sessions.getLatestChildSessionId(
        knownFocusIdeationSessionId!,
        "verification",
        { includeArchived: true },
      ),
    enabled: Boolean(knownFocusIdeationSessionId),
    staleTime: 5_000,
  });
  const latestVerificationChildSessionId =
    latestVerificationChildQuery.data?.latestChildSessionId ?? null;
  useEffect(() => {
    if (!knownFocusIdeationSessionId || !latestVerificationChildQuery.isSuccess) {
      return;
    }
    if (!latestVerificationChildSessionId) {
      setLastVerificationFocus((current) =>
        current?.parentSessionId === knownFocusIdeationSessionId ? null : current,
      );
      return;
    }
    const nextFocus: Extract<AgentsChatFocus, { type: "verification" }> = {
      type: "verification",
      parentSessionId: knownFocusIdeationSessionId,
      childSessionId: latestVerificationChildSessionId,
    };
    setLastVerificationFocus((current) =>
      current?.parentSessionId === nextFocus.parentSessionId &&
      current.childSessionId === nextFocus.childSessionId
        ? current
        : nextFocus,
    );
  }, [
    knownFocusIdeationSessionId,
    latestVerificationChildQuery.isSuccess,
    latestVerificationChildSessionId,
  ]);
  const focusSwitcherIdeationSessionId =
    knownFocusIdeationSessionId ??
    lastVerificationFocus?.parentSessionId ??
    null;
  const verificationFocusTarget =
    lastVerificationFocus &&
    lastVerificationFocus.parentSessionId === focusSwitcherIdeationSessionId
      ? lastVerificationFocus
      : null;
  const chatFocusOptions = useMemo(() => {
    const options: AgentsChatFocusSwitchOption[] = [
      {
        type: "workspace" as const,
        label: "Workspace",
        description: "Show the workspace agent chat",
      },
    ];

    if (focusSwitcherIdeationSessionId) {
      options.push({
        type: "ideation" as const,
        label: "Ideation",
        description: "Show the attached ideation chat",
        tone: "accent" as const,
      });
    }

    if (verificationFocusTarget) {
      options.push({
        type: "verification" as const,
        label: "Verification",
        description: "Show the verification agent chat",
        tone: "warning" as const,
      });
    }

    return options;
  }, [focusSwitcherIdeationSessionId, verificationFocusTarget]);
  const handleSelectChatFocus = useCallback(
    (type: AgentsChatFocusType) => {
      if (type === "workspace") {
        handleReturnToWorkspaceChat();
        return;
      }

      if (type === "ideation") {
        if (focusSwitcherIdeationSessionId) {
          handleFocusIdeationSession(focusSwitcherIdeationSessionId);
        }
        return;
      }

      if (verificationFocusTarget) {
        setChatFocus(verificationFocusTarget);
      }
    },
    [
      focusSwitcherIdeationSessionId,
      handleFocusIdeationSession,
      handleReturnToWorkspaceChat,
      verificationFocusTarget,
    ],
  );
  const {
    openArtifactTab,
    scheduleArtifactPanePreload,
    setArtifactPaneVisibility,
    setArtifactTaskMode,
    toggleArtifactPaneVisibility,
  } = useAgentArtifactController({
    hasAutoOpenArtifacts,
    selectedConversationId,
  });

  const { clearAutoManagedTitle, handleAutoManagedTitle } = useAgentsAutoTitle({
    findConversationById,
    invalidateProjectConversations,
  });

  const handleStartAgentConversation = useStartAgentConversation({
    handleAutoManagedTitle,
    invalidateProjectConversations,
    queryClient,
    selectConversation,
    setActiveConversation,
    setFocusedProject,
    setOptimisticConversationsById,
    setOptimisticSelectedConversationId,
    setOptimisticWorkspacesByConversationId,
    setRuntimeForConversation,
  });

  const {
    handleArchiveConversation,
    handleArchiveProject,
    handleRenameConversation,
    handleRestoreConversation,
    handleSidebarCreateAgent,
    handleSidebarFocusProject,
    handleSidebarSelectConversation,
  } = useAgentConversationActions({
    activeProjectId,
    clearAgentConversationSelection,
    clearAutoManagedTitle,
    closeSidebarOverlay,
    findConversationById,
    focusedProjectId,
    invalidateProjectConversations,
    isSidebarOverlayOpen,
    projectId,
    projects,
    queryClient,
    selectConversation,
    selectedConversationId,
    selectedProjectId,
    setActiveConversation,
    setFocusedProject,
    setOptimisticSelectedConversationId,
  });

  const {
    handleOpenPublishPane,
    handlePreloadArtifacts,
    handleSelectArtifact,
  } = useAgentArtifactActions({
    hasAutoOpenArtifacts,
    openArtifactTab,
    scheduleArtifactPanePreload,
    selectedConversationId,
    setArtifactPaneVisibility,
  });
  // Switching artifact tabs no longer touches the chat focus. The user
  // toggles between workspace and child chats explicitly via the composer
  // chat-focus pill.
  const handleSelectArtifactWithChatFocus = handleSelectArtifact;

  const handleAgentUserMessageSent = useAgentUserMessageAutoTitle({
    activeProjectId,
    findConversationById,
    handleAutoManagedTitle,
    selectedConversationId,
  });
  const handleStartRuntimePreferenceChange = useCallback(
    (targetProjectId: string, runtime: AgentRuntimeSelection) => {
      setLastRuntimeForProject(
        targetProjectId,
        normalizeRuntimeSelection(runtime, modelRegistry),
      );
    },
    [modelRegistry, setLastRuntimeForProject],
  );

  const { handlePublishWorkspace, publishingConversationId } =
    useAgentWorkspacePublisher({
      activeWorkspace,
      findConversationById,
      invalidateProjectConversations,
      optimisticWorkspacesByConversationId,
      queryClient,
      selectedConversationId,
    });

  const {
    activeProjectOptions,
    defaultRuntime,
    handleActiveConversationModeChange,
    handleActiveEffortChange,
    handleActiveModelChange,
    switchingConversationModeId,
  } = useAgentsActiveComposerControls({
    activeConversation,
    activeConversationModeLocked,
    activeProjectId,
    activeWorkspace,
    defaultProjectId,
    invalidateProjectConversations,
    lastRuntimeByProjectId,
    modelRegistry,
    normalizedActiveRuntime,
    projects,
    queryClient,
    runtimeByConversationId,
    selectedConversationId,
    setRuntimeForConversation,
  });

  const sidebarProps = useAgentsSidebarProps({
    projects,
    defaultProjectId,
    focusedProjectId,
    selectedConversationId,
    pinnedConversation: selectedConversationFallback,
    onFocusProject: handleSidebarFocusProject,
    onSelectConversation: handleSidebarSelectConversation,
    onCreateAgent: handleSidebarCreateAgent,
    onCreateProject,
    onArchiveProject: handleArchiveProject,
    onRenameConversation: handleRenameConversation,
    onArchiveConversation: handleArchiveConversation,
    onRestoreConversation: handleRestoreConversation,
    showArchived,
    onShowArchivedChange: setShowArchived,
  });

  return {
    mainRegionProps: {
      activeConversation,
      activeConversationMode,
      activeConversationModeLocked,
      activeProjectId,
      activeProjectOptions,
      activeWorkspace,
      attachedIdeationSessionId,
      availableArtifactTabs,
      chatFocus,
      chatFocusOptions,
      defaultProjectId,
      defaultRuntime,
      hasAutoOpenArtifacts,
      isLoadingProjects,
      normalizedActiveRuntime,
      onActiveConversationModeChange: handleActiveConversationModeChange,
      onActiveEffortChange: handleActiveEffortChange,
      onActiveModelChange: handleActiveModelChange,
      onAgentUserMessageSent: handleAgentUserMessageSent,
      onCreateProject,
      onFocusIdeationSession: handleFocusIdeationSession,
      onOpenPublishPane: handleOpenPublishPane,
      onPreloadArtifacts: handlePreloadArtifacts,
      onPublishWorkspace: handlePublishWorkspace,
      onRenameConversation: handleRenameConversation,
      onRuntimePreferenceChange: handleStartRuntimePreferenceChange,
      onSelectArtifact: handleSelectArtifactWithChatFocus,
      onStartAgentConversation: handleStartAgentConversation,
      onToggleArtifacts: toggleArtifactPaneVisibility,
      onSelectChatFocus: handleSelectChatFocus,
      projects,
      publishShortcutLabel,
      publishingConversationId,
      selectedConversationId,
      setTerminalChatDockElement,
      switchingConversationModeId,
      terminalUnavailableReason,
    },
    shellProps: {
      isSidebarCollapsed,
      isSidebarOverlayOpen,
      onCloseSidebarOverlay: closeSidebarOverlay,
      onToggleSidebarCollapse: toggleSidebarCollapse,
      sidebarProps,
      sidebarWidth,
      splitContainerRef,
      suppressSidebarTransition,
    },
    sideRegionProps: {
      activeConversation,
      activeWorkspace,
      artifactWidthCss,
      chatDockElement: terminalChatDockElement,
      focusedIdeationSessionId: focusedArtifactIdeationSessionId,
      hasAutoOpenArtifacts,
      isArtifactResizing,
      openArtifactTab,
      panelDockElement: terminalPanelDockElement,
      publishingConversationId,
      selectedConversationId,
      setArtifactPaneVisibility,
      setArtifactTaskMode,
      setTerminalPanelDockElement,
      terminalUnavailableReason,
      onFocusVerificationSession: handleFocusVerificationSession,
      onPublishWorkspace: handlePublishWorkspace,
      onResizeReset: handleArtifactResizeReset,
      onResizeStart: handleArtifactResizeStart,
      onSelectArtifact: handleSelectArtifactWithChatFocus,
    },
  };
}
