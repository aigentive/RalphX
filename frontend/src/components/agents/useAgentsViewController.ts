import { useCallback, useEffect, useMemo, useState } from "react";
import { useQuery, useQueryClient, type InfiniteData } from "@tanstack/react-query";

import {
  chatApi,
  type AgentConversationRuntimeStatus,
  type AgentConversationWorkspace,
  type AgentConversationWorkspaceMode,
  type AgentWorkspaceReviewContext,
  type ComposerIntegrationReference,
  type ConversationListPageResponse,
} from "@/api/chat";
import { ideationApi } from "@/api/ideation";
import { chatKeys } from "@/hooks/useChat";
import { useAgentModels } from "@/hooks/useAgentModels";
import { useProjects } from "@/hooks/useProjects";
import { useEventBus } from "@/providers/EventProvider";
import type {
  AgentArtifactTab,
  AgentRuntimeSelection,
} from "@/stores/agentSessionStore";
import type { ChatConversation } from "@/types/chat-conversation";
import { PlanArtifactEventSchema } from "@/types/events";
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
import { useAgentUserMessageJiraInvalidation } from "./useAgentUserMessageJiraInvalidation";
import { hasJiraIntegrationReference } from "./agentJiraIssueQueries";
import { hasLinearIntegrationReference } from "./agentLinearIssueQueries";
import { hasGranolaIntegrationReference } from "./agentGranolaNoteQueries";
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
  invalidateWorkspaceQueries,
  preflightAgentWorkspaceFreshness,
  prReviewContextForConversation,
  resolveWorkspaceReviewOwnerConversationId,
  workspaceReviewContextForConversation,
} from "./agentWorkspaceQueries";
import {
  agentConversationIssueKeys,
  hasOpenAgentConversationIssues,
  useAgentConversationIssues,
} from "./agentConversationIssueQueries";
import type { IdeationArtifactTab } from "./agentArtifactTabs";
import {
  getAgentConversationStoreKey,
  toProjectAgentConversation,
  type AgentConversation,
} from "./agentConversations";
import { agentConversationKeys } from "./useProjectAgentConversations";
import type { AgentPublishFocusRequest } from "./agentPublishFocus";
import type { AgentTaskArtifactFocusRequest } from "./agentTaskArtifactFocus";
import type { DiffFilterMode } from "./AgentsPublishDiffFilter";
import {
  getAgentChatFocusSwitchOptions,
  getFocusedArtifactIdeationSessionId,
  latestVerificationChildSessionIdQueryKey,
  type AgentsChatFocus,
  type AgentsChatFocusType,
} from "./agentChatFocus";

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

type PrReviewArtifactEventPayload = {
  conversationId?: string;
  conversation_id?: string;
  artifact?: {
    id?: string;
  } | null;
};

type AgentConversationLifecyclePayload = {
  conversationId?: string;
  conversation_id?: string;
};

type WorkspaceReviewPublishPromotionState = Pick<
  AgentWorkspaceReviewContext,
  "monitor" | "isCurrent" | "isOutdated"
>;

function hasCurrentPassedWorkspaceReview(
  context: WorkspaceReviewPublishPromotionState | null,
): boolean {
  if (!context?.isCurrent || context.isOutdated) {
    return false;
  }
  const gateStatus = context.monitor.reviewGateStatus ?? null;
  if (gateStatus) {
    return gateStatus === "passed";
  }
  return context.monitor.reviewOutcome === "passed";
}

export function useAgentsViewController({
  projectId,
  onCreateProject,
}: UseAgentsViewControllerParams) {
  const queryClient = useQueryClient();
  const eventBus = useEventBus();
  const [chatFocus, setChatFocus] = useState<AgentsChatFocus>({ type: "workspace" });
  const [publishFocusRequest, setPublishFocusRequest] =
    useState<AgentPublishFocusRequest | null>(null);
  const [taskArtifactFocusRequest, setTaskArtifactFocusRequest] =
    useState<AgentTaskArtifactFocusRequest | null>(null);
  const [selectedTaskArtifactId, setSelectedTaskArtifactId] =
    useState<string | null>(null);
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
    setTaskArtifactFocusRequest(null);
    setSelectedTaskArtifactId(null);
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
  const handleFocusTaskRuntime = useCallback(
    (
      taskId: string,
      contextType: Extract<AgentsChatFocus, { type: "task_runtime" }>["contextType"],
    ) => {
      const nextFocus: Extract<AgentsChatFocus, { type: "task_runtime" }> = {
        type: "task_runtime",
        taskId,
        contextType,
      };
      setChatFocus((current) =>
        current.type === "task_runtime" &&
        current.taskId === taskId &&
        current.contextType === contextType
          ? current
          : nextFocus,
      );
    },
    [],
  );
  const handleFocusWorkspaceReview = useCallback((conversationId: string) => {
    setChatFocus((current) =>
      current.type === "workspace_review" &&
      current.conversationId === conversationId
        ? current
        : { type: "workspace_review", conversationId },
    );
  }, []);
  const handleTaskArtifactSelectionChange = useCallback(
    (taskId: string | null) => {
      setSelectedTaskArtifactId(taskId);
      if (taskId) {
        setChatFocus((current) =>
          current.type === "task_runtime" && current.taskId === taskId
            ? current
            : { type: "task_runtime", taskId, contextType: "task_execution" },
        );
      }
    },
    [],
  );
  const handleReturnToWorkspaceChat = useCallback(() => {
    setChatFocus((current) =>
      current.type === "workspace" ? current : { type: "workspace" },
    );
  }, []);
  const focusedWorkspaceReviewRuntimeConversationId =
    chatFocus.type === "workspace_review" ? chatFocus.conversationId : null;
  const activeRuntimeConversationId =
    focusedWorkspaceReviewRuntimeConversationId ?? selectedConversationId;
  const {
    activeConversationMode,
    activeConversationModeLocked,
    activeWorkspace,
    activeWorkspaceFreshness,
    normalizedActiveRuntime,
    publishShortcutLabel,
    terminalArchivedReason,
    terminalUnavailableReason,
  } = useAgentsWorkspaceModel({
    activeConversation,
    focusedWorkspaceReviewConversationId:
      focusedWorkspaceReviewRuntimeConversationId,
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

      void invalidateWorkspaceQueries(queryClient, conversationId);
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
  const activeProjectConversationId =
    activeConversation?.contextType === "project" ? activeConversation.id : null;
  const activeConversationIssuesQuery =
    useAgentConversationIssues(activeProjectConversationId);
  const hasActiveConversationIssues = hasOpenAgentConversationIssues(
    activeConversationIssuesQuery.data,
  );
  const prReviewConversationId =
    activeConversation?.contextType === "project" &&
    activeConversationMode === "review_pr" &&
    activeWorkspace?.mode === "review_pr"
      ? activeWorkspace.conversationId
      : null;
  const shouldLoadPrReviewContext = Boolean(prReviewConversationId);
  const prReviewContextQuery = useQuery({
    queryKey: agentWorkspaceKeys.prReview(prReviewConversationId ?? ""),
    queryFn: () => chatApi.getAgentWorkspacePrReviewContext(prReviewConversationId!),
    enabled: shouldLoadPrReviewContext,
    staleTime: 5_000,
  });
  const prReviewContext = prReviewContextForConversation(
    prReviewContextQuery.data,
    prReviewConversationId,
  );
  const workspaceReviewConversationId = resolveWorkspaceReviewOwnerConversationId({
    activeConversationContextType: activeConversation?.contextType,
    activeConversationId: activeConversation?.id,
    activeConversationParentId: activeConversation?.parentConversationId,
    activeConversationMode,
    activeWorkspaceConversationId: activeWorkspace?.conversationId,
  });
  const shouldLoadWorkspaceReviewContext = Boolean(workspaceReviewConversationId);
  const workspaceReviewContextQuery = useQuery({
    queryKey: agentWorkspaceKeys.workspaceReview(workspaceReviewConversationId ?? ""),
    queryFn: () =>
      chatApi.getAgentWorkspaceReviewContext(workspaceReviewConversationId!),
    enabled: shouldLoadWorkspaceReviewContext,
    staleTime: 5_000,
    refetchInterval: (query) =>
      query.state.data?.monitor.status === "reviewing" ? 2_000 : false,
  });
  const workspaceReviewContext = workspaceReviewContextForConversation(
    workspaceReviewContextQuery.data,
    workspaceReviewConversationId,
  );
  const isWorkspaceReviewRunning =
    workspaceReviewContext?.monitor.status === "reviewing" ||
    workspaceReviewContext?.monitor.reviewGateStatus === "reviewing";
  const promoteWorkspaceReviewPublishShortcut =
    hasCurrentPassedWorkspaceReview(workspaceReviewContext);
  const workspaceReviewArtifactId =
    workspaceReviewContext?.monitor.reviewArtifactId ?? null;
  const reviewArtifactId =
    workspaceReviewArtifactId ?? prReviewContext?.monitor?.reviewArtifactId ?? null;
  const shouldShowWorkspaceReviewTab = Boolean(
    workspaceReviewContext?.shouldShowTab || workspaceReviewArtifactId,
  );
  const availableArtifactTabsWithReview = useMemo<IdeationArtifactTab[]>(() => {
    const tabs =
      activeConversation?.contextType === "project" &&
      hasActiveConversationIssues &&
      !availableArtifactTabs.includes("issues")
        ? (["issues", ...availableArtifactTabs] as IdeationArtifactTab[])
        : availableArtifactTabs;
    if (
      (!reviewArtifactId && !shouldShowWorkspaceReviewTab) ||
      tabs.includes("review")
    ) {
      return tabs;
    }
    return [...tabs, "review"];
  }, [
    activeConversation?.contextType,
    availableArtifactTabs,
    hasActiveConversationIssues,
    reviewArtifactId,
    shouldShowWorkspaceReviewTab,
  ]);
  const hasAutoOpenArtifactsWithReview =
    hasAutoOpenArtifacts || Boolean(reviewArtifactId) || shouldShowWorkspaceReviewTab;
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
  const taskRuntimeFocusTarget =
    chatFocus.type === "task_runtime" ? chatFocus : null;
  const workspaceReviewChildConversationId =
    workspaceReviewContext?.monitor.reviewConversationId ?? null;
  const workspaceReviewFocusTarget = useMemo(
    () =>
      workspaceReviewChildConversationId
        ? ({
            type: "workspace_review",
            conversationId: workspaceReviewChildConversationId,
          } satisfies Extract<AgentsChatFocus, { type: "workspace_review" }>)
        : null,
    [workspaceReviewChildConversationId],
  );
  const workspaceReviewRuntimeStatus = useMemo<AgentConversationRuntimeStatus | null>(() => {
    if (
      !workspaceReviewConversationId ||
      workspaceReviewContext?.monitor.status !== "reviewing" ||
      !workspaceReviewChildConversationId
    ) {
      return null;
    }

    return {
      conversationId: workspaceReviewConversationId,
      isRunning: true,
      agentStatus: "generating",
      primarySource: "workspace_review",
      summaryLabel: "Reviewing",
      items: [
        {
          source: "workspace_review",
          contextType: "project",
          contextId: workspaceReviewChildConversationId,
          label: "Reviewing",
          title: "Review workspace changes",
          agentStatus: "generating",
          taskId: null,
          internalStatus: "reviewing",
          runningProcess: null,
          ideationSession: null,
          parentSessionId: null,
          childSessionId: null,
          conversationId: workspaceReviewChildConversationId,
        },
      ],
    };
  }, [
    workspaceReviewChildConversationId,
    workspaceReviewContext?.monitor.status,
    workspaceReviewConversationId,
  ]);
  useEffect(() => {
    if (
      workspaceReviewContext?.monitor.status !== "reviewing" ||
      !workspaceReviewChildConversationId
    ) {
      return;
    }
    setChatFocus((current) =>
      current.type === "workspace_review" &&
      current.conversationId === workspaceReviewChildConversationId
        ? current
        : {
            type: "workspace_review",
            conversationId: workspaceReviewChildConversationId,
          },
    );
  }, [workspaceReviewChildConversationId, workspaceReviewContext?.monitor.status]);
  useEffect(() => {
    const fixerStatus = workspaceReviewContext?.monitor.reviewFixerStatus ?? null;
    if (fixerStatus !== "queued" && fixerStatus !== "running") {
      return;
    }
    setChatFocus((current) =>
      current.type === "workspace_review" ? { type: "workspace" } : current,
    );
  }, [
    workspaceReviewContext?.monitor.reviewFixerRunId,
    workspaceReviewContext?.monitor.reviewFixerStatus,
  ]);
  const hasAttachedPlanArtifact = availableArtifactTabs.includes("plan");
  const chatFocusOptions = useMemo(() => {
    return getAgentChatFocusSwitchOptions({
      mode: activeConversationMode,
      focusSwitcherIdeationSessionId,
      verificationFocusTarget,
      taskRuntimeFocusTarget,
      workspaceReviewFocusTarget,
      hasPlanArtifact: hasAttachedPlanArtifact,
    });
  }, [
    activeConversationMode,
    focusSwitcherIdeationSessionId,
    hasAttachedPlanArtifact,
    taskRuntimeFocusTarget,
    verificationFocusTarget,
    workspaceReviewFocusTarget,
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

      if (type === "verification" && verificationFocusTarget) {
        setChatFocus(verificationFocusTarget);
        return;
      }

      if (type === "task_runtime" && taskRuntimeFocusTarget) {
        setChatFocus(taskRuntimeFocusTarget);
        return;
      }

      if (type === "workspace_review" && workspaceReviewFocusTarget) {
        setChatFocus(workspaceReviewFocusTarget);
      }
    },
    [
      chatFocusOptions,
      focusSwitcherIdeationSessionId,
      handleFocusIdeationSession,
      handleReturnToWorkspaceChat,
      taskRuntimeFocusTarget,
      verificationFocusTarget,
      workspaceReviewFocusTarget,
    ],
  );
  const {
    openArtifactTab,
    scheduleArtifactPanePreload,
    setArtifactPaneVisibility,
    setArtifactTaskMode,
    toggleArtifactPaneVisibility,
  } = useAgentArtifactController({
    hasAutoOpenArtifacts: hasAutoOpenArtifactsWithReview,
    selectedConversationId,
  });
  const handleOpenPlanArtifact = useCallback(() => {
    if (!selectedConversationId) {
      return;
    }
    openArtifactTab(selectedConversationId, "plan");
  }, [openArtifactTab, selectedConversationId]);
  const handleOpenTaskArtifact = useCallback(
    (taskId: string) => {
      if (!selectedConversationId) {
        return;
      }
      setSelectedTaskArtifactId(taskId);
      setTaskArtifactFocusRequest((current) => ({
        taskId,
        requestId: (current?.requestId ?? 0) + 1,
      }));
      openArtifactTab(selectedConversationId, "tasks");
    },
    [openArtifactTab, selectedConversationId],
  );

  const { clearAutoManagedTitle, handleAutoManagedTitle } = useAgentsAutoTitle({
    findConversationById,
    invalidateProjectConversations,
  });
  const openJiraTabForConversation = useCallback(
    (conversationId: string) => {
      openArtifactTab(conversationId, "jira");
    },
    [openArtifactTab],
  );
  const openLinearTabForConversation = useCallback(
    (conversationId: string) => {
      openArtifactTab(conversationId, "linear");
    },
    [openArtifactTab],
  );
  const openGranolaTabForConversation = useCallback(
    (conversationId: string) => {
      openArtifactTab(conversationId, "granola");
    },
    [openArtifactTab],
  );
  useEffect(() => {
    const invalidateReviewArtifact = (payload: PrReviewArtifactEventPayload) => {
      const conversationId = payload.conversationId ?? payload.conversation_id;
      if (!conversationId) {
        return;
      }
      void queryClient.invalidateQueries({
        queryKey: agentWorkspaceKeys.prReview(conversationId),
      });
      void queryClient.invalidateQueries({
        queryKey: agentWorkspaceKeys.workspaceReview(conversationId),
      });
      void queryClient.invalidateQueries({
        queryKey: agentWorkspaceKeys.workspaceReviewHunkAnnotations(conversationId),
      });
      const artifactId = payload.artifact?.id;
      if (artifactId) {
        void queryClient.invalidateQueries({
          queryKey: ["agents", "artifact", artifactId],
        });
      }
    };

    const unsubscribeCreated = eventBus.subscribe<PrReviewArtifactEventPayload>(
      "pr_review_artifact:created",
      (payload) => {
        invalidateReviewArtifact(payload);
        const conversationId = payload.conversationId ?? payload.conversation_id;
        if (conversationId && conversationId === selectedConversationId) {
          openArtifactTab(conversationId, "review");
        }
      },
    );
    const unsubscribeUpdated = eventBus.subscribe<PrReviewArtifactEventPayload>(
      "pr_review_artifact:updated",
      invalidateReviewArtifact,
    );
    const unsubscribeWorkspaceCreated =
      eventBus.subscribe<PrReviewArtifactEventPayload>(
        "workspace_review_artifact:created",
        (payload) => {
          invalidateReviewArtifact(payload);
          const conversationId = payload.conversationId ?? payload.conversation_id;
          if (conversationId && conversationId === selectedConversationId) {
            openArtifactTab(conversationId, "review");
          }
        },
      );
    const unsubscribeWorkspaceUpdated =
      eventBus.subscribe<PrReviewArtifactEventPayload>(
        "workspace_review_artifact:updated",
        invalidateReviewArtifact,
      );

    return () => {
      unsubscribeCreated();
      unsubscribeUpdated();
      unsubscribeWorkspaceCreated();
      unsubscribeWorkspaceUpdated();
    };
  }, [eventBus, openArtifactTab, queryClient, selectedConversationId]);

  useEffect(() => {
    const invalidateConversationIssues = (
      payload: AgentConversationLifecyclePayload,
    ) => {
      const conversationId = payload.conversationId ?? payload.conversation_id;
      if (conversationId) {
        void queryClient.invalidateQueries({
          queryKey: agentConversationIssueKeys.list(conversationId),
        });
      }
      if (selectedConversationId && activeConversation?.contextType === "project") {
        void queryClient.invalidateQueries({
          queryKey: agentWorkspaceKeys.workspaceReview(selectedConversationId),
        });
      }
    };

    const unsubscribeRunCompleted =
      eventBus.subscribe<AgentConversationLifecyclePayload>(
        "agent:run_completed",
        invalidateConversationIssues,
      );
    const unsubscribeRunStarted =
      eventBus.subscribe<AgentConversationLifecyclePayload>(
        "agent:run_started",
        invalidateConversationIssues,
      );
    const unsubscribeTurnCompleted =
      eventBus.subscribe<AgentConversationLifecyclePayload>(
        "agent:turn_completed",
        invalidateConversationIssues,
      );

    return () => {
      unsubscribeRunStarted();
      unsubscribeRunCompleted();
      unsubscribeTurnCompleted();
    };
  }, [activeConversation?.contextType, eventBus, queryClient, selectedConversationId]);

  useEffect(() => {
    const unsubscribeCreated = eventBus.subscribe<unknown>(
      "plan_artifact:created",
      (payload) => {
        const parsed = PlanArtifactEventSchema.safeParse({
          type: "created",
          ...(payload as Record<string, unknown>),
        });
        if (!parsed.success || parsed.data.type !== "created") {
          return;
        }
        if (
          !selectedConversationId ||
          activeConversationMode !== "plan" ||
          parsed.data.sessionId !== attachedIdeationSessionId
        ) {
          return;
        }

        openArtifactTab(selectedConversationId, "plan");
      },
    );

    return unsubscribeCreated;
  }, [
    activeConversationMode,
    attachedIdeationSessionId,
    eventBus,
    openArtifactTab,
    selectedConversationId,
  ]);

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
    onJiraLinked: openJiraTabForConversation,
    onLinearLinked: openLinearTabForConversation,
    onGranolaLinked: openGranolaTabForConversation,
  });

  const {
    handleArchiveConversation,
    handleArchiveProject,
    handleAutoRenameConversation,
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
    hasAutoOpenArtifacts: hasAutoOpenArtifactsWithReview,
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
      if (!isWorkspaceReviewRunning) {
        handleReturnToWorkspaceChat();
      }
      setPublishFocusRequest((current) => ({
        conversationId: selectedConversationId,
        filePath,
        mode,
        requestId: (current?.requestId ?? 0) + 1,
      }));
      openArtifactTab(selectedConversationId, "publish");
    },
    [
      handleReturnToWorkspaceChat,
      isWorkspaceReviewRunning,
      openArtifactTab,
      selectedConversationId,
    ],
  );
  const handleOpenPublishPaneWithChatFocus = useCallback(() => {
    if (!isWorkspaceReviewRunning) {
      handleReturnToWorkspaceChat();
    }
    handleOpenPublishPane();
  }, [
    handleOpenPublishPane,
    handleReturnToWorkspaceChat,
    isWorkspaceReviewRunning,
  ]);
  const handleSelectArtifactWithChatFocus = useCallback(
    (tab: AgentArtifactTab) => {
      if (tab !== "review" && !isWorkspaceReviewRunning) {
        handleReturnToWorkspaceChat();
      }
      handleSelectArtifact(tab);
    },
    [handleReturnToWorkspaceChat, handleSelectArtifact, isWorkspaceReviewRunning],
  );

  const handleAgentUserMessageAutoTitle = useAgentUserMessageAutoTitle({
    activeProjectId,
    findConversationById,
    handleAutoManagedTitle,
    selectedConversationId,
  });
  const invalidateAgentUserMessageJira = useAgentUserMessageJiraInvalidation({
    queryClient,
    selectedConversationId,
  });
  const handleAgentUserMessageSent = useCallback(
    (event: {
      content: string;
      result: { conversationId: string };
      composerIntegrationReferences?: ComposerIntegrationReference[];
    }) => {
      handleAgentUserMessageAutoTitle(event);
      invalidateAgentUserMessageJira(event);
      if (hasJiraIntegrationReference(event.composerIntegrationReferences)) {
        openJiraTabForConversation(event.result.conversationId);
      } else if (hasLinearIntegrationReference(event.composerIntegrationReferences)) {
        openLinearTabForConversation(event.result.conversationId);
      } else if (hasGranolaIntegrationReference(event.composerIntegrationReferences)) {
        openGranolaTabForConversation(event.result.conversationId);
      }
    },
    [
      handleAgentUserMessageAutoTitle,
      invalidateAgentUserMessageJira,
      openJiraTabForConversation,
      openGranolaTabForConversation,
      openLinearTabForConversation,
    ],
  );
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
    handleActiveProviderChange,
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
    runtimeConversationId: activeRuntimeConversationId,
    runtimeDefaultPolicy: focusedWorkspaceReviewRuntimeConversationId
      ? "workspace_review_utility"
      : "provider_default",
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
    onAutoRenameConversation: handleAutoRenameConversation,
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
      availableArtifactTabs: availableArtifactTabsWithReview,
      chatFocus,
      chatFocusOptions,
      defaultProjectId,
      defaultRuntime,
      hasAutoOpenArtifacts: hasAutoOpenArtifactsWithReview,
      isLoadingProjects,
      modelRegistry,
      normalizedActiveRuntime,
      workspaceReviewRuntimeStatus,
      onActiveConversationModeChange: handleActiveConversationModeChange,
      onActiveConversationModeMenuOpen: handleActiveConversationModeMenuOpen,
      onActiveEffortChange: handleActiveEffortChange,
      onActiveModelChange: handleActiveModelChange,
      onActiveProviderChange: handleActiveProviderChange,
      onAgentUserMessageSent: handleAgentUserMessageSent,
      onConversationModeSwitched: handleConversationModeSwitched,
      onCreateProject,
      onFocusIdeationSession: handleFocusIdeationSession,
      onFocusWorkspaceReview: handleFocusWorkspaceReview,
      onFocusVerificationSession: handleFocusVerificationSession,
      onFocusTaskRuntime: handleFocusTaskRuntime,
      onOpenTaskArtifact: handleOpenTaskArtifact,
      onForkConversation: handleForkConversation,
      onOpenPlanArtifact: handleOpenPlanArtifact,
      onOpenPublishPane: handleOpenPublishPaneWithChatFocus,
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
      promotePublishShortcut: promoteWorkspaceReviewPublishShortcut,
      publishingConversationId,
      selectedConversationId,
      selectedTaskArtifactId,
      setTerminalChatDockElement,
      switchingConversationModeId,
      terminalArchivedReason,
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
      activeWorkspaceFreshness,
      artifactWidthCss,
      chatDockElement: terminalChatDockElement,
      focusedIdeationSessionId: focusedArtifactIdeationSessionId,
      hasAutoOpenArtifacts: hasAutoOpenArtifactsWithReview,
      isArtifactResizing,
      openArtifactTab,
      panelDockElement: terminalPanelDockElement,
      publishFocusRequest,
      publishingConversationId,
      selectedConversationId,
      setArtifactPaneVisibility,
      setArtifactTaskMode,
      setTerminalPanelDockElement,
      taskArtifactFocusRequest,
      terminalArchivedReason,
      terminalUnavailableReason,
      onFocusVerificationSession: handleFocusVerificationSession,
      onFocusWorkspaceReview: handleFocusWorkspaceReview,
      onOpenPublish: handleOpenPublishPaneWithChatFocus,
      onTaskArtifactSelectionChange: handleTaskArtifactSelectionChange,
      onPublishWorkspace: handlePublishWorkspace,
      onResizeReset: handleArtifactResizeReset,
      onResizeStart: handleArtifactResizeStart,
      onSelectArtifact: handleSelectArtifactWithChatFocus,
    },
  };
}
