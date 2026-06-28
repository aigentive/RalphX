import {
  AlertCircle,
  CheckCircle2,
  FileText,
  GitPullRequestArrow,
  LayoutGrid,
  Network,
  ClipboardList,
  ScrollText,
  Ticket,
  X,
} from "lucide-react";
import type { ElementType } from "react";
import { lazy, memo, Suspense, useCallback, useEffect, useMemo, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";

import { artifactApi } from "@/api/artifact";
import { atlassianApi } from "@/api/atlassian";
import { granolaApi } from "@/api/granola";
import { linearApi } from "@/api/linear";
import { ideationApi, toTaskProposal } from "@/api/ideation";
import { verificationApi } from "@/api/verification";
import {
  chatApi,
  type AgentConversationWorkspace,
  type AgentConversationWorkspaceFreshness,
  type AgentWorkspaceReviewContext,
  type StartAgentWorkspaceReviewResult,
} from "@/api/chat";
import { Button } from "@/components/ui/button";
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import { cn } from "@/lib/utils";
import { withAlpha } from "@/lib/theme-colors";
import type { TeamMetadata } from "@/components/Ideation/PlanDisplay";
import type {
  AgentArtifactTab,
  AgentTaskArtifactMode,
} from "@/stores/agentSessionStore";
import { useConversationHistoryWindow } from "@/hooks/useChat";
import { ideationKeys } from "@/hooks/useIdeation";
import { useDependencyGraph } from "@/hooks/useDependencyGraph";
import { useVerificationStatus, verificationStatusKey } from "@/hooks/useVerificationStatus";
import type { Artifact } from "@/types/artifact";
import type { IdeationSession, TaskProposal, VerificationStatus } from "@/types/ideation";
import type {
  DependencyGraphResponse,
} from "@/api/ideation.types";
import type { AgentConversation } from "./agentConversations";
import { AgentReviewPanel } from "./AgentReviewPanel";
import {
  getVisibleIdeationArtifactTabs,
  type IdeationArtifactTab,
} from "./agentArtifactTabs";
import { resolveAttachedIdeationSessionId } from "./attachedIdeationSession";
import type { ProposalDetailEnrichment } from "@/components/Ideation/ProposalDetailSheet";
import { EmptyArtifactState } from "./AgentsArtifactEmptyState";
import { AgentPublishPanel } from "./AgentsPublishPanel";
import { shouldShowAgentWorkspacePublishSurface } from "./agentWorkspacePublishState";
import type { AgentPublishFocusRequest } from "./agentPublishFocus";
import type { AgentTaskArtifactFocusRequest } from "./agentTaskArtifactFocus";
import {
  agentWorkspaceKeys,
  invalidateWorkspaceQueries,
  prReviewContextForConversation,
  workspaceReviewContextForConversation,
} from "./agentWorkspaceQueries";
import {
  hasOpenAgentConversationIssues,
  useAgentConversationIssues,
} from "./agentConversationIssueQueries";
import {
  buildPlanActionHint,
  isPlanRecommendationCheckPending,
  PLAN_IMPLEMENT_DIRECTLY_REQUEST,
  PLAN_TO_PROPOSALS_REQUEST,
} from "./agentPlanModeActions";

const EMPTY_PROPOSAL_HIGHLIGHTS = new Set<string>();

function noop() {}

const LazyTaskGraphView = lazy(() =>
  import("@/components/TaskGraph").then((module) => ({ default: module.TaskGraphView })),
);
const LazyTaskBoard = lazy(() =>
  import("@/components/tasks/TaskBoard").then((module) => ({ default: module.TaskBoard })),
);
const LazyAgentsTaskDetailOverlay = lazy(() =>
  import("@/components/agents/task-details/AgentsTaskDetailOverlay").then((module) => ({
    default: module.AgentsTaskDetailOverlay,
  })),
);
const LazyExportPlanDialog = lazy(() =>
  import("@/components/Ideation/ExportPlanDialog").then((module) => ({
    default: module.ExportPlanDialog,
  })),
);
const LazyPlanDisplay = lazy(() =>
  import("@/components/Ideation/PlanDisplay").then((module) => ({ default: module.PlanDisplay })),
);
const LazyPlanEditor = lazy(() =>
  import("@/components/Ideation/PlanEditor").then((module) => ({ default: module.PlanEditor })),
);
const LazyPlanEmptyState = lazy(() =>
  import("@/components/Ideation/PlanEmptyState").then((module) => ({
    default: module.PlanEmptyState,
  })),
);
const LazyProposalsTabContent = lazy(() =>
  import("@/components/Ideation/ProposalsTabContent").then((module) => ({
    default: module.ProposalsTabContent,
  })),
);
const LazyProposalDetailSheet = lazy(() =>
  import("@/components/Ideation/ProposalDetailSheet").then((module) => ({
    default: module.ProposalDetailSheet,
  })),
);
const LazyVerificationPanel = lazy(() =>
  import("@/components/Ideation/VerificationPanel").then((module) => ({
    default: module.VerificationPanel,
  })),
);
const LazyAgentsJiraIssuePanel = lazy(() =>
  import("@/components/agents/AgentsJiraIssuePanel").then((module) => ({
    default: module.AgentsJiraIssuePanel,
  })),
);
const LazyAgentsLinearIssuePanel = lazy(() =>
  import("@/components/agents/AgentsLinearIssuePanel").then((module) => ({
    default: module.AgentsLinearIssuePanel,
  })),
);
const LazyAgentsGranolaNotePanel = lazy(() =>
  import("@/components/agents/AgentsGranolaNotePanel").then((module) => ({
    default: module.AgentsGranolaNotePanel,
  })),
);
const LazyAgentsIssuesPanel = lazy(() =>
  import("@/components/agents/AgentsIssuesPanel").then((module) => ({
    default: module.AgentsIssuesPanel,
  })),
);
const LazyPullRequestDetailPanel = lazy(() =>
  import("@/components/pr/PullRequestDetailPanel").then((module) => ({
    default: module.PullRequestDetailPanel,
  })),
);

const ARTIFACT_TABS: Array<{
  id: IdeationArtifactTab;
  label: string;
  icon: ElementType;
}> = [
  { id: "issues", label: "Issues", icon: AlertCircle },
  { id: "plan", label: "Plan", icon: FileText },
  { id: "verification", label: "Verification", icon: CheckCircle2 },
  { id: "proposal", label: "Proposals", icon: GitPullRequestArrow },
  { id: "tasks", label: "Tasks", icon: ClipboardList },
];

const REVIEW_TAB = {
  id: "review" as const,
  label: "Review",
  icon: FileText,
};

const PUBLISH_TAB = {
  id: "publish" as const,
  label: "Commit & Publish",
  icon: GitPullRequestArrow,
};

const JIRA_TAB = {
  id: "jira" as const,
  label: "Jira",
  icon: Ticket,
};

const LINEAR_TAB = {
  id: "linear" as const,
  label: "Linear",
  icon: Ticket,
};

const GRANOLA_TAB = {
  id: "granola" as const,
  label: "Granola",
  icon: ScrollText,
};

const PR_TAB = {
  id: "pr" as const,
  label: "PR",
  icon: GitPullRequestArrow,
};

const SELECTED_TASK_STORAGE_PREFIX = "agents:artifact:selected-task:";

function workspaceHasPullRequest(
  workspace: AgentConversationWorkspace | null | undefined,
): boolean {
  return Boolean(workspace?.publicationPrNumber != null || workspace?.sourcePullRequest);
}

function readSelectedTaskForConversation(
  conversationId: string | null,
): string | null {
  if (!conversationId) return null;
  if (typeof window === "undefined") return null;
  try {
    return window.localStorage.getItem(
      `${SELECTED_TASK_STORAGE_PREFIX}${conversationId}`,
    );
  } catch {
    return null;
  }
}

function writeSelectedTaskForConversation(
  conversationId: string | null,
  taskId: string | null,
): void {
  if (!conversationId) return;
  if (typeof window === "undefined") return;
  try {
    const key = `${SELECTED_TASK_STORAGE_PREFIX}${conversationId}`;
    if (taskId) {
      window.localStorage.setItem(key, taskId);
    } else {
      window.localStorage.removeItem(key);
    }
  } catch {
    // Ignore quota / private-mode write failures.
  }
}

interface AgentsArtifactPaneProps {
  conversation: AgentConversation | null;
  workspace?: AgentConversationWorkspace | null;
  activeWorkspaceFreshness?: AgentConversationWorkspaceFreshness | undefined;
  projectBaseBranch?: string | null;
  focusedIdeationSessionId?: string | null;
  activeTab: AgentArtifactTab;
  taskMode: AgentTaskArtifactMode;
  onTabChange: (tab: AgentArtifactTab) => void;
  onTaskModeChange: (mode: AgentTaskArtifactMode) => void;
  onPublishWorkspace: ((conversationId: string) => Promise<void>) | undefined;
  isPublishingWorkspace?: boolean;
  publishFocusRequest?: AgentPublishFocusRequest | null;
  taskFocusRequest?: AgentTaskArtifactFocusRequest | null;
  onFocusVerificationSession: ((parentSessionId: string, childSessionId: string) => void) | undefined;
  onTaskArtifactSelectionChange?: (taskId: string | null) => void;
  onClose: () => void;
}

export const AgentsArtifactPane = memo(function AgentsArtifactPane({
  conversation,
  workspace = null,
  activeWorkspaceFreshness,
  projectBaseBranch = null,
  focusedIdeationSessionId = null,
  activeTab,
  taskMode,
  onTabChange,
  onTaskModeChange,
  onPublishWorkspace,
  isPublishingWorkspace = false,
  publishFocusRequest = null,
  taskFocusRequest = null,
  onFocusVerificationSession,
  onTaskArtifactSelectionChange,
  onClose,
}: AgentsArtifactPaneProps) {
  const queryClient = useQueryClient();
  const canHydrateIdeationArtifacts = Boolean(
    conversation?.contextType === "ideation" ||
      focusedIdeationSessionId ||
      workspace?.mode === "ideation" ||
      workspace?.mode === "plan" ||
      workspace?.linkedIdeationSessionId ||
      workspace?.linkedPlanBranchId,
  );
  const showPublishTab = shouldShowAgentWorkspacePublishSurface(workspace);
  const showPullRequestTab = workspaceHasPullRequest(workspace);
  const shouldLoadIdeationData = canHydrateIdeationArtifacts;
  const conversationQuery = useConversationHistoryWindow(conversation?.id ?? null, {
    enabled: shouldLoadIdeationData && !focusedIdeationSessionId && !!conversation?.id,
    pageSize: 40,
  });
  const conversationData = conversationQuery.data;
  const conversationMessages = useMemo(
    () =>
      shouldLoadIdeationData &&
      conversationData &&
      conversationData.conversation?.id === conversation?.id
        ? conversationData.messages
        : [],
    [conversationData, conversation?.id, shouldLoadIdeationData],
  );
  const attachedSessionId = useMemo(
    () =>
      focusedIdeationSessionId ??
      (shouldLoadIdeationData
        ? resolveAttachedIdeationSessionId(
            conversation,
            conversationMessages,
            workspace?.linkedIdeationSessionId ?? null,
          )
        : null),
    [
      conversation,
      conversationMessages,
      focusedIdeationSessionId,
      shouldLoadIdeationData,
      workspace?.linkedIdeationSessionId,
    ],
  );
  const atlassianSettingsQuery = useQuery({
    queryKey: ["atlassian", "settings"],
    queryFn: () => atlassianApi.getSettings(),
    staleTime: 30_000,
  });
  const showJiraTab = Boolean(
    atlassianSettingsQuery.data?.enabled &&
      atlassianSettingsQuery.data?.jiraAvailable,
  );
  const linearSettingsQuery = useQuery({
    queryKey: ["linear", "settings"],
    queryFn: () => linearApi.getSettings(),
    staleTime: 30_000,
  });
  const showLinearTab = Boolean(
    linearSettingsQuery.data?.enabled &&
      linearSettingsQuery.data?.issueSearchAvailable,
  );
  const granolaSettingsQuery = useQuery({
    queryKey: ["granola", "settings"],
    queryFn: () => granolaApi.getSettings(),
    staleTime: 30_000,
  });
  const showGranolaTab = Boolean(
    granolaSettingsQuery.data?.enabled &&
      granolaSettingsQuery.data?.validationStatus === "valid",
  );
  const [displayedVerificationStatus, setDisplayedVerificationStatus] = useState<{
    status: VerificationStatus;
    inProgress: boolean;
  } | null>(null);
  const conversationId = conversation?.id ?? null;
  const prReviewConversationId =
    workspace?.mode === "review_pr" ? workspace.conversationId : null;
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
  const shouldLoadWorkspaceReviewContext = Boolean(
    conversationId &&
      workspace &&
      ["edit", "ideation", "plan", "review_pr"].includes(workspace.mode),
  );
  const workspaceReviewContextQuery = useQuery({
    queryKey: agentWorkspaceKeys.workspaceReview(conversationId ?? ""),
    queryFn: () => chatApi.getAgentWorkspaceReviewContext(conversationId!),
    enabled: shouldLoadWorkspaceReviewContext,
    staleTime: 5_000,
    refetchInterval: (query) =>
      query.state.data?.monitor.status === "reviewing" ? 2_000 : false,
  });
  const workspaceReviewContext = workspaceReviewContextForConversation(
    workspaceReviewContextQuery.data,
    conversationId,
  );
  const workspaceReviewArtifactId =
    workspaceReviewContext?.monitor.reviewArtifactId ?? null;
  const prReviewArtifactId =
    prReviewContext?.monitor?.reviewArtifactId ?? null;
  const reviewArtifactId = workspaceReviewArtifactId ?? prReviewArtifactId;
  const reviewArtifactQuery = useQuery({
    queryKey: ["agents", "artifact", reviewArtifactId],
    queryFn: () => artifactApi.get(reviewArtifactId!),
    enabled: Boolean(reviewArtifactId),
    staleTime: 5_000,
  });
  const reviewArtifact =
    reviewArtifactId && reviewArtifactQuery.data?.id === reviewArtifactId
      ? reviewArtifactQuery.data
      : null;
  const startWorkspaceReviewMutation = useMutation({
    mutationFn: ({
      conversationId,
      force,
    }: {
      conversationId: string;
      force: boolean;
    }) => chatApi.startAgentWorkspaceReview(conversationId, { force }),
    onSuccess: (result, variables) => {
      void queryClient.invalidateQueries({
        queryKey: agentWorkspaceKeys.workspaceReview(variables.conversationId),
      });
      const artifactId = result.monitor.reviewArtifactId;
      if (artifactId) {
        void queryClient.invalidateQueries({
          queryKey: ["agents", "artifact", artifactId],
        });
      }
    },
  });
  const [taskArtifactSelectedId, setTaskArtifactSelectedIdState] =
    useState<string | null>(() => readSelectedTaskForConversation(conversationId));
  useEffect(() => {
    setDisplayedVerificationStatus(null);
  }, [attachedSessionId]);
  useEffect(() => {
    setTaskArtifactSelectedIdState(readSelectedTaskForConversation(conversationId));
  }, [conversationId]);
  const setTaskArtifactSelectedId = useCallback(
    (id: string | null) => {
      setTaskArtifactSelectedIdState(id);
      writeSelectedTaskForConversation(conversationId, id);
      onTaskArtifactSelectionChange?.(id);
    },
    [conversationId, onTaskArtifactSelectionChange],
  );
  const taskFocusRequestId = taskFocusRequest?.requestId ?? null;
  const taskFocusRequestTaskId = taskFocusRequest?.taskId ?? null;
  useEffect(() => {
    if (!taskFocusRequestTaskId) {
      return;
    }
    setTaskArtifactSelectedId(taskFocusRequestTaskId);
  }, [setTaskArtifactSelectedId, taskFocusRequestId, taskFocusRequestTaskId]);
  const sessionQuery = useQuery({
    queryKey: ideationKeys.sessionWithData(attachedSessionId ?? ""),
    queryFn: () => ideationApi.sessions.getWithData(attachedSessionId!),
    enabled: shouldLoadIdeationData && !!attachedSessionId,
    staleTime: 0,
    refetchInterval: (query) =>
      query.state.data?.session.verificationInProgress ||
      query.state.data?.session.acceptanceStatus === "pending"
        ? 3_000
        : false,
  });
  const rawSessionData = sessionQuery.data;
  const sessionData =
    attachedSessionId && rawSessionData?.session.id === attachedSessionId
      ? rawSessionData
      : null;
  const session = sessionData?.session ? (sessionData.session as IdeationSession) : null;
  const proposals = useMemo<TaskProposal[]>(
    () => (sessionData?.proposals ?? []).map(toTaskProposal),
    [sessionData?.proposals],
  );
  const planArtifactId = shouldLoadIdeationData
    ? sessionData?.session.planArtifactId ?? sessionData?.session.inheritedPlanArtifactId ?? null
    : null;
  const sessionVerificationStatus =
    sessionData?.session.verificationStatus ?? "unverified";
  const hasVerificationEvidence = Boolean(
    sessionData &&
      (sessionData.session.verificationInProgress ||
        sessionVerificationStatus !== "unverified" ||
        sessionData.session.gapScore != null ||
        (displayedVerificationStatus !== null &&
          (displayedVerificationStatus.inProgress ||
            displayedVerificationStatus.status !== "unverified"))),
  );
  const proposalCount = proposals.length;
  const artifactMode =
    workspace?.mode ??
    conversation?.agentMode ??
    (conversation?.contextType === "ideation" ? "ideation" : null);
  const issueConversationId =
    conversation?.contextType === "project" ? conversation.id : null;
  const conversationIssuesQuery = useAgentConversationIssues(issueConversationId);
  const hasConversationIssues = hasOpenAgentConversationIssues(
    conversationIssuesQuery.data,
  );
  const availableIdeationTabIds = useMemo(
    () =>
      getVisibleIdeationArtifactTabs({
        hasAttachedIdeationSession: Boolean(sessionData),
        hasPlanArtifact: Boolean(planArtifactId),
        hasProposals: proposalCount > 0,
        hasVerificationEvidence,
        hasExecutionTasks: Boolean(
          workspace?.linkedPlanBranchId ||
            sessionData?.session.acceptanceStatus === "accepted" ||
            sessionData?.session.convertedAt,
        ),
        artifactMode,
      }),
    [
      artifactMode,
      hasVerificationEvidence,
      planArtifactId,
      proposalCount,
      sessionData,
      workspace?.linkedPlanBranchId,
    ],
  );
  const availableArtifactTabIds = useMemo<IdeationArtifactTab[]>(() => {
    const tabs =
      conversation?.contextType === "project" && hasConversationIssues
        ? (["issues", ...availableIdeationTabIds] as IdeationArtifactTab[])
        : availableIdeationTabIds;
    const shouldShowReviewTab =
      Boolean(reviewArtifactId) || Boolean(workspaceReviewContext?.shouldShowTab);
    if (!shouldShowReviewTab || tabs.includes("review")) {
      return tabs;
    }
    return [...tabs, "review"];
  }, [
    availableIdeationTabIds,
    conversation?.contextType,
    hasConversationIssues,
    reviewArtifactId,
    workspaceReviewContext?.shouldShowTab,
  ]);
  const visibleTabs = useMemo(
    () => [
      ...ARTIFACT_TABS.filter((tab) => availableArtifactTabIds.includes(tab.id)),
      ...(showPullRequestTab ? [PR_TAB] : []),
      ...(showJiraTab ? [JIRA_TAB] : []),
      ...(showLinearTab ? [LINEAR_TAB] : []),
      ...(showGranolaTab ? [GRANOLA_TAB] : []),
      ...(availableArtifactTabIds.includes("review") ? [REVIEW_TAB] : []),
      ...(showPublishTab ? [PUBLISH_TAB] : []),
    ],
    [
      availableArtifactTabIds,
      showGranolaTab,
      showJiraTab,
      showLinearTab,
      showPublishTab,
      showPullRequestTab,
    ],
  );
  const fallbackActiveTab =
    workspaceReviewContext?.shouldShowTab || reviewArtifactId
      ? "review"
      : showPullRequestTab
        ? "pr"
        : showJiraTab
          ? "jira"
          : showLinearTab
            ? "linear"
            : showGranolaTab
              ? "granola"
              : visibleTabs.some((tab) => tab.id === "plan")
                ? "plan"
                : visibleTabs.some((tab) => tab.id === "issues")
                  ? "issues"
                  : visibleTabs.some((tab) => tab.id === "review")
                    ? "review"
                    : "plan";
  const effectiveActiveTab =
    visibleTabs.some((tab) => tab.id === activeTab)
      ? activeTab
      : fallbackActiveTab;
  const isWorkspaceReviewActionPending =
    startWorkspaceReviewMutation.isPending &&
    startWorkspaceReviewMutation.variables?.conversationId === conversationId;
  const workspaceReviewStartResult = workspaceReviewContextForConversation(
    startWorkspaceReviewMutation.data,
    conversationId,
  );
  const reviewDisplayContext = isWorkspaceReviewActionPending
    ? workspaceReviewStartResult ?? workspaceReviewContext
    : workspaceReviewContext ?? workspaceReviewStartResult;
  const isWorkspaceReviewRunning =
    isWorkspaceReviewActionPending ||
    reviewDisplayContext?.monitor.status === "reviewing";
  const workspaceReviewBlocked =
    (isWorkspaceReviewActionPending && Boolean(startWorkspaceReviewMutation.error)) ||
    reviewDisplayContext?.monitor.status === "blocked";
  const reviewTabStatusColor = isWorkspaceReviewRunning
    ? "var(--accent-primary)"
    : workspaceReviewBlocked
      ? "var(--status-error)"
      : reviewDisplayContext?.isOutdated
        ? "var(--status-warning)"
        : reviewDisplayContext?.isCurrent
          ? "var(--status-success)"
          : reviewDisplayContext?.target
            ? "var(--text-muted)"
            : null;
  const shouldLoadVerificationData =
    shouldLoadIdeationData && effectiveActiveTab === "verification";
  const shouldLoadDependencyGraph =
    shouldLoadIdeationData &&
    (effectiveActiveTab === "proposal" || effectiveActiveTab === "tasks");
  const shouldUseSessionPlanQuery =
    shouldLoadIdeationData &&
    sessionData?.session.sessionFlow === "planning" &&
    !!attachedSessionId;
  const planArtifactQueryKey = shouldUseSessionPlanQuery
    ? ["agents", "session-plan", attachedSessionId, planArtifactId]
    : ["agents", "artifact", planArtifactId];
  const planArtifactQuery = useQuery({
    queryKey: planArtifactQueryKey,
    queryFn: () =>
      shouldUseSessionPlanQuery
        ? artifactApi.getSessionPlan(attachedSessionId!)
        : artifactApi.get(planArtifactId!),
    enabled:
      shouldLoadIdeationData &&
      (shouldUseSessionPlanQuery ? !!attachedSessionId : !!planArtifactId),
    staleTime: 5_000,
  });
  const planArtifact = planArtifactQuery.data ?? null;
  const isPlanHydrating =
    shouldLoadIdeationData &&
    effectiveActiveTab === "plan" &&
    !planArtifact &&
    !!attachedSessionId &&
    (planArtifactQuery.isFetching || sessionQuery.isFetching);
  const verificationQuery = useVerificationStatus(
    shouldLoadVerificationData ? attachedSessionId ?? undefined : undefined,
  );
  const dependencyQuery = useDependencyGraph(
    shouldLoadDependencyGraph ? attachedSessionId ?? "" : "",
  );
  const verificationData =
    attachedSessionId && verificationQuery.data?.sessionId === attachedSessionId
      ? verificationQuery.data
      : null;
  const dependencyGraph = attachedSessionId && sessionData ? dependencyQuery.data ?? null : null;
  const verificationState =
    displayedVerificationStatus?.status ??
    verificationData?.status ??
    sessionData?.session.verificationStatus ??
    "unverified";
  const verificationInProgress =
    displayedVerificationStatus?.inProgress ??
    verificationData?.inProgress ??
    sessionData?.session.verificationInProgress ??
    false;
  const handlePlanUpdated = useCallback(
    (updatedPlan: Artifact) => {
      queryClient.setQueryData(["agents", "artifact", updatedPlan.id], updatedPlan);
      if (attachedSessionId) {
        queryClient.setQueryData(
          ["agents", "session-plan", attachedSessionId, updatedPlan.id],
          updatedPlan,
        );
        queryClient.setQueryData(
          ["agents", "plan-approval", attachedSessionId],
          updatedPlan,
        );
      }
    },
    [attachedSessionId, queryClient],
  );
  const handleStartReview = useCallback((force: boolean) => {
    if (!conversationId || isWorkspaceReviewActionPending) {
      return;
    }
    startWorkspaceReviewMutation.mutate({ conversationId, force });
  }, [conversationId, isWorkspaceReviewActionPending, startWorkspaceReviewMutation]);

  return (
    <aside
      className="h-full w-full min-w-0 flex flex-col overflow-hidden border-l"
      style={{
        background: "var(--bg-surface)",
        borderColor: "var(--overlay-faint)",
      }}
      data-testid="agents-artifact-pane"
    >
      <div
        data-testid="agents-artifact-tab-row"
        className="h-11 px-4 flex items-center gap-0 border-b shrink-0"
        style={{
          background: withAlpha("var(--bg-surface)", 60),
          backdropFilter: "blur(12px)",
          WebkitBackdropFilter: "blur(12px)",
          borderColor: "var(--overlay-faint)",
        }}
      >
        <div className="flex h-full items-stretch gap-0 min-w-0 self-stretch">
          {visibleTabs.map(({ id, label, icon: Icon }) => {
            const isActive = effectiveActiveTab === id;
            const count = id === "proposal" ? proposalCount : 0;

            let iconColor: string | undefined;
            let iconPulse = false;
            let tabStatusColor: string | null = null;
            if (id === "verification") {
              if (verificationInProgress) {
                iconColor = "var(--accent-primary)";
                iconPulse = true;
              } else if (
                verificationState === "verified" ||
                verificationState === "imported_verified"
              ) {
                iconColor = "var(--status-success)";
              } else if (verificationState === "needs_revision") {
                iconColor = "var(--status-warning)";
              }
            } else if (id === "review") {
              iconColor = reviewTabStatusColor ?? undefined;
              iconPulse = isWorkspaceReviewRunning;
              tabStatusColor = reviewTabStatusColor;
            }

            return (
              <button
                key={id}
                type="button"
                onClick={() => {
                  if (
                    id === "tasks" &&
                    effectiveActiveTab === "tasks" &&
                    taskArtifactSelectedId
                  ) {
                    setTaskArtifactSelectedId(null);
                    return;
                  }
                  onTabChange(id);
                }}
                className={cn(
                  "relative flex h-full self-stretch items-center gap-1.5 bg-transparent px-3 text-[0.75rem] font-medium transition-colors duration-150 rounded-none shadow-none outline-none ring-0 focus:ring-0 focus:outline-none focus-visible:outline-none focus-visible:ring-0 appearance-none",
                  id === "tasks" ? "hidden xl:flex" : ""
                )}
                style={{
                  color: isActive ? "var(--text-primary)" : "var(--text-muted)",
                  background: "transparent",
                  boxShadow: "none",
                }}
                data-testid={`agents-artifact-tab-${id}`}
                data-theme-button-skip="true"
              >
                <Icon
                  className={cn("w-4 h-4 shrink-0", iconPulse ? "animate-pulse" : "")}
                  style={iconColor ? { color: iconColor } : undefined}
                />
                <span>{label}</span>
                {tabStatusColor && (
                  <span
                    aria-hidden="true"
                    className="h-1.5 w-1.5 rounded-full"
                    style={{ backgroundColor: tabStatusColor }}
                  />
                )}
                {count > 0 && (
                  <span
                    className="text-[0.625rem] font-semibold px-1.5 py-0.5 rounded-full"
                    style={{
                      background: isActive
                        ? withAlpha("var(--accent-primary)", 15)
                        : "var(--overlay-weak)",
                      color: isActive ? "var(--accent-primary)" : "var(--text-muted)",
                    }}
                  >
                    {count}
                  </span>
                )}
                {isActive && (
                  <span
                    className="absolute -bottom-px left-3 right-3 h-[2px] rounded-full"
                    style={{ background: "var(--accent-primary)" }}
                  />
                )}
              </button>
            );
          })}
        </div>

        <div className="ml-auto flex items-center gap-1">
          {effectiveActiveTab === "tasks" && (
            <div
              className="h-8 p-0.5 flex items-center rounded-md border"
              style={{
                borderColor: "var(--border-subtle)",
                background: "var(--bg-base)",
              }}
              data-testid="agents-task-mode-toggle"
            >
              <Tooltip>
                <TooltipTrigger asChild>
                  <Button
                    type="button"
                    variant="ghost"
                    size="sm"
                    onClick={() => onTaskModeChange("graph")}
                    className="h-7 w-7 p-0"
                    style={{
                      color: taskMode === "graph" ? "var(--accent-primary)" : "var(--text-muted)",
                      background: taskMode === "graph" ? "var(--accent-muted)" : "transparent",
                    }}
                    aria-label="Graph"
                  >
                    <Network className="w-4 h-4" />
                  </Button>
                </TooltipTrigger>
                <TooltipContent side="bottom" className="text-xs">
                  Graph
                </TooltipContent>
              </Tooltip>
              <Tooltip>
                <TooltipTrigger asChild>
                  <Button
                    type="button"
                    variant="ghost"
                    size="sm"
                    onClick={() => onTaskModeChange("kanban")}
                    className="h-7 w-7 p-0"
                    style={{
                      color: taskMode === "kanban" ? "var(--accent-primary)" : "var(--text-muted)",
                      background: taskMode === "kanban" ? "var(--accent-muted)" : "transparent",
                    }}
                    aria-label="Kanban"
                  >
                    <LayoutGrid className="w-4 h-4" />
                  </Button>
                </TooltipTrigger>
                <TooltipContent side="bottom" className="text-xs">
                  Kanban
                </TooltipContent>
              </Tooltip>
            </div>
          )}

          <Tooltip>
            <TooltipTrigger asChild>
              <Button
                type="button"
                variant="ghost"
                size="sm"
                onClick={onClose}
                className="h-8 w-8 p-0"
                aria-label="Close artifacts"
                data-testid="agents-artifact-close"
              >
                <X className="w-4 h-4" />
              </Button>
            </TooltipTrigger>
            <TooltipContent side="bottom" className="text-xs">
              Close artifacts
            </TooltipContent>
          </Tooltip>
        </div>
      </div>

      <div
        className="flex-1 min-h-0 overflow-y-auto"
        data-testid={`agents-artifact-content-${effectiveActiveTab}`}
      >
        <ArtifactContent
          activeTab={effectiveActiveTab}
          workspace={workspace}
          conversationId={conversationId}
          activeWorkspaceFreshness={activeWorkspaceFreshness}
          conversationTitle={conversation?.title ?? null}
          projectBaseBranch={projectBaseBranch}
          isLoading={conversationQuery.isLoading || sessionQuery.isLoading}
          attachedSessionId={attachedSessionId}
          projectId={conversation?.projectId ?? null}
          session={session}
          sessionTitle={sessionData?.session.title ?? null}
          taskMode={taskMode}
          reviewArtifact={reviewArtifact}
          reviewContext={workspaceReviewContext}
          reviewStartResult={workspaceReviewStartResult}
          reviewStartError={
            isWorkspaceReviewActionPending ? startWorkspaceReviewMutation.error : null
          }
          isReviewLoading={
            Boolean(reviewArtifactId) &&
            !reviewArtifact &&
            reviewArtifactQuery.isFetching
          }
          isReviewActionPending={isWorkspaceReviewActionPending}
          onStartReview={handleStartReview}
          planArtifact={planArtifact}
          isPlanLoading={isPlanHydrating}
          onPlanUpdated={handlePlanUpdated}
          dependencyGraph={dependencyGraph}
          proposals={proposals}
          onPublishWorkspace={onPublishWorkspace}
          isPublishingWorkspace={isPublishingWorkspace}
          publishFocusRequest={publishFocusRequest}
          onFocusVerificationSession={onFocusVerificationSession}
          onDisplayedVerificationStatusChange={setDisplayedVerificationStatus}
          verificationState={verificationState}
          verificationInProgress={verificationInProgress}
          onOpenVerification={() => onTabChange("verification")}
          taskArtifactSelectedId={taskArtifactSelectedId}
          onTaskArtifactSelectedIdChange={setTaskArtifactSelectedId}
        />
      </div>
    </aside>
  );
});

type ArtifactContentProps = {
  activeTab: AgentArtifactTab;
  workspace: AgentConversationWorkspace | null;
  conversationId: string | null;
  activeWorkspaceFreshness: AgentConversationWorkspaceFreshness | undefined;
  conversationTitle: string | null;
  projectBaseBranch: string | null;
  isLoading: boolean;
  attachedSessionId: string | null;
  projectId: string | null;
  session: IdeationSession | null;
  sessionTitle: string | null;
  taskMode: AgentTaskArtifactMode;
  reviewArtifact: Artifact | null;
  reviewContext: AgentWorkspaceReviewContext | null;
  reviewStartResult: StartAgentWorkspaceReviewResult | null;
  reviewStartError: Error | null;
  isReviewLoading: boolean;
  isReviewActionPending: boolean;
  onStartReview: (force: boolean) => void;
  planArtifact: Artifact | null;
  isPlanLoading: boolean;
  onPlanUpdated: (updatedPlan: Artifact) => void;
  dependencyGraph: DependencyGraphResponse | null;
  proposals: TaskProposal[];
  onPublishWorkspace: ((conversationId: string) => Promise<void>) | undefined;
  isPublishingWorkspace: boolean;
  publishFocusRequest: AgentPublishFocusRequest | null;
  onFocusVerificationSession: ((parentSessionId: string, childSessionId: string) => void) | undefined;
  onDisplayedVerificationStatusChange: (status: {
    status: VerificationStatus;
    inProgress: boolean;
  } | null) => void;
  verificationState: VerificationStatus | null;
  verificationInProgress: boolean;
  onOpenVerification: () => void;
  taskArtifactSelectedId: string | null;
  onTaskArtifactSelectedIdChange: (id: string | null) => void;
};

function ArtifactContent({
  activeTab,
  workspace,
  conversationId,
  activeWorkspaceFreshness,
  conversationTitle,
  projectBaseBranch,
  isLoading,
  attachedSessionId,
  projectId,
  session,
  sessionTitle,
  taskMode,
  reviewArtifact,
  reviewContext,
  reviewStartResult,
  reviewStartError,
  isReviewLoading,
  isReviewActionPending,
  onStartReview,
  planArtifact,
  isPlanLoading,
  onPlanUpdated,
  dependencyGraph,
  proposals,
  onPublishWorkspace,
  isPublishingWorkspace,
  publishFocusRequest,
  onFocusVerificationSession: _onFocusVerificationSession,
  onDisplayedVerificationStatusChange,
  verificationState,
  verificationInProgress,
  onOpenVerification,
  taskArtifactSelectedId,
  onTaskArtifactSelectedIdChange,
}: ArtifactContentProps) {
  const criticalPathSet = useMemo(
    () => new Set(dependencyGraph?.criticalPath ?? []),
    [dependencyGraph?.criticalPath],
  );
  const [viewingProposalId, setViewingProposalId] = useState<string | null>(null);
  const [viewingEnrichment, setViewingEnrichment] = useState<ProposalDetailEnrichment | undefined>(undefined);
  const viewingProposal = viewingProposalId
    ? proposals.find((p) => p.id === viewingProposalId) ?? null
    : null;
  const handleViewProposal = useCallback(
    (proposalId: string, enrichment: ProposalDetailEnrichment) => {
      setViewingProposalId(proposalId);
      setViewingEnrichment(enrichment);
    },
    [],
  );
  const handleCloseProposalDetail = useCallback(() => {
    setViewingProposalId(null);
    setViewingEnrichment(undefined);
  }, []);
  // Opening the Verification tab no longer auto-focuses the chat on the
  // verification child. The user switches chats explicitly via the composer
  // chat-focus pill instead.
  const handleDisplayedVerificationChildChange = useCallback(
    (_childSessionId: string | null) => {
      // intentionally empty — see comment above.
    },
    [],
  );
  const handleDisplayedVerificationStatusChange = useCallback(
    (status: VerificationStatus, inProgress: boolean) => {
      onDisplayedVerificationStatusChange({ status, inProgress });
    },
    [onDisplayedVerificationStatusChange],
  );

  if (activeTab === "publish") {
    return (
      <AgentPublishPanel
        workspace={workspace}
        conversationTitle={conversationTitle}
        projectBaseBranch={projectBaseBranch}
        onPublishWorkspace={onPublishWorkspace}
        isPublishingWorkspace={isPublishingWorkspace}
        publishFocusRequest={publishFocusRequest}
      />
    );
  }

  if (activeTab === "jira") {
    return (
      <Suspense fallback={<EmptyArtifactState title="Loading Jira..." />}>
        <LazyAgentsJiraIssuePanel
          conversationId={conversationId}
          projectId={projectId}
        />
      </Suspense>
    );
  }

  if (activeTab === "linear") {
    return (
      <Suspense fallback={<EmptyArtifactState title="Loading Linear..." />}>
        <LazyAgentsLinearIssuePanel
          conversationId={conversationId}
          projectId={projectId}
        />
      </Suspense>
    );
  }

  if (activeTab === "granola") {
    return (
      <Suspense fallback={<EmptyArtifactState title="Loading Granola..." />}>
        <LazyAgentsGranolaNotePanel
          conversationId={conversationId}
          projectId={projectId}
        />
      </Suspense>
    );
  }

  if (activeTab === "pr") {
    return (
      <Suspense fallback={<EmptyArtifactState title="Loading pull request..." />}>
        <LazyPullRequestDetailPanel workspace={workspace} />
      </Suspense>
    );
  }

  if (activeTab === "issues") {
    return (
      <Suspense fallback={<EmptyArtifactState title="Loading issues..." />}>
        <LazyAgentsIssuesPanel
          conversationId={conversationId}
          projectId={projectId}
        />
      </Suspense>
    );
  }

  if (activeTab === "review") {
    return (
      <AgentReviewPanel
        reviewArtifact={reviewArtifact}
        reviewContext={reviewContext}
        reviewStartResult={reviewStartResult}
        reviewStartError={reviewStartError}
        isReviewLoading={isReviewLoading}
        isReviewActionPending={isReviewActionPending}
        onStartReview={onStartReview}
      />
    );
  }

  if (isLoading) {
    return <EmptyArtifactState title="Loading attached run..." />;
  }

  if (!attachedSessionId) {
    return (
      <EmptyArtifactState
        title="No ideation run attached"
        detail="Start ideation from this agent chat to populate plan, verification, proposals, and tasks here."
      />
    );
  }

  if (activeTab === "plan") {
    return (
      <AgentPlanPanel
        workspace={workspace}
        activeWorkspaceFreshness={activeWorkspaceFreshness}
        session={session}
        sessionTitle={sessionTitle}
        planArtifact={planArtifact}
        isPlanLoading={isPlanLoading}
        proposals={proposals}
        onPlanUpdated={onPlanUpdated}
        verificationState={verificationState}
        verificationInProgress={verificationInProgress}
        onOpenVerification={onOpenVerification}
      />
    );
  }

  if (activeTab === "verification") {
    if (!session) {
      return <EmptyArtifactState title="No verification data yet" />;
    }
    return (
      <div className="flex h-full min-h-0 flex-col">
        <Suspense fallback={<EmptyArtifactState title="Loading verification..." />}>
          <LazyVerificationPanel
            session={session}
            onDisplayedVerificationChildChange={handleDisplayedVerificationChildChange}
            onDisplayedVerificationStatusChange={handleDisplayedVerificationStatusChange}
          />
        </Suspense>
      </div>
    );
  }

  if (activeTab === "proposal") {
    if (!session || proposals.length === 0) {
      return <EmptyArtifactState title="No proposals yet" />;
    }
    return (
      <>
        <Suspense fallback={<EmptyArtifactState title="Loading proposals..." />}>
          <LazyProposalsTabContent
            session={session}
            proposals={proposals}
            dependencyGraph={dependencyGraph}
            criticalPathSet={criticalPathSet}
            highlightedIds={EMPTY_PROPOSAL_HIGHLIGHTS}
            isReadOnly
            onEditProposal={noop}
            onNavigateToTask={noop}
            onViewProposal={handleViewProposal}
            {...(viewingProposalId != null && { selectedProposalId: viewingProposalId })}
            onViewHistoricalPlan={noop}
            onImportPlan={noop}
            onClearAll={noop}
            onAcceptPlan={noop}
            onReviewSync={noop}
            onUndoSync={noop}
            onDismissSync={noop}
            hideToolbar
          />
        </Suspense>
        {viewingProposal && (
          <Suspense fallback={null}>
            <LazyProposalDetailSheet
              proposal={viewingProposal}
              {...(viewingEnrichment !== undefined && { enrichment: viewingEnrichment })}
              isReadOnly
              onClose={handleCloseProposalDetail}
            />
          </Suspense>
        )}
      </>
    );
  }

  return (
    <TaskArtifactSurface
      projectId={projectId}
      sessionId={attachedSessionId}
      mode={taskMode}
      selectedTaskId={taskArtifactSelectedId}
      onSelectedTaskIdChange={onTaskArtifactSelectedIdChange}
    />
  );
}

function AgentPlanPanel({
  workspace,
  activeWorkspaceFreshness,
  session,
  sessionTitle,
  planArtifact,
  isPlanLoading,
  proposals,
  onPlanUpdated,
  verificationState,
  verificationInProgress,
  onOpenVerification,
}: {
  workspace: AgentConversationWorkspace | null;
  activeWorkspaceFreshness: AgentConversationWorkspaceFreshness | undefined;
  session: IdeationSession | null;
  sessionTitle: string | null;
  planArtifact: Artifact | null;
  isPlanLoading: boolean;
  proposals: TaskProposal[];
  onPlanUpdated: (updatedPlan: Artifact) => void;
  verificationState: VerificationStatus | null;
  verificationInProgress: boolean;
  onOpenVerification: () => void;
}) {
  const [isEditing, setIsEditing] = useState(false);
  const [isPlanExpanded, setIsPlanExpanded] = useState(true);
  const [exportDialogOpen, setExportDialogOpen] = useState(false);
  const [isApprovingPlan, setIsApprovingPlan] = useState(false);
  const [isStartingPlanVerification, setIsStartingPlanVerification] = useState(false);
  const [isImplementingPlanDirectly, setIsImplementingPlanDirectly] = useState(false);
  const queryClient = useQueryClient();

  useEffect(() => {
    setIsEditing(false);
    setIsPlanExpanded(true);
  }, [planArtifact?.id, planArtifact?.metadata.version]);

  const teamMetadata = useMemo<TeamMetadata | undefined>(() => {
    if (!session?.teamMode || session.teamMode === "solo") {
      return undefined;
    }
    return {
      teamIdeated: true,
      teamMode: session.teamMode as "research" | "debate",
      teammateCount: session.teamConfig?.maxTeammates ?? 0,
      findings: [],
    };
  }, [session?.teamConfig?.maxTeammates, session?.teamMode]);

  const handleCreateProposals = useCallback(async () => {
    if (!session) return;
    try {
      const shouldPromoteWorkspace =
        session.sessionFlow === "planning" &&
        workspace?.mode !== "ideation" &&
        workspace?.linkedIdeationSessionId === session.id &&
        Boolean(workspace.conversationId);

      if (shouldPromoteWorkspace && workspace?.conversationId) {
        const result = await chatApi.switchAgentConversationMode({
          conversationId: workspace.conversationId,
          mode: "ideation",
        });
        if (result.workspace) {
          queryClient.setQueryData(
            agentWorkspaceKeys.workspace(workspace.conversationId),
            result.workspace,
          );
        }
        void invalidateWorkspaceQueries(queryClient, workspace.conversationId);
      }

      await chatApi.sendAgentMessage("ideation", session.id, PLAN_TO_PROPOSALS_REQUEST);
    } catch (err) {
      console.error("Failed to create proposals:", err);
      toast.error("Failed to request proposal creation");
    }
  }, [queryClient, session, workspace]);

  const isPlanningSession = session?.sessionFlow === "planning";
  const isOwnedCurrentPlan = Boolean(
    isPlanningSession &&
      session?.planArtifactId &&
      planArtifact?.id === session.planArtifactId,
  );
  const planApprovalStatus = isOwnedCurrentPlan
    ? planArtifact?.planApproval?.status ?? "draft"
    : undefined;
  const isPlanApproved = planApprovalStatus === "approved";
  const canShowPlanModeControls =
    workspace?.mode === "plan" &&
    activeWorkspaceFreshness?.hasUncommittedChanges !== true;
  const canApprovePlan =
    canShowPlanModeControls && isOwnedCurrentPlan && planApprovalStatus === "draft";
  const canShowApprovedPlanActions =
    canShowPlanModeControls && !isImplementingPlanDirectly;
  const isPlanVerificationSatisfied =
    verificationState === "verified" || verificationState === "imported_verified";
  const canVerifyPlan =
    canShowApprovedPlanActions &&
    isOwnedCurrentPlan &&
    !isPlanVerificationSatisfied;
  const canCreateProposals =
    canShowApprovedPlanActions &&
    session !== null &&
    (!isPlanningSession || isPlanApproved);
  const canImplementDirectly = Boolean(
    canShowApprovedPlanActions &&
      isOwnedCurrentPlan &&
      isPlanApproved &&
      session?.projectId &&
      workspace?.conversationId,
  );
  const planComplexityQuery = useQuery({
    queryKey: [
      "agents",
      "plan-complexity",
      session?.id,
      planArtifact?.id,
      planArtifact?.metadata.version,
    ],
    queryFn: () => artifactApi.getPlanComplexityAssessment(session!.id),
    enabled: Boolean(
      session &&
        isOwnedCurrentPlan &&
        isPlanApproved &&
        canShowApprovedPlanActions,
    ),
    staleTime: 5_000,
    refetchInterval: (query) => (query.state.data ? false : 4_000),
  });
  const isPlanRecommendationPending = isPlanRecommendationCheckPending({
    assessment: planComplexityQuery.data,
    isFetching:
      (planComplexityQuery.isFetching || planComplexityQuery.isLoading) &&
      !planComplexityQuery.data,
    approvedAt: planArtifact?.planApproval?.approvedAt,
  });
  const planActionHint = buildPlanActionHint({
    assessment: planComplexityQuery.data,
    isAssessing: isPlanRecommendationPending,
    canChoose: canImplementDirectly && canCreateProposals,
  });

  const handleApprovePlan = useCallback(async () => {
    if (!session || !planArtifact || !canApprovePlan) {
      return;
    }
    setIsApprovingPlan(true);
    try {
      const approvedPlan = await artifactApi.approvePlanArtifact({
        sessionId: session.id,
        artifactId: planArtifact.id,
      });
      onPlanUpdated(approvedPlan);
      queryClient.setQueryData(
        ["agents", "session-plan", session.id, approvedPlan.id],
        approvedPlan,
      );
      queryClient.setQueryData(
        ["agents", "plan-approval", session.id],
        approvedPlan,
      );
      await queryClient.invalidateQueries({
        queryKey: ["agents", "plan-complexity", session.id],
      });
      toast.success("Plan approved");
    } catch (err) {
      console.error("Failed to approve plan:", err);
      toast.error(err instanceof Error ? err.message : "Failed to approve plan");
    } finally {
      setIsApprovingPlan(false);
    }
  }, [canApprovePlan, onPlanUpdated, planArtifact, queryClient, session]);

  const handleImplementDirectly = useCallback(async () => {
    if (!session || !workspace?.conversationId || !canImplementDirectly) {
      return;
    }
    setIsImplementingPlanDirectly(true);
    try {
      if (workspace.mode !== "edit") {
        const result = await chatApi.switchAgentConversationMode({
          conversationId: workspace.conversationId,
          mode: "edit",
        });
        if (result.workspace) {
          queryClient.setQueryData(
            agentWorkspaceKeys.workspace(workspace.conversationId),
            result.workspace,
          );
        }
        void invalidateWorkspaceQueries(queryClient, workspace.conversationId);
      }

      await chatApi.sendAgentMessage(
        "project",
        session.projectId,
        PLAN_IMPLEMENT_DIRECTLY_REQUEST,
        undefined,
        undefined,
        {
          conversationId: workspace.conversationId,
          suppressUserMessage: true,
        },
      );
      toast.success("Implementation started");
    } catch (err) {
      console.error("Failed to implement plan directly:", err);
      toast.error(err instanceof Error ? err.message : "Failed to start implementation");
    } finally {
      setIsImplementingPlanDirectly(false);
    }
  }, [canImplementDirectly, queryClient, session, workspace]);

  const handleVerifyPlan = useCallback(async () => {
    if (!session || !canVerifyPlan || verificationInProgress) {
      return;
    }
    setIsStartingPlanVerification(true);
    try {
      let disabledSpecialists: string[] = [];
      try {
        const specialists = await verificationApi.getSpecialists();
        disabledSpecialists = specialists.specialists
          .filter((specialist) => !specialist.enabled_by_default)
          .map((specialist) => specialist.name);
      } catch (err) {
        console.warn("Failed to load verification specialists:", err);
      }

      await verificationApi.confirm(session.id, disabledSpecialists);
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: verificationStatusKey(session.id) }),
        queryClient.invalidateQueries({ queryKey: ideationKeys.sessionWithData(session.id) }),
        queryClient.invalidateQueries({ queryKey: ideationKeys.sessions() }),
      ]);
      onOpenVerification();
      toast.success("Plan verification started");
    } catch (err) {
      console.error("Failed to start plan verification:", err);
      toast.error(
        err instanceof Error ? err.message : "Failed to start plan verification",
      );
    } finally {
      setIsStartingPlanVerification(false);
    }
  }, [canVerifyPlan, onOpenVerification, queryClient, session, verificationInProgress]);

  if (isPlanLoading) {
    return <EmptyArtifactState title="Loading plan..." />;
  }

  return (
    <div className="min-h-full px-4 pb-4 pt-4">
      {planArtifact ? (
        isEditing ? (
          <Suspense fallback={<EmptyArtifactState title="Loading plan editor..." />}>
            <LazyPlanEditor
              plan={planArtifact}
              onSave={(updated) => {
                onPlanUpdated(updated);
                setIsEditing(false);
              }}
              onCancel={() => setIsEditing(false)}
            />
          </Suspense>
        ) : (
          <Suspense fallback={<EmptyArtifactState title="Loading plan..." />}>
            <LazyPlanDisplay
              plan={planArtifact}
              linkedProposalsCount={proposals.filter((proposal) => proposal.planArtifactId === planArtifact.id).length}
              onEdit={() => setIsEditing(true)}
              onExport={() => setExportDialogOpen(true)}
              isExpanded={isPlanExpanded}
              onExpandedChange={setIsPlanExpanded}
              chromeless
              {...(teamMetadata !== undefined && { teamMetadata })}
              {...(canApprovePlan && {
                showApprove: true,
                onApprove: handleApprovePlan,
                isApproving: isApprovingPlan,
              })}
              {...(isOwnedCurrentPlan && { isApproved: isPlanApproved })}
              {...(canVerifyPlan && {
                onVerifyPlan: handleVerifyPlan,
                isVerifyingPlan:
                  isStartingPlanVerification || verificationInProgress,
              })}
              {...(canImplementDirectly && {
                onImplementDirectly: handleImplementDirectly,
                isImplementingDirectly: isImplementingPlanDirectly,
              })}
              {...(planComplexityQuery.data && {
                primaryPlanAction: planComplexityQuery.data.recommendedAction,
              })}
              {...(isPlanRecommendationPending && {
                isPlanActionRecommendationPending: true,
              })}
              {...(planActionHint && { planActionHint })}
              {...(canCreateProposals && { onCreateProposals: handleCreateProposals })}
              {...(isPlanningSession && {
                createProposalsLabel: "Create Proposals",
              })}
            />
          </Suspense>
        )
      ) : (
        <Suspense fallback={<EmptyArtifactState title="Loading plan..." />}>
          <LazyPlanEmptyState />
        </Suspense>
      )}

      {session && exportDialogOpen && (
        <Suspense fallback={null}>
          <LazyExportPlanDialog
            open={exportDialogOpen}
            onOpenChange={setExportDialogOpen}
            sessionId={session.id}
            sessionTitle={sessionTitle}
            verificationStatus={session.verificationStatus ?? "unverified"}
            planArtifact={planArtifact}
            projectId={session.projectId}
          />
        </Suspense>
      )}
    </div>
  );
}

function TaskArtifactSurface({
  projectId,
  sessionId,
  mode,
  selectedTaskId,
  onSelectedTaskIdChange,
}: {
  projectId: string | null;
  sessionId: string;
  mode: AgentTaskArtifactMode;
  selectedTaskId: string | null;
  onSelectedTaskIdChange: (id: string | null) => void;
}) {
  const handleTaskSelect = useCallback(
    (taskId: string) => {
      onSelectedTaskIdChange(taskId);
    },
    [onSelectedTaskIdChange],
  );
  const handleCloseTaskDetail = useCallback(() => {
    onSelectedTaskIdChange(null);
  }, [onSelectedTaskIdChange]);

  if (!projectId) {
    return <EmptyArtifactState title="No project selected" />;
  }

  const backLabel = mode === "kanban" ? "Back to Kanban" : "Back to Graph";
  const detailOverlay = selectedTaskId ? (
    <Suspense fallback={null}>
      <LazyAgentsTaskDetailOverlay
        projectId={projectId}
        selectedTaskIdOverride={selectedTaskId}
        onCloseOverride={handleCloseTaskDetail}
        backLabel={backLabel}
        onBack={handleCloseTaskDetail}
        constrainContent
      />
    </Suspense>
  ) : null;

  if (mode === "kanban") {
    return (
      <div className="relative h-full min-h-[520px] overflow-hidden bg-[var(--bg-base)]">
        <Suspense fallback={<EmptyArtifactState title="Loading task board..." />}>
          <LazyTaskBoard
            projectId={projectId}
            ideationSessionId={sessionId}
            onTaskSelect={handleTaskSelect}
            fillWidth
          />
        </Suspense>
        {detailOverlay}
      </div>
    );
  }

  return (
    <div className="relative h-full min-h-[520px] overflow-hidden bg-[var(--bg-base)]">
      <Suspense fallback={<EmptyArtifactState title="Loading task graph..." />}>
        <LazyTaskGraphView
          projectId={projectId}
          ideationSessionId={sessionId}
          hideCanvasControls
          onTaskSelect={handleTaskSelect}
        />
      </Suspense>
      {detailOverlay}
    </div>
  );
}
