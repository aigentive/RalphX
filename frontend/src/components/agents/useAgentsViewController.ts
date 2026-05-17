import { useQueryClient } from "@tanstack/react-query";

import { useProjects } from "@/hooks/useProjects";
import { useAgentArtifactController } from "./useAgentArtifactController";
import { useAgentConversationTitleEvents } from "./useAgentConversationTitleEvents";
import { useProjectAgentBridgeEvents } from "./useProjectAgentBridgeEvents";
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

interface UseAgentsViewControllerParams {
  projectId: string;
  onCreateProject: () => void;
}

export function useAgentsViewController({
  projectId,
  onCreateProject,
}: UseAgentsViewControllerParams) {
  const queryClient = useQueryClient();
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
  const {
    clearAgentConversationSelection,
    focusedProjectId,
    lastRuntimeByProjectId,
    runtimeByConversationId,
    selectConversation,
    selectedProjectId,
    setActiveConversation,
    setFocusedProject,
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
  const {
    activeConversationMode,
    activeConversationModeLocked,
    activeWorkspace,
    normalizedActiveRuntime,
    publishShortcutLabel,
    terminalUnavailableReason,
  } = useAgentsWorkspaceModel({
    activeConversation,
    optimisticWorkspacesByConversationId,
    runtimeByConversationId,
    selectedConversationId,
  });
  const activeProjectBaseBranch = useMemo(
    () => projects.find((project) => project.id === activeProjectId)?.baseBranch ?? null,
    [activeProjectId, projects],
  );
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
  useProjectAgentBridgeEvents({
    conversation: activeConversation,
    attachedIdeationSessionId,
    projectId: activeProjectId,
  });
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

  const handleAgentUserMessageSent = useAgentUserMessageAutoTitle({
    activeProjectId,
    findConversationById,
    handleAutoManagedTitle,
    selectedConversationId,
  });

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
    handleActiveModelChange,
    switchingConversationModeId,
  } = useAgentsActiveComposerControls({
    activeConversation,
    activeProjectId,
    activeWorkspace,
    defaultProjectId,
    invalidateProjectConversations,
    lastRuntimeByProjectId,
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
      defaultProjectId,
      defaultRuntime,
      hasAutoOpenArtifacts,
      isLoadingProjects,
      normalizedActiveRuntime,
      onActiveConversationModeChange: handleActiveConversationModeChange,
      onActiveModelChange: handleActiveModelChange,
      onAgentUserMessageSent: handleAgentUserMessageSent,
      onCreateProject,
      onOpenPublishPane: handleOpenPublishPane,
      onPreloadArtifacts: handlePreloadArtifacts,
      onPublishWorkspace: handlePublishWorkspace,
      onRenameConversation: handleRenameConversation,
      onSelectArtifact: handleSelectArtifact,
      onStartAgentConversation: handleStartAgentConversation,
      onToggleArtifacts: toggleArtifactPaneVisibility,
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
      activeProjectBaseBranch,
      activeWorkspace,
      artifactWidthCss,
      chatDockElement: terminalChatDockElement,
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
      onPublishWorkspace: handlePublishWorkspace,
      onResizeReset: handleArtifactResizeReset,
      onResizeStart: handleArtifactResizeStart,
      onSelectArtifact: handleSelectArtifact,
    },
  };
}
