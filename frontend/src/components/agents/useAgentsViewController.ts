import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useQuery, useQueryClient, type InfiniteData } from "@tanstack/react-query";

import {
  chatApi,
  type AgentConversationWorkspace,
  type AgentConversationWorkspaceMode,
  type AgentWorkspaceReviewContext,
  type ComposerIntegrationReference,
  type ConversationListPageResponse,
} from "@/api/chat";
import { ideationApi } from "@/api/ideation";
import { requestAutomationRunOpen } from "@/components/automations/automationRunNavigation";
import { getAutomationConversationTabPolicy } from "@/components/automations/automationConversationTabPolicy";
import { chatKeys, useConversationSummary } from "@/hooks/useChat";
import { useAgentModels } from "@/hooks/useAgentModels";
import { useFeatureFlags } from "@/hooks/useFeatureFlags";
import { useManualRoleDefaults } from "@/hooks/useManualRoleDefaults";
import { useProjects } from "@/hooks/useProjects";
import { useEventBus } from "@/providers/EventProvider";
import {
  useAgentSessionStore,
  type AgentArtifactTab,
  type AgentAutomationRunFocusRequest,
  type AgentRuntimeSelection,
  type AgentTaskArtifactFocusRequest,
} from "@/stores/agentSessionStore";
import type { ChatConversation } from "@/types/chat-conversation";
import { PlanArtifactEventSchema } from "@/types/events";
import { useAgentArtifactController } from "./useAgentArtifactController";
import { seedAgentArtifactTab } from "./agentArtifactState";
import { useAgentArtifactUiStore } from "./agentArtifactUiStore";
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
import { hasClickUpIntegrationReference } from "./agentClickUpTicketQueries";
import { hasGranolaIntegrationReference } from "./agentGranolaNoteQueries";
import { useAgentsSessionBindings } from "./useAgentsSessionBindings";
import { useSyncedAgentProjectFocus } from "./useSyncedAgentProjectFocus";
import { useAgentsOptimisticState } from "./useAgentsOptimisticState";
import { useAgentsTerminalDocks } from "./useAgentsTerminalDocks";
import { useAgentsSidebarState } from "./useAgentsSidebarState";
import { useAgentsSidebarProps } from "./useAgentsSidebarProps";
import { normalizeRuntimeForPersistence } from "./agentOptions";
import {
  runtimeFromConversation,
  runtimeFromManualRoleDefault,
} from "./agentConversationRuntime";
import {
  agentWorkspaceKeys,
  invalidateWorkspaceQueries,
  preflightAgentWorkspaceFreshness,
  prReviewContextForConversation,
  refreshWorkspaceReviewContext,
  refreshWorkspaceReviewAfterSignal,
  resolveWorkspaceReviewOwnerConversationId,
  workspaceReviewContextForConversation,
} from "./agentWorkspaceQueries";
import {
  agentConversationIssueKeys,
  hasOpenAgentConversationIssues,
  useAgentConversationIssues,
} from "./agentConversationIssueQueries";
import {
  getAgentConversationStoreKey,
  toProjectAgentConversation,
  type AgentConversation,
} from "./agentConversations";
import { agentConversationKeys } from "./useProjectAgentConversations";
import type { AgentPublishFocusRequest } from "./agentPublishFocus";
import type { AgentPublishSubTab } from "./agentPublishSubTab";
import type { DiffFilterMode } from "./AgentsPublishDiffFilter";
import {
  focusWorkspaceReview,
  getAgentChatFocusSwitchOptions,
  getConversationScopedChatFocus,
  getFocusedArtifactIdeationSession,
  latestVerificationChildSessionIdQueryKey,
  type AgentsChatFocus,
  type AgentsChatFocusType,
  type AutomationRunFocusOptions,
} from "./agentChatFocus";

interface UseAgentsViewControllerParams {
  projectId: string;
  onCreateProject: () => void;
  onOpenAutomation?: (automationId: string) => void;
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
  parentConversationId?: string;
  parent_conversation_id?: string;
  childConversationId?: string;
  child_conversation_id?: string;
  contextId?: string;
  context_id?: string;
};

type WorkspaceReviewPublishPromotionState = Pick<
  AgentWorkspaceReviewContext,
  "monitor" | "reviewArtifactIsCurrent" | "reviewArtifactIsOutdated"
>;

export function getWorkspaceRepairFocusTarget({
  reviewFixerConversationId,
  repairRuntimeConversationId,
  repairFixerKind,
}: {
  reviewFixerConversationId: string | null;
  repairRuntimeConversationId: string | null;
  repairFixerKind: "workspace_repair" | "pr_fixer" | null;
}): Extract<AgentsChatFocus, { type: "workspace_repair" }> | null {
  // Backend guards make a Review fixer and an ordinary repair attempt mutually exclusive.
  const conversationId =
    reviewFixerConversationId ??
    (repairFixerKind === "workspace_repair"
      ? repairRuntimeConversationId
      : null);
  return conversationId ? { type: "workspace_repair", conversationId } : null;
}

function hasCurrentPassedWorkspaceReview(
  context: WorkspaceReviewPublishPromotionState | null,
): boolean {
  if (
    !context?.reviewArtifactIsCurrent ||
    context.reviewArtifactIsOutdated
  ) {
    return false;
  }
  const gateStatus = context.monitor.reviewGateStatus ?? null;
  if (gateStatus) {
    return gateStatus === "passed";
  }
  return context.monitor.reviewOutcome === "passed";
}

function addLifecyclePayloadId(ids: Set<string>, value: string | undefined) {
  const trimmed = value?.trim();
  if (trimmed) {
    ids.add(trimmed);
  }
}

function lifecyclePayloadIds(payload: AgentConversationLifecyclePayload): Set<string> {
  const ids = new Set<string>();
  addLifecyclePayloadId(ids, payload.conversationId);
  addLifecyclePayloadId(ids, payload.conversation_id);
  addLifecyclePayloadId(ids, payload.parentConversationId);
  addLifecyclePayloadId(ids, payload.parent_conversation_id);
  addLifecyclePayloadId(ids, payload.childConversationId);
  addLifecyclePayloadId(ids, payload.child_conversation_id);
  addLifecyclePayloadId(ids, payload.contextId);
  addLifecyclePayloadId(ids, payload.context_id);
  return ids;
}

function lifecyclePayloadOwnsWorkspaceReviewQuery(
  payload: AgentConversationLifecyclePayload,
  selectedConversationId: string,
  workspaceReviewChildConversationId: string | null | undefined,
): boolean {
  const ids = lifecyclePayloadIds(payload);
  return (
    ids.has(selectedConversationId) ||
    (workspaceReviewChildConversationId
      ? ids.has(workspaceReviewChildConversationId)
      : false)
  );
}

function defaultAutomationRunFocusTab(
  request: AgentAutomationRunFocusRequest,
): AgentArtifactTab {
  return getAutomationConversationTabPolicy({
    surface: "run",
    runStatus: request.runStatus,
    judgeState: request.judgeState,
    workspaceMode: request.workspaceMode,
    availability: {
      hasPlanArtifact: request.hasPlanArtifact,
      hasPullRequest: request.hasPullRequest,
    },
  }).defaultTab as AgentArtifactTab;
}

export function useAgentsViewController({
  projectId,
  onCreateProject,
  onOpenAutomation,
}: UseAgentsViewControllerParams) {
  const { data: featureFlags } = useFeatureFlags();
  const queryClient = useQueryClient();
  const eventBus = useEventBus();
  const [chatFocus, setChatFocus] = useState<AgentsChatFocus>({ type: "workspace" });
  const setVisibleAgentScope = useAgentSessionStore(
    (state) => state.setVisibleAgentScope,
  );
  const setStartConversationDraft = useAgentSessionStore(
    (state) => state.setStartConversationDraft,
  );
  const [publishFocusRequest, setPublishFocusRequest] =
    useState<AgentPublishFocusRequest | null>(null);
  const requestPublishSubTab = useCallback(
    (conversationId: string, tab: AgentPublishSubTab) => {
      useAgentArtifactUiStore
        .getState()
        .requestPublishSubTab(conversationId, tab);
    },
    [],
  );
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
    clearAgentConversationSelection: clearStoredAgentConversationSelection,
    focusedProjectId,
    lastRuntimeByProjectId,
    runtimeByConversationId,
    selectConversation: selectStoredConversation,
    selectedProjectId,
    setActiveConversation,
    setFocusedProject,
    setLastRuntimeForProjectMode,
    setRuntimeForConversation,
    storedSelectedConversationId,
  } = useAgentsSessionBindings({
    setOptimisticSelectedConversationId,
  });
  const selectedConversationIdRef = useRef<string | null>(null);
  const selectConversation = useCallback(
    (projectId: string | null, conversationId: string) => {
      selectedConversationIdRef.current = conversationId;
      selectStoredConversation(projectId, conversationId);
    },
    [selectStoredConversation],
  );
  const clearAgentConversationSelection = useCallback(() => {
    selectedConversationIdRef.current = null;
    clearStoredAgentConversationSelection();
  }, [clearStoredAgentConversationSelection]);
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
  const conversationScopedChatFocus = getConversationScopedChatFocus(
    chatFocus,
    selectedConversationId,
  );
  const automationRunFocusSeededConversationRef = useRef<string | null>(null);
  useEffect(() => {
    selectedConversationIdRef.current = selectedConversationId;
  }, [selectedConversationId]);
  useEffect(() => {
    if (!selectedConversationId) {
      setVisibleAgentScope(null);
      return;
    }
    setVisibleAgentScope({
      workspaceConversationId: selectedConversationId,
      ...((conversationScopedChatFocus.type === "workspace" ||
        conversationScopedChatFocus.type === "workspace_review") && {
        visibleConversationId:
          conversationScopedChatFocus.type === "workspace_review"
            ? conversationScopedChatFocus.conversationId
            : selectedConversationId,
      }),
      ...(conversationScopedChatFocus.type === "automation_run" && {
        visibleConversationId: conversationScopedChatFocus.conversationId,
        automationRunId: conversationScopedChatFocus.runId,
        automationConversationId: conversationScopedChatFocus.conversationId,
      }),
    });
    return () => setVisibleAgentScope(null);
  }, [conversationScopedChatFocus, selectedConversationId, setVisibleAgentScope]);
  useEffect(() => {
    setChatFocus({ type: "workspace" });
    setLastVerificationFocus(null);
    setPublishFocusRequest(null);
    setTaskArtifactFocusRequest(null);
    setSelectedTaskArtifactId(null);
    automationRunFocusSeededConversationRef.current = null;
  }, [selectedConversationId]);
  const externalTaskArtifactFocusRequest = useAgentSessionStore((state) =>
    selectedConversationId
      ? state.taskArtifactFocusRequestByConversationId[selectedConversationId] ??
        null
      : null,
  );
  const externalAutomationRunFocusRequest = useAgentSessionStore((state) =>
    selectedConversationId
      ? state.automationRunFocusRequestByConversationId[selectedConversationId] ??
        null
      : null,
  );
  useEffect(() => {
    if (!selectedConversationId || activeConversation?.contextType !== "project") {
      return;
    }
    void preflightAgentWorkspaceFreshness(queryClient, selectedConversationId);
  }, [activeConversation?.contextType, queryClient, selectedConversationId]);
  const focusedArtifactIdeationSession =
    getFocusedArtifactIdeationSession(conversationScopedChatFocus);
  const focusedArtifactIdeationSessionId =
    focusedArtifactIdeationSession?.sessionId ?? null;
  const handleFocusIdeationSession = useCallback((sessionId: string) => {
    const conversationId = selectedConversationIdRef.current;
    if (!conversationId) {
      return;
    }
    setChatFocus((current) =>
      current.type === "ideation" &&
      current.conversationId === conversationId &&
      current.sessionId === sessionId
        ? current
        : { type: "ideation", conversationId, sessionId },
    );
  }, []);
  const handleFocusIdeationSessionForConversation = useCallback(
    (conversationId: string, sessionId: string) => {
      if (selectedConversationIdRef.current !== conversationId) {
        return;
      }
      handleFocusIdeationSession(sessionId);
    },
    [handleFocusIdeationSession],
  );
  const handleFocusVerificationSession = useCallback(
    (parentSessionId: string, childSessionId: string) => {
      const conversationId = selectedConversationIdRef.current;
      if (!conversationId) {
        return;
      }
      const nextFocus: Extract<AgentsChatFocus, { type: "verification" }> = {
        type: "verification",
        conversationId,
        parentSessionId,
        childSessionId,
      };
      setLastVerificationFocus(nextFocus);
      setChatFocus((current) =>
        current.type === "verification" &&
        current.conversationId === conversationId &&
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
  const handleFocusAutomationRun = useCallback(
    (
      automationId: string,
      runId: string,
      conversationId: string,
      options?: AutomationRunFocusOptions,
    ) => {
      const nextFocus: Extract<AgentsChatFocus, { type: "automation_run" }> = {
        type: "automation_run",
        automationId,
        runId,
        conversationId,
      };
      setChatFocus((current) =>
        current.type === "automation_run" &&
        current.automationId === automationId &&
        current.runId === runId &&
        current.conversationId === conversationId
          ? current
          : nextFocus,
      );
      if (selectedConversationId && options) {
        const seededTab = getAutomationConversationTabPolicy({
          surface: "run",
          runStatus: options.runStatus,
          judgeState: options.judgeState,
          workspaceMode: options.workspaceMode,
          availability: {
            hasPlanArtifact: options.hasPlanArtifact,
            hasPullRequest: options.hasPullRequest,
            canStartPlan: false,
          },
        }).defaultTab;
        seedAgentArtifactTab(selectedConversationId, seededTab, false);
        automationRunFocusSeededConversationRef.current = selectedConversationId;
      }
    },
    [selectedConversationId],
  );
  const handleFocusWorkspaceReview = useCallback((
    conversationId: string,
    runtimeHint?: AgentRuntimeSelection,
  ) => {
    setChatFocus((current) =>
      focusWorkspaceReview(current, conversationId, runtimeHint),
    );
  }, []);
  const handleFocusWorkspaceRepair = useCallback((conversationId: string) => {
    setChatFocus((current) =>
      current.type === "workspace_repair" &&
      current.conversationId === conversationId
        ? current
        : { type: "workspace_repair", conversationId },
    );
  }, []);
  const handleFocusPrFixer = useCallback((conversationId: string) => {
    setChatFocus((current) =>
      current.type === "pr_fixer" && current.conversationId === conversationId
        ? current
        : { type: "pr_fixer", conversationId },
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
  const focusedChildRuntimeConversationId =
    chatFocus.type === "workspace_review" ||
    chatFocus.type === "workspace_repair" ||
    chatFocus.type === "pr_fixer"
      ? chatFocus.conversationId
      : null;
  const focusedChildRuntimeSummaryQuery = useConversationSummary(
    focusedChildRuntimeConversationId,
    { enabled: Boolean(focusedChildRuntimeConversationId) },
  );
  const focusedWorkspaceReviewConversation = useMemo(
    () => {
      const summary = focusedChildRuntimeSummaryQuery.data;
      if (
        summary &&
        summary.id === focusedChildRuntimeConversationId
      ) {
        return toProjectAgentConversation(summary);
      }
      return focusedChildRuntimeConversationId
        ? focusedConversations.data?.find(
            (conversation) =>
              conversation.id === focusedChildRuntimeConversationId,
          ) ?? null
        : null;
    },
    [
      focusedConversations.data,
      focusedChildRuntimeConversationId,
      focusedChildRuntimeSummaryQuery.data,
    ],
  );
  const activeRuntimeConversationId =
    focusedChildRuntimeConversationId ?? selectedConversationId;
  const reviewerRoleDefaults = useManualRoleDefaults(activeProjectId);
  const activeProject = useMemo(
    () => projects.find((project) => project.id === activeProjectId) ?? null,
    [activeProjectId, projects],
  );
  const workspaceReviewerRuntime = useMemo(
    () =>
      runtimeFromManualRoleDefault(
        reviewerRoleDefaults.catalog?.roles.find(
          (entry) => entry.role === "workspace_reviewer",
        )?.effective ?? null,
        modelRegistry,
      ),
    [modelRegistry, reviewerRoleDefaults.catalog],
  );
  const {
    activeConversationMode,
    activeConversationModeLocked,
    activeWorkspace,
    activeWorkspaceFreshness,
    focusedWorkspaceReviewServiceTier,
    normalizedActiveRuntime,
    publishShortcutLabel,
    terminalArchivedReason,
    terminalUnavailableReason,
  } = useAgentsWorkspaceModel({
    activeConversation,
    activeProject,
    focusedWorkspaceReviewConversation,
    focusedWorkspaceReviewConversationId:
      chatFocus.type === "workspace_review"
        ? focusedChildRuntimeConversationId
        : null,
    focusedWorkspaceReviewRuntimeHint:
      chatFocus.type === "workspace_review"
        ? (chatFocus.runtimeHint ?? null)
        : null,
    modelRegistry,
    optimisticWorkspacesByConversationId,
    runtimeByConversationId,
    selectedConversationId,
    workspaceReviewerRuntime,
  });
  const activeProjectBaseBranch = useMemo(
    () => activeProject?.baseBranch ?? null,
    [activeProject],
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
    hasAttachedPlanArtifact,
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
    queryFn: ({ signal }) =>
      chatApi.getAgentWorkspaceReviewContext(workspaceReviewConversationId!, {
        signal,
      }),
    enabled: shouldLoadWorkspaceReviewContext,
    staleTime: 5_000,
    refetchInterval: (query) =>
      query.state.data?.monitor.status === "reviewing" ? 2_000 : false,
  });
  const workspaceReviewContext = workspaceReviewContextForConversation(
    workspaceReviewContextQuery.data,
    workspaceReviewConversationId,
  );
  const hydratedTerminalWorkspaceReviewRef = useRef<string | null>(null);
  useEffect(() => {
    if (!workspaceReviewConversationId || !workspaceReviewContext) {
      hydratedTerminalWorkspaceReviewRef.current = null;
      return;
    }
    const { monitor } = workspaceReviewContext;
    if (monitor.status === "reviewing") {
      return;
    }
    const hydrationKey = `${workspaceReviewConversationId}:${monitor.updatedAt}:${monitor.status}`;
    if (hydratedTerminalWorkspaceReviewRef.current === hydrationKey) {
      return;
    }
    hydratedTerminalWorkspaceReviewRef.current = hydrationKey;
    void refreshWorkspaceReviewContext(
      queryClient,
      workspaceReviewConversationId,
      "full_target",
    ).catch(() => undefined);
  }, [
    queryClient,
    workspaceReviewContext,
    workspaceReviewConversationId,
  ]);
  const isWorkspaceReviewRunning =
    workspaceReviewContext?.monitor.status === "reviewing" ||
    workspaceReviewContext?.monitor.reviewGateStatus === "reviewing";
  const promoteWorkspaceReviewPublishShortcut =
    hasCurrentPassedWorkspaceReview(workspaceReviewContext);
  const workspaceReviewArtifactId =
    workspaceReviewContext?.monitor.reviewArtifactId ?? null;
  const prReviewArtifactId = prReviewContext?.monitor?.reviewArtifactId ?? null;
  const reviewArtifactId = workspaceReviewArtifactId ?? prReviewArtifactId;
  const shouldShowPrReviewTab = Boolean(prReviewContext || prReviewArtifactId);
  const shouldShowWorkspaceReviewTab = Boolean(
    workspaceReviewContext?.shouldShowTab || workspaceReviewArtifactId,
  );
  const availableArtifactTabsWithReview = useMemo<AgentArtifactTab[]>(() => {
    const tabs =
      activeConversation?.contextType === "project" &&
      hasActiveConversationIssues &&
      !availableArtifactTabs.includes("issues")
        ? (["issues", ...availableArtifactTabs] as AgentArtifactTab[])
        : availableArtifactTabs;
    const withReview: AgentArtifactTab[] = shouldShowPrReviewTab
      ? (tabs.includes("review") ? [...tabs] : [...tabs, "review"])
      : tabs.filter((tab) => tab !== "review");
    return activeConversation?.coordinationMode === "rx_native_team" &&
      featureFlags.agentConversationTeam
      ? [...withReview.filter((tab) => tab !== "team"), "team"]
      : withReview.filter((tab) => tab !== "team");
  }, [
    activeConversation?.contextType,
    activeConversation?.coordinationMode,
    availableArtifactTabs,
    hasActiveConversationIssues,
    shouldShowPrReviewTab,
    featureFlags.agentConversationTeam,
  ]);
  const hasAutomationArtifact =
    activeConversation?.agentMode === "automation" &&
    Boolean(activeConversation.automationId);
  const hasPersonaArtifact =
    activeConversation?.agentMode === "persona_builder";
  const hasAutoOpenArtifactsWithReview =
    hasAutoOpenArtifacts ||
    hasAutomationArtifact ||
    hasPersonaArtifact ||
    Boolean(reviewArtifactId) ||
    shouldShowWorkspaceReviewTab;
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
  const conversationScopedLastVerificationFocus =
    lastVerificationFocus?.conversationId === selectedConversationId
      ? lastVerificationFocus
      : null;
  useEffect(() => {
    if (
      !selectedConversationId ||
      !knownFocusIdeationSessionId ||
      !latestVerificationChildQuery.isSuccess
    ) {
      return;
    }
    if (!latestVerificationChildSessionId) {
      setLastVerificationFocus((current) =>
        current?.conversationId === selectedConversationId &&
        current.parentSessionId === knownFocusIdeationSessionId
          ? null
          : current,
      );
      return;
    }
    const nextFocus: Extract<AgentsChatFocus, { type: "verification" }> = {
      type: "verification",
      conversationId: selectedConversationId,
      parentSessionId: knownFocusIdeationSessionId,
      childSessionId: latestVerificationChildSessionId,
    };
    setLastVerificationFocus((current) =>
      current?.conversationId === nextFocus.conversationId &&
      current.parentSessionId === nextFocus.parentSessionId &&
      current.childSessionId === nextFocus.childSessionId
        ? current
        : nextFocus,
    );
  }, [
    knownFocusIdeationSessionId,
    latestVerificationChildQuery.isSuccess,
    latestVerificationChildSessionId,
    selectedConversationId,
  ]);
  const focusSwitcherIdeationSessionId =
    knownFocusIdeationSessionId ??
    conversationScopedLastVerificationFocus?.parentSessionId ??
    null;
  const verificationFocusTarget =
    conversationScopedLastVerificationFocus &&
    conversationScopedLastVerificationFocus.parentSessionId ===
      focusSwitcherIdeationSessionId
      ? conversationScopedLastVerificationFocus
      : null;
  const taskRuntimeFocusTarget =
    chatFocus.type === "task_runtime" ? chatFocus : null;
  const automationRunFocusTarget =
    chatFocus.type === "automation_run" ? chatFocus : null;
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
  const repairRuntimeConversationId =
    workspaceReviewContext?.repairRuntimeConversationId ?? null;
  const repairFixerKind = workspaceReviewContext?.repairFixerKind ?? null;
  const workspaceRepairFocusTarget = useMemo(
    () =>
      getWorkspaceRepairFocusTarget({
        reviewFixerConversationId:
          workspaceReviewContext?.monitor.reviewFixerConversationId ?? null,
        repairRuntimeConversationId,
        repairFixerKind,
      }),
    [
      repairFixerKind,
      repairRuntimeConversationId,
      workspaceReviewContext?.monitor.reviewFixerConversationId,
    ],
  );
  const prFixerFocusTarget = useMemo(
    () =>
      repairFixerKind === "pr_fixer" && repairRuntimeConversationId
        ? ({
            type: "pr_fixer",
            conversationId: repairRuntimeConversationId,
          } satisfies Extract<AgentsChatFocus, { type: "pr_fixer" }>)
        : null,
    [repairFixerKind, repairRuntimeConversationId],
  );
  const reviewFixerIsActive = ["routing", "queued", "running"].includes(
    workspaceReviewContext?.monitor.reviewFixerStatus ?? "",
  );
  const repairAttemptIsActive = Boolean(
    repairRuntimeConversationId && repairFixerKind,
  );
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
    if (
      !workspaceRepairFocusTarget ||
      (!reviewFixerIsActive &&
        !(repairAttemptIsActive && repairFixerKind === "workspace_repair"))
    ) {
      return;
    }
    setChatFocus((current) =>
      current.type === "workspace_repair" &&
      current.conversationId === workspaceRepairFocusTarget.conversationId
        ? current
        : workspaceRepairFocusTarget,
    );
  }, [
    repairAttemptIsActive,
    repairFixerKind,
    reviewFixerIsActive,
    workspaceRepairFocusTarget,
  ]);
  useEffect(() => {
    if (!prFixerFocusTarget || !repairAttemptIsActive) return;
    setChatFocus((current) =>
      current.type === "pr_fixer" &&
      current.conversationId === prFixerFocusTarget.conversationId
        ? current
        : prFixerFocusTarget,
    );
  }, [prFixerFocusTarget, repairAttemptIsActive]);
  const chatFocusOptions = useMemo(() => {
    return getAgentChatFocusSwitchOptions({
      mode: activeConversationMode,
      focusSwitcherIdeationSessionId,
      verificationFocusTarget,
      taskRuntimeFocusTarget,
      workspaceReviewFocusTarget,
      workspaceRepairFocusTarget,
      prFixerFocusTarget,
      automationRunFocusTarget,
      hasPlanArtifact: hasAttachedPlanArtifact,
    });
  }, [
    activeConversationMode,
    automationRunFocusTarget,
    focusSwitcherIdeationSessionId,
    hasAttachedPlanArtifact,
    taskRuntimeFocusTarget,
    verificationFocusTarget,
    workspaceReviewFocusTarget,
    workspaceRepairFocusTarget,
    prFixerFocusTarget,
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
        return;
      }

      if (type === "workspace_repair" && workspaceRepairFocusTarget) {
        setChatFocus(workspaceRepairFocusTarget);
        return;
      }

      if (type === "pr_fixer" && prFixerFocusTarget) {
        setChatFocus(prFixerFocusTarget);
        return;
      }

      if (type === "automation_run" && automationRunFocusTarget) {
        setChatFocus(automationRunFocusTarget);
      }
    },
    [
      chatFocusOptions,
      focusSwitcherIdeationSessionId,
      handleFocusIdeationSession,
      handleReturnToWorkspaceChat,
      taskRuntimeFocusTarget,
      verificationFocusTarget,
      automationRunFocusTarget,
      workspaceReviewFocusTarget,
      workspaceRepairFocusTarget,
      prFixerFocusTarget,
    ],
  );
  const {
    hideArtifactTab,
    openArtifactTab,
    scheduleArtifactPanePreload,
    seedArtifactTab,
    setArtifactPaneVisibility,
    setArtifactTaskMode,
    showArtifactTab,
    toggleArtifactPaneVisibility,
  } = useAgentArtifactController({
    hasAutoOpenArtifacts: hasAutoOpenArtifactsWithReview,
    selectedConversationId,
  });
  useEffect(() => {
    if (
      !activeConversation?.automationId ||
      !activeConversation.automationRunId ||
      !activeConversation.projectId ||
      chatFocus.type === "automation_run"
    ) {
      return;
    }

    void requestAutomationRunOpen(
      queryClient,
      {
        projectId: activeConversation.projectId,
        automationId: activeConversation.automationId,
        runId: activeConversation.automationRunId,
        conversationId: activeConversation.id,
      },
      { fallback: "clear-selection" },
    );
  }, [
    activeConversation?.automationId,
    activeConversation?.automationRunId,
    activeConversation?.id,
    activeConversation?.projectId,
    chatFocus.type,
    queryClient,
  ]);
  useEffect(() => {
    if (
      !externalAutomationRunFocusRequest ||
      !selectedConversationId ||
      activeConversation?.agentMode !== "automation" ||
      activeConversation.automationId !==
        externalAutomationRunFocusRequest.automationId ||
      activeConversation.automationRunId
    ) {
      return;
    }

    handleFocusAutomationRun(
      externalAutomationRunFocusRequest.automationId,
      externalAutomationRunFocusRequest.runId,
      externalAutomationRunFocusRequest.conversationId,
    );

    const seededTab =
      externalAutomationRunFocusRequest.seededTab ??
      defaultAutomationRunFocusTab(externalAutomationRunFocusRequest);
    const optimisticArtifactState =
      useAgentArtifactUiStore.getState().artifactByConversationId[
        selectedConversationId
      ] ?? null;
    const seededTabIsHidden = Boolean(
      optimisticArtifactState?.hiddenTabs?.includes(seededTab) ||
        useAgentSessionStore.getState().artifactByConversationId[
          selectedConversationId
        ]?.hiddenTabs?.includes(seededTab),
    );
    if (seededTabIsHidden) {
      openArtifactTab(selectedConversationId, seededTab);
    } else if (
      !optimisticArtifactState ||
      optimisticArtifactState.activeTab === seededTab
    ) {
      seedAgentArtifactTab(
        selectedConversationId,
        seededTab,
        hasAutoOpenArtifactsWithReview,
      );
    }
    automationRunFocusSeededConversationRef.current = selectedConversationId;

    useAgentSessionStore
      .getState()
      .clearAutomationRunFocusRequest(
        selectedConversationId,
        externalAutomationRunFocusRequest.requestId,
      );
  }, [
    activeConversation?.agentMode,
    activeConversation?.automationId,
    activeConversation?.automationRunId,
    externalAutomationRunFocusRequest,
    handleFocusAutomationRun,
    hasAutoOpenArtifactsWithReview,
    openArtifactTab,
    selectedConversationId,
  ]);
  useEffect(() => {
    if (
      !selectedConversationId ||
      activeConversation?.agentMode !== "automation" ||
      !activeConversation.automationId ||
      chatFocus.type === "automation_run" ||
      externalAutomationRunFocusRequest ||
      automationRunFocusSeededConversationRef.current === selectedConversationId
    ) {
      return;
    }
    seedArtifactTab(selectedConversationId, "automation");
  }, [
    activeConversation?.agentMode,
    activeConversation?.automationId,
    chatFocus.type,
    externalAutomationRunFocusRequest,
    seedArtifactTab,
    selectedConversationId,
  ]);
  useEffect(() => {
    if (
      !selectedConversationId ||
      activeConversation?.agentMode !== "persona_builder"
    ) {
      return;
    }
    seedArtifactTab(selectedConversationId, "persona");
  }, [activeConversation?.agentMode, seedArtifactTab, selectedConversationId]);
  useEffect(() => {
    if (!externalTaskArtifactFocusRequest || !selectedConversationId) {
      return;
    }
    setSelectedTaskArtifactId(externalTaskArtifactFocusRequest.taskId);
    setTaskArtifactFocusRequest(externalTaskArtifactFocusRequest);
    openArtifactTab(selectedConversationId, "tasks");
  }, [
    externalTaskArtifactFocusRequest,
    openArtifactTab,
    selectedConversationId,
  ]);
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
      useAgentSessionStore
        .getState()
        .focusTaskArtifact(selectedConversationId, taskId);
    },
    [selectedConversationId],
  );

  const { clearAutoManagedTitle, handleAutoManagedTitle } = useAgentsAutoTitle({
    findConversationById,
    invalidateProjectConversations,
  });
  const seedJiraTabForConversation = useCallback(
    (conversationId: string) => {
      seedArtifactTab(conversationId, "jira");
    },
    [seedArtifactTab],
  );
  const seedLinearTabForConversation = useCallback(
    (conversationId: string) => {
      seedArtifactTab(conversationId, "linear");
    },
    [seedArtifactTab],
  );
  const seedClickUpTabForConversation = useCallback(
    (conversationId: string) => {
      seedArtifactTab(conversationId, "clickup");
    },
    [seedArtifactTab],
  );
  const seedGranolaTabForConversation = useCallback(
    (conversationId: string) => {
      seedArtifactTab(conversationId, "granola");
    },
    [seedArtifactTab],
  );
  useEffect(() => {
    const invalidatePrReviewArtifact = (payload: PrReviewArtifactEventPayload) => {
      const conversationId = payload.conversationId ?? payload.conversation_id;
      if (!conversationId) {
        return;
      }
      void queryClient.invalidateQueries({
        queryKey: agentWorkspaceKeys.prReview(conversationId),
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

    const refreshWorkspaceReviewArtifact = (
      payload: PrReviewArtifactEventPayload,
    ) => {
      const conversationId = payload.conversationId ?? payload.conversation_id;
      if (
        !conversationId ||
        !workspaceReviewConversationId ||
        conversationId !== workspaceReviewConversationId
      ) {
        return;
      }
      void refreshWorkspaceReviewAfterSignal(
        queryClient,
        conversationId,
      ).catch(() => undefined);
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
        invalidatePrReviewArtifact(payload);
        const conversationId = payload.conversationId ?? payload.conversation_id;
        if (conversationId && conversationId === selectedConversationId) {
          openArtifactTab(conversationId, "review");
        }
      },
    );
    const unsubscribeUpdated = eventBus.subscribe<PrReviewArtifactEventPayload>(
      "pr_review_artifact:updated",
      invalidatePrReviewArtifact,
    );
    const unsubscribeWorkspaceCreated =
      eventBus.subscribe<PrReviewArtifactEventPayload>(
        "workspace_review_artifact:created",
        (payload) => {
          refreshWorkspaceReviewArtifact(payload);
          const conversationId = payload.conversationId ?? payload.conversation_id;
          if (
            conversationId &&
            conversationId === workspaceReviewConversationId &&
            conversationId === selectedConversationId
          ) {
            requestPublishSubTab(conversationId, "review");
            openArtifactTab(conversationId, "publish");
          }
        },
      );
    const unsubscribeWorkspaceUpdated =
      eventBus.subscribe<PrReviewArtifactEventPayload>(
        "workspace_review_artifact:updated",
        refreshWorkspaceReviewArtifact,
      );

    return () => {
      unsubscribeCreated();
      unsubscribeUpdated();
      unsubscribeWorkspaceCreated();
      unsubscribeWorkspaceUpdated();
    };
  }, [
    eventBus,
    openArtifactTab,
    queryClient,
    requestPublishSubTab,
    selectedConversationId,
    workspaceReviewConversationId,
  ]);

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
      if (
        workspaceReviewConversationId &&
        activeConversation?.contextType === "project" &&
        lifecyclePayloadOwnsWorkspaceReviewQuery(
          payload,
          workspaceReviewConversationId,
          workspaceReviewChildConversationId,
        )
      ) {
        void refreshWorkspaceReviewAfterSignal(
          queryClient,
          workspaceReviewConversationId,
        ).catch(() => undefined);
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
  }, [
    activeConversation?.contextType,
    eventBus,
    queryClient,
    workspaceReviewConversationId,
    workspaceReviewChildConversationId,
  ]);

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
    onJiraLinked: seedJiraTabForConversation,
    onLinearLinked: seedLinearTabForConversation,
    onClickUpLinked: seedClickUpTabForConversation,
    onGranolaLinked: seedGranolaTabForConversation,
  });

  const {
    handleArchiveConversation,
    handleBulkArchiveConversations,
    handleBulkMuteConversations,
    handleArchiveProject,
    handleAutoRenameConversation,
    handleForkConversation,
    handleRenameConversation,
    handleRestoreConversation,
    handleSetConversationMuted,
    handleSidebarCreateAgent,
    handleSidebarFocusProject,
    handleSidebarSelectConversation,
    handleStartPersonaBuilder,
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
    setStartConversationDraft,
    setOptimisticConversationsById,
    setOptimisticSelectedConversationId,
    setOptimisticWorkspacesByConversationId,
    setRuntimeForConversation,
  });
  const handleStartActivePersonaBuilder = useCallback(() => {
    if (activeConversation) {
      handleStartPersonaBuilder(activeConversation);
    }
  }, [activeConversation, handleStartPersonaBuilder]);
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
    onPublishSubTabRequest: requestPublishSubTab,
    openArtifactTab,
    scheduleArtifactPanePreload,
    selectedConversationId,
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
          selectConversation(agentConversation.projectId!, agentConversation.id);
          setActiveConversation(
            getAgentConversationStoreKey(agentConversation),
            agentConversation.id,
          );
          void invalidateProjectConversations(agentConversation.projectId!);
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
      requestPublishSubTab(selectedConversationId, "changes");
      openArtifactTab(selectedConversationId, "publish");
    },
    [
      handleReturnToWorkspaceChat,
      isWorkspaceReviewRunning,
      openArtifactTab,
      requestPublishSubTab,
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
      if (tab === "publish") {
        handleOpenPublishPane();
        return;
      }
      handleSelectArtifact(tab);
    },
    [
      handleOpenPublishPane,
      handleReturnToWorkspaceChat,
      handleSelectArtifact,
      isWorkspaceReviewRunning,
    ],
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
        seedJiraTabForConversation(event.result.conversationId);
      } else if (hasLinearIntegrationReference(event.composerIntegrationReferences)) {
        seedLinearTabForConversation(event.result.conversationId);
      } else if (hasClickUpIntegrationReference(event.composerIntegrationReferences)) {
        seedClickUpTabForConversation(event.result.conversationId);
      } else if (hasGranolaIntegrationReference(event.composerIntegrationReferences)) {
        seedGranolaTabForConversation(event.result.conversationId);
      }
    },
    [
      handleAgentUserMessageAutoTitle,
      invalidateAgentUserMessageJira,
      seedClickUpTabForConversation,
      seedJiraTabForConversation,
      seedGranolaTabForConversation,
      seedLinearTabForConversation,
    ],
  );
  const handleStartRuntimePreferenceChange = useCallback(
    (targetProjectId: string, mode: AgentConversationWorkspaceMode, runtime: AgentRuntimeSelection) => {
      setLastRuntimeForProjectMode(
        targetProjectId,
        mode,
        normalizeRuntimeForPersistence(runtime, modelRegistry),
      );
    },
    [modelRegistry, setLastRuntimeForProjectMode],
  );

  const { handlePublishWorkspace, publishAttemptsByConversationId } =
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
    handleActiveCapabilityChange,
    handleActiveConversationModeChange,
    handleActiveConversationModeMenuOpen,
    handleActiveEffortChange,
    handleActiveModelChange,
    handleActiveProviderChange,
    switchingConversationModeId,
    updatingCapabilityConversationId,
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
    onBulkArchiveConversations: handleBulkArchiveConversations,
    onBulkMuteConversations: handleBulkMuteConversations,
    onSetConversationMuted: handleSetConversationMuted,
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
      chatFocus: conversationScopedChatFocus,
      chatFocusOptions,
      defaultProjectId,
      defaultRuntime,
      hasAttachedPlanArtifact,
      hasAutoOpenArtifacts: hasAutoOpenArtifactsWithReview,
      focusedWorkspaceReviewServiceTier,
      isLoadingProjects,
      modelRegistry,
      normalizedActiveRuntime,
      onActiveConversationModeChange: handleActiveConversationModeChange,
      onActiveConversationModeMenuOpen: handleActiveConversationModeMenuOpen,
      onActiveCapabilityChange: handleActiveCapabilityChange,
      onActiveEffortChange: handleActiveEffortChange,
      onActiveModelChange: handleActiveModelChange,
      onActiveProviderChange: handleActiveProviderChange,
      onAgentUserMessageSent: handleAgentUserMessageSent,
      onConversationModeSwitched: handleConversationModeSwitched,
      onFocusIdeationSession: handleFocusIdeationSession,
      onFocusIdeationSessionForConversation:
        handleFocusIdeationSessionForConversation,
      onFocusWorkspaceReview: handleFocusWorkspaceReview,
      onFocusWorkspaceRepair: handleFocusWorkspaceRepair,
      onFocusPrFixer: handleFocusPrFixer,
      onFocusVerificationSession: handleFocusVerificationSession,
      onFocusTaskRuntime: handleFocusTaskRuntime,
      onFocusAutomationRun: handleFocusAutomationRun,
      onOpenTaskArtifact: handleOpenTaskArtifact,
      ...(onOpenAutomation ? { onOpenAutomation } : {}),
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
      onStartPersonaBuilder: handleStartActivePersonaBuilder,
      onToggleArtifacts: toggleArtifactPaneVisibility,
      onSelectChatFocus: handleSelectChatFocus,
      projects,
      publishShortcutLabel,
      promotePublishShortcut: promoteWorkspaceReviewPublishShortcut,
      publishAttemptsByConversationId,
      selectedConversationId,
      selectedTaskArtifactId,
      setTerminalChatDockElement,
      switchingConversationModeId,
      updatingCapabilityConversationId,
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
      focusedIdeationSession: focusedArtifactIdeationSession,
      hasAutoOpenArtifacts: hasAutoOpenArtifactsWithReview,
      hideArtifactTab,
      isArtifactResizing,
      openArtifactTab,
      automationRunFocusTarget,
      panelDockElement: terminalPanelDockElement,
      publishFocusRequest,
      publishAttemptsByConversationId,
      selectedConversationId,
      setArtifactPaneVisibility,
      setArtifactTaskMode,
      showArtifactTab,
      setTerminalPanelDockElement,
      taskArtifactFocusRequest,
      terminalArchivedReason,
      terminalUnavailableReason,
      onConversationModeSwitched: handleConversationModeSwitched,
      onFocusAutomationRun: handleFocusAutomationRun,
      onFocusVerificationSession: handleFocusVerificationSession,
      onFocusIdeationSessionForConversation:
        handleFocusIdeationSessionForConversation,
      onFocusWorkspaceReview: handleFocusWorkspaceReview,
      onFocusTaskRuntime: handleFocusTaskRuntime,
      ...(onOpenAutomation ? { onOpenAutomation } : {}),
      onOpenPublish: handleOpenPublishPaneWithChatFocus,
      onTaskArtifactSelectionChange: handleTaskArtifactSelectionChange,
      onPublishWorkspace: handlePublishWorkspace,
      onResizeReset: handleArtifactResizeReset,
      onResizeStart: handleArtifactResizeStart,
      onSelectArtifact: handleSelectArtifactWithChatFocus,
    },
  };
}
