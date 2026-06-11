import { useCallback, useEffect, useMemo, useState } from "react";
import { useQuery, useQueryClient, type InfiniteData } from "@tanstack/react-query";

import {
  chatApi,
  type AgentConversationWorkspace,
  type AgentConversationWorkspaceMode,
  type ConversationListPageResponse,
} from "@/api/chat";
import { ideationApi } from "@/api/ideation";
import { chatKeys } from "@/hooks/useChat";
import { useAgentModels } from "@/hooks/useAgentModels";
import { useProjects } from "@/hooks/useProjects";
import { useEventBus } from "@/providers/EventProvider";
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
import { runtimeFromConversation } from "./agentConversationRuntime";
import {
  agentWorkspaceKeys,
  preflightAgentWorkspaceFreshness,
} from "./agentWorkspaceQueries";
import {
  getAgentConversationStoreKey,
  toProjectAgentConversation,
  type AgentConversation,
} from "./agentConversations";
import { agentConversationKeys } from "./useProjectAgentConversations";
import type { AgentPublishFocusRequest } from "./agentPublishFocus";
import type { DiffFilterMode } from "./AgentsPublishDiffFilter";
import {
  getAgentChatFocusSwitchOptions,
  getFocusedArtifactIdeationSessionId,
  latestVerificationChildSessionIdQueryKey,
  type AgentsChatFocus,
  type AgentsChatFocusType,
} from "./agentChatFocus";
import type { AgentRuntimeSelection } from "@/stores/agentSessionStore";
import type { ChatConversation } from "@/types/chat-conversation";

interface UseAgentsViewControllerParams {
  projectId: string;
  onCreateProject: () => void;
}

type AgentConversationListPage = Omit<
  ConversationListPageResponse,
  "conversations"
> & {
  conversations: AgentConversation[];
};

export function useAgentsViewController({
  projectId,
  onCreateProject,
}: UseAgentsViewControllerParams) {
  const queryClient = useQueryClient();
  const eventBus = useEventBus();
  const [chatFocus, setChatFocus] = useState<AgentsChatFocus>({ type: "workspace" });
  const [publishFocusRequest, setPublishFocusRequest] =
    useState<AgentPublishFocusRequest | null>(null);
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
    setPublishFocusRequest(null);
  }, [selectedConversationId]);
  useEffect(() => {
    if (!selectedConversationId || activeConversation?.contextType !== "project") {
      return;
    }
    void preflightAgentWorkspaceFreshness(queryClient, selectedConversationId);
  }, [activeConversation?.contextType, queryClient, selectedConversationId]);
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
    activeWorkspaceFreshness,
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
  const handleConversationModeSwitched = useCallback(
    (
      conversationId: string,
      mode: AgentConversationWorkspaceMode,
      workspace: AgentConversationWorkspace | null,
    ) => {
      const projectIdForConversation =
        activeConversation?.id === conversationId
          ? activeConversation.projectId
          : activeProjectId;
      const patchConversation = <T extends ChatConversation | AgentConversation>(
        conversation: T,
      ): T =>
        conversation.agentMode === mode
          ? conversation
          : { ...conversation, agentMode: mode };

      queryClient.setQueryData<ChatConversation | null | undefined>(
        chatKeys.conversationSummary(conversationId),
        (current) => (current ? patchConversation(current) : current),
      );
      setOptimisticConversationsById((current) => {
        const existing =
          current[conversationId] ??
          (activeConversation?.id === conversationId ? activeConversation : null);
        if (!existing) {
          return current;
        }
        const patched = patchConversation(existing);
        return patched === existing
          ? current
          : { ...current, [conversationId]: patched };
      });

      if (projectIdForConversation) {
        queryClient.setQueriesData<InfiniteData<AgentConversationListPage>>(
          {
            predicate: (query) => {
              const queryKey = query.queryKey;
              return (
                queryKey[0] === agentConversationKeys.all[0] &&
                queryKey[1] === agentConversationKeys.all[1] &&
                queryKey[2] === projectIdForConversation &&
                queryKey[3] === "archived"
              );
            },
          },
          (current) => {
            if (!current || !Array.isArray(current.pages)) {
              return current;
            }
            let changed = false;
            const pages = current.pages.map((page) => {
              let pageChanged = false;
              const conversations = page.conversations.map((conversation) => {
                if (conversation.id !== conversationId) {
                  return conversation;
                }
                const patched = patchConversation(conversation);
                pageChanged ||= patched !== conversation;
                return patched;
              });
              changed ||= pageChanged;
              return pageChanged ? { ...page, conversations } : page;
            });
            return changed ? { ...current, pages } : current;
          },
        );
      }

      if (workspace) {
        queryClient.setQueryData(
          agentWorkspaceKeys.workspace(conversationId),
          workspace,
        );
        setOptimisticWorkspacesByConversationId((current) =>
          current[conversationId] === workspace
            ? current
            : { ...current, [conversationId]: workspace },
        );
      }

      void queryClient.invalidateQueries({
        queryKey: chatKeys.conversationSummary(conversationId),
      });
      if (projectIdForConversation) {
        void invalidateProjectConversations(projectIdForConversation);
      }
    },
    [
      activeConversation,
      activeProjectId,
      invalidateProjectConversations,
      queryClient,
      setOptimisticConversationsById,
      setOptimisticWorkspacesByConversationId,
    ],
  );
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
    queryKey: latestVerificationChildSessionIdQueryKey(
      knownFocusIdeationSessionId,
    ),
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
  const hasAttachedPlanArtifact = availableArtifactTabs.includes("plan");
  const chatFocusOptions = useMemo(() => {
    return getAgentChatFocusSwitchOptions({
      mode: activeConversationMode,
      focusSwitcherIdeationSessionId,
      verificationFocusTarget,
      hasPlanArtifact: hasAttachedPlanArtifact,
    });
  }, [
    activeConversationMode,
    focusSwitcherIdeationSessionId,
    hasAttachedPlanArtifact,
    verificationFocusTarget,
  ]);
  useEffect(() => {
    if (chatFocusOptions.some((option) => option.type === chatFocus.type)) {
      return;
    }
    setChatFocus({ type: "workspace" });
  }, [chatFocus.type, chatFocusOptions]);
  const handleSelectChatFocus = useCallback(
    (type: AgentsChatFocusType) => {
      if (!chatFocusOptions.some((option) => option.type === type)) {
        return;
      }

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
      chatFocusOptions,
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
    handleForkConversation,
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
    setOptimisticConversationsById,
    setOptimisticSelectedConversationId,
    setOptimisticWorkspacesByConversationId,
    setRuntimeForConversation,
  });
  const handleSidebarForkConversation = useCallback(
    async (conversation: AgentConversation) => {
      await handleForkConversation(conversation.id);
    },
    [handleForkConversation],
  );

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
  useEffect(() => {
    return eventBus.subscribe<{
      parent_conversation_id: string;
      conversation_id: string;
      context_type: string;
      context_id: string;
    }>("agent:conversation_forked", (payload) => {
      if (
        payload.context_type !== "project" ||
        payload.parent_conversation_id !== selectedConversationId
      ) {
        return;
      }

      void chatApi
        .getConversationSummary(payload.conversation_id)
        .then((conversation) => {
          if (!conversation || conversation.contextType !== "project") {
            return;
          }
          const agentConversation = toProjectAgentConversation(conversation);
          const forkRuntime = runtimeFromConversation(agentConversation);
          queryClient.setQueryData(
            chatKeys.conversationSummary(conversation.id),
            conversation,
          );
          setOptimisticConversationsById((current) => ({
            ...current,
            [agentConversation.id]: agentConversation,
          }));
          setOptimisticSelectedConversationId(agentConversation.id);
          setFocusedProject(agentConversation.projectId);
          if (forkRuntime) {
            setRuntimeForConversation(
              agentConversation.id,
              agentConversation.projectId,
              forkRuntime,
            );
          }
          selectConversation(agentConversation.projectId, agentConversation.id);
          setActiveConversation(
            getAgentConversationStoreKey(agentConversation),
            agentConversation.id,
          );
          void invalidateProjectConversations(agentConversation.projectId);
        })
        .catch(() => {
          // Manual /fork already handles errors. This listener only keeps
          // terminal continuity sends aligned when the backend auto-forks.
        });
    });
  }, [
    eventBus,
    invalidateProjectConversations,
    queryClient,
    selectConversation,
    selectedConversationId,
    setActiveConversation,
    setFocusedProject,
    setOptimisticConversationsById,
    setOptimisticSelectedConversationId,
    setRuntimeForConversation,
  ]);
  const handleOpenPublishFile = useCallback(
    (filePath: string, mode: DiffFilterMode) => {
      if (!selectedConversationId) {
        return;
      }
      setPublishFocusRequest((current) => ({
        conversationId: selectedConversationId,
        filePath,
        mode,
        requestId: (current?.requestId ?? 0) + 1,
      }));
      openArtifactTab(selectedConversationId, "publish");
    },
    [openArtifactTab, selectedConversationId],
  );
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
    handleActiveConversationModeMenuOpen,
    handleActiveEffortChange,
    handleActiveModelChange,
    switchingConversationModeId,
  } = useAgentsActiveComposerControls({
    activeConversation,
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
    onForkConversation: handleSidebarForkConversation,
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
      activeWorkspaceFreshness,
      attachedIdeationSessionId,
      availableArtifactTabs,
      chatFocus,
      chatFocusOptions,
      defaultProjectId,
      defaultRuntime,
      hasAutoOpenArtifacts,
      isLoadingProjects,
      modelRegistry,
      normalizedActiveRuntime,
      onActiveConversationModeChange: handleActiveConversationModeChange,
      onActiveConversationModeMenuOpen: handleActiveConversationModeMenuOpen,
      onActiveEffortChange: handleActiveEffortChange,
      onActiveModelChange: handleActiveModelChange,
      onAgentUserMessageSent: handleAgentUserMessageSent,
      onConversationModeSwitched: handleConversationModeSwitched,
      onCreateProject,
      onFocusIdeationSession: handleFocusIdeationSession,
      onForkConversation: handleForkConversation,
      onOpenPublishPane: handleOpenPublishPane,
      onOpenPublishFile: handleOpenPublishFile,
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
      activeProjectBaseBranch,
      activeWorkspace,
      artifactWidthCss,
      chatDockElement: terminalChatDockElement,
      focusedIdeationSessionId: focusedArtifactIdeationSessionId,
      hasAutoOpenArtifacts,
      isArtifactResizing,
      openArtifactTab,
      panelDockElement: terminalPanelDockElement,
      publishFocusRequest,
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
