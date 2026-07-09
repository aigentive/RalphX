import {
  AlertCircle,
  CheckCircle2,
  FileText,
  GitPullRequestArrow,
  LayoutGrid,
  ListPlus,
  Network,
  Rocket,
  ClipboardList,
  ScrollText,
  ShieldCheck,
  Sparkles,
  Ticket,
  Workflow,
  X,
} from "lucide-react";
import type { ElementType } from "react";
import {
  lazy,
  memo,
  Suspense,
  useCallback,
  useEffect,
  useMemo,
  useState,
} from "react";
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
  type AgentConversationPlanSeedResult,
  type AgentConversationWorkspace,
  type AgentConversationWorkspaceFreshness,
  type AgentConversationRuntimeStatus,
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
import { extractErrorMessage } from "@/lib/errors";
import { withAlpha } from "@/lib/theme-colors";
import type {
  PlanDisplayConversationReference,
  PlanDisplayBodyMode,
  TeamMetadata,
} from "@/components/Ideation/PlanDisplay";
import { useChatStore } from "@/stores/chatStore";
import {
  selectActivePlanId,
  selectActiveExecutionPlanId,
  usePlanStore,
} from "@/stores/planStore";
import {
  useAgentSessionStore,
  type AgentArtifactTab,
  type AgentTaskArtifactMode,
} from "@/stores/agentSessionStore";
import {
  invalidateConversationDataQueries,
  useConversationHistoryWindow,
} from "@/hooks/useChat";
import { ideationKeys } from "@/hooks/useIdeation";
import { taskKeys, useTasks } from "@/hooks/useTasks";
import { useDependencyGraph } from "@/hooks/useDependencyGraph";
import {
  useVerificationStatus,
  verificationStatusKey,
} from "@/hooks/useVerificationStatus";
import { useConfirmation } from "@/hooks/useConfirmation";
import type { Artifact } from "@/types/artifact";
import type {
  IdeationSession,
  TaskProposal,
  VerificationStatus,
} from "@/types/ideation";
import type { Task } from "@/types/task";
import { getStatusCounts, type StatusCounts } from "@/types/status";
import type { DependencyGraphResponse } from "@/api/ideation.types";
import {
  getAgentConversationStoreKey,
  type AgentConversation,
} from "./agentConversations";
import { AgentReviewPanel } from "./AgentReviewPanel";
import { AgentPlanStartPanel } from "./AgentPlanStartPanel";
import {
  PlanLifecycleBanner,
  type PlanLifecycleAction,
  type PlanLifecycleState,
} from "./PlanLifecycleBanner";
import {
  getVisibleIdeationArtifactTabs,
  type IdeationArtifactTab,
} from "./agentArtifactTabs";
import { resolveAttachedIdeationSessionId } from "./attachedIdeationSession";
import type { ProposalDetailEnrichment } from "@/components/Ideation/ProposalDetailSheet";
import { ArtifactLoadingState, EmptyArtifactState } from "./AgentsArtifactEmptyState";
import { AgentPublishPanel } from "./AgentsPublishPanel";
import { shouldShowAgentWorkspacePublishSurface } from "./agentWorkspacePublishState";
import type { AgentPublishFocusRequest } from "./agentPublishFocus";
import type { AgentTaskArtifactFocusRequest } from "./agentTaskArtifactFocus";
import type { AgentTaskRuntimeContextType } from "./agentTaskRuntimeContext";
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
import { useAgentConversationRuntimeStatus } from "./useAgentConversationRuntimeStatus";
import { agentConversationKeys } from "./useProjectAgentConversations";

const EMPTY_PROPOSAL_HIGHLIGHTS = new Set<string>();

function noop() {}

function getProposalCreatedTaskIds(
  proposals: readonly TaskProposal[],
): Set<string> {
  return new Set(
    proposals
      .map((proposal) => proposal.createdTaskId)
      .filter((taskId): taskId is string => Boolean(taskId)),
  );
}

function getVisibleImplementationTasks({
  tasks,
  proposals,
  activeExecutionPlanId,
  sessionId,
}: {
  tasks: readonly Task[];
  proposals: readonly TaskProposal[];
  activeExecutionPlanId: string | null;
  sessionId: string | null;
}): Task[] {
  const activeTasks = tasks.filter(
    (task) =>
      task.archivedAt === null &&
      (sessionId === null || task.ideationSessionId === sessionId),
  );
  const createdTaskIds = getProposalCreatedTaskIds(proposals);
  const proposalCreatedTasks =
    createdTaskIds.size === 0
      ? []
      : activeTasks.filter((task) => createdTaskIds.has(task.id));

  if (activeExecutionPlanId) {
    const activeExecutionPlanTasks = activeTasks.filter(
      (task) => task.executionPlanId === activeExecutionPlanId,
    );
    return activeExecutionPlanTasks.length > 0
      ? activeExecutionPlanTasks
      : proposalCreatedTasks;
  }

  return proposalCreatedTasks;
}

type WorkspaceReviewPassState = Pick<
  AgentWorkspaceReviewContext | StartAgentWorkspaceReviewResult,
  "monitor" | "isCurrent"
>;

function hasPassedWorkspaceReview(
  context: WorkspaceReviewPassState | null,
): boolean {
  const gateStatus = context?.monitor.reviewGateStatus ?? null;
  if (gateStatus) {
    return gateStatus === "passed";
  }
  return Boolean(
    context?.isCurrent && context.monitor.reviewOutcome === "passed",
  );
}

function hasGeneratingConversationRuntime(
  status: AgentConversationRuntimeStatus | null | undefined,
): boolean {
  return Boolean(
    status?.agentStatus === "generating" ||
    status?.items.some((item) => item.agentStatus === "generating"),
  );
}

const LazyTaskGraphView = lazy(() =>
  import("@/components/TaskGraph").then((module) => ({
    default: module.TaskGraphView,
  })),
);
const LazyTaskBoard = lazy(() =>
  import("@/components/tasks/TaskBoard").then((module) => ({
    default: module.TaskBoard,
  })),
);
const LazyAgentsTaskDetailOverlay = lazy(() =>
  import("@/components/agents/task-details/AgentsTaskDetailOverlay").then(
    (module) => ({
      default: module.AgentsTaskDetailOverlay,
    }),
  ),
);
const LazyExportPlanDialog = lazy(() =>
  import("@/components/Ideation/ExportPlanDialog").then((module) => ({
    default: module.ExportPlanDialog,
  })),
);
const LazyPlanDisplay = lazy(() =>
  import("@/components/Ideation/PlanDisplay").then((module) => ({
    default: module.PlanDisplay,
  })),
);
const LazyPlanEditor = lazy(() =>
  import("@/components/Ideation/PlanEditor").then((module) => ({
    default: module.PlanEditor,
  })),
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
const LazyAgentsAutomationPanel = lazy(() =>
  import("@/components/agents/AgentsAutomationPanel").then((module) => ({
    default: module.AgentsAutomationPanel,
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
  { id: "tasks", label: "Tasks", icon: ClipboardList },
];

const REVIEW_TAB = {
  id: "review" as const,
  label: "Review",
  icon: FileText,
};

const AUTOMATION_TAB = {
  id: "automation" as const,
  label: "Automation",
  icon: Workflow,
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
  return Boolean(
    workspace?.publicationPrNumber != null || workspace?.sourcePullRequest,
  );
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
  onOpenPublish?: () => void;
  onTaskModeChange: (mode: AgentTaskArtifactMode) => void;
  onPublishWorkspace: ((conversationId: string) => Promise<void>) | undefined;
  isPublishingWorkspace?: boolean;
  publishFocusRequest?: AgentPublishFocusRequest | null;
  taskFocusRequest?: AgentTaskArtifactFocusRequest | null;
  onOpenAutomation?: (automationId: string) => void;
  onFocusVerificationSession:
    ((parentSessionId: string, childSessionId: string) => void) | undefined;
  onFocusWorkspaceReview?: (conversationId: string) => void;
  onFocusTaskRuntime?: (
    taskId: string,
    contextType: AgentTaskRuntimeContextType
  ) => void;
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
  onOpenPublish,
  onTaskModeChange,
  onPublishWorkspace,
  isPublishingWorkspace = false,
  publishFocusRequest = null,
  taskFocusRequest = null,
  onOpenAutomation,
  onFocusVerificationSession,
  onFocusWorkspaceReview,
  onFocusTaskRuntime,
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
  const conversationQuery = useConversationHistoryWindow(
    conversation?.id ?? null,
    {
      enabled:
        shouldLoadIdeationData &&
        !focusedIdeationSessionId &&
        !!conversation?.id,
      pageSize: 40,
    },
  );
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
  const [displayedVerificationStatus, setDisplayedVerificationStatus] =
    useState<{
      status: VerificationStatus;
      inProgress: boolean;
    } | null>(null);
  const conversationId = conversation?.id ?? workspace?.conversationId ?? null;
  const conversationProjectId =
    conversation?.projectId ?? workspace?.projectId ?? null;
  const canStartPlan = Boolean(
    conversationId &&
    conversationProjectId &&
    (workspace
      ? workspace.mode === "edit" || workspace.mode === "plan"
      : conversation?.contextType === "project"),
  );
  const prReviewConversationId =
    workspace?.mode === "review_pr" ? workspace.conversationId : null;
  const shouldLoadPrReviewContext = Boolean(prReviewConversationId);
  const prReviewContextQuery = useQuery({
    queryKey: agentWorkspaceKeys.prReview(prReviewConversationId ?? ""),
    queryFn: () =>
      chatApi.getAgentWorkspacePrReviewContext(prReviewConversationId!),
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
  const prReviewArtifactId = prReviewContext?.monitor?.reviewArtifactId ?? null;
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
      queryClient.setQueryData(
        agentWorkspaceKeys.workspaceReview(variables.conversationId),
        result,
      );
      void queryClient.invalidateQueries({
        queryKey: agentWorkspaceKeys.workspaceReview(variables.conversationId),
      });
      const reviewConversationId = result.monitor.reviewConversationId;
      if (reviewConversationId) {
        invalidateConversationDataQueries(queryClient, reviewConversationId);
        onFocusWorkspaceReview?.(reviewConversationId);
      }
      const artifactId = result.monitor.reviewArtifactId;
      if (artifactId) {
        void queryClient.invalidateQueries({
          queryKey: ["agents", "artifact", artifactId],
        });
      }
    },
  });
  const startWorkspaceReviewFixerMutation = useMutation({
    mutationFn: ({ conversationId }: { conversationId: string }) =>
      chatApi.startAgentWorkspaceReviewFixer(conversationId),
    onSuccess: (result, variables) => {
      queryClient.setQueryData(
        agentWorkspaceKeys.workspaceReview(variables.conversationId),
        result,
      );
      void queryClient.invalidateQueries({
        queryKey: agentWorkspaceKeys.workspaceReview(variables.conversationId),
      });
      const fixerConversationId =
        result.monitor.reviewFixerConversationId ?? variables.conversationId;
      invalidateConversationDataQueries(queryClient, fixerConversationId);
      if (fixerConversationId !== variables.conversationId) {
        invalidateConversationDataQueries(
          queryClient,
          variables.conversationId,
        );
      }
      const artifactId = result.monitor.reviewArtifactId;
      if (artifactId) {
        void queryClient.invalidateQueries({
          queryKey: ["agents", "artifact", artifactId],
        });
      }
    },
  });
  const [taskArtifactSelectedId, setTaskArtifactSelectedIdState] = useState<
    string | null
  >(() => readSelectedTaskForConversation(conversationId));
  useEffect(() => {
    setDisplayedVerificationStatus(null);
  }, [attachedSessionId]);
  useEffect(() => {
    setTaskArtifactSelectedIdState(
      readSelectedTaskForConversation(conversationId),
    );
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
  const session = sessionData?.session
    ? (sessionData.session as IdeationSession)
    : null;
  const proposals = useMemo<TaskProposal[]>(
    () => (sessionData?.proposals ?? []).map(toTaskProposal),
    [sessionData?.proposals],
  );
  const taskProjectId =
    session?.projectId ??
    conversation?.projectId ??
    workspace?.projectId ??
    null;
  const activePlanSessionId = usePlanStore(selectActivePlanId(taskProjectId ?? ""));
  const projectActiveExecutionPlanId = usePlanStore(
    selectActiveExecutionPlanId(taskProjectId ?? ""),
  );
  const hasForeignActivePlan = Boolean(
    activePlanSessionId && activePlanSessionId !== attachedSessionId,
  );
  const activeExecutionPlanId = hasForeignActivePlan
    ? null
    : projectActiveExecutionPlanId;
  const hasProposalCreatedTasks = useMemo(
    () => proposals.some((proposal) => proposal.createdTaskId != null),
    [proposals],
  );
  const shouldLoadImplementationTasks = Boolean(
    taskProjectId &&
    attachedSessionId &&
    (activeExecutionPlanId ||
      hasProposalCreatedTasks ||
      session?.status === "accepted" ||
      workspace?.linkedPlanBranchId),
  );
  const implementationTasksQuery = useTasks(taskProjectId ?? "", {
    enabled: shouldLoadImplementationTasks,
  });
  const visibleImplementationTasks = useMemo(
    () =>
      getVisibleImplementationTasks({
        tasks: implementationTasksQuery.data ?? [],
        proposals,
        activeExecutionPlanId,
        sessionId: attachedSessionId,
      }),
    [
      activeExecutionPlanId,
      attachedSessionId,
      implementationTasksQuery.data,
      proposals,
    ],
  );
  const implementationTaskCounts = useMemo(
    () => getStatusCounts(visibleImplementationTasks),
    [visibleImplementationTasks],
  );
  const visibleImplementationTaskCount = implementationTaskCounts.total;
  const hasImplementationAttempt = visibleImplementationTaskCount > 0;
  const planArtifactId = shouldLoadIdeationData
    ? (sessionData?.session.planArtifactId ??
      sessionData?.session.inheritedPlanArtifactId ??
      null)
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
  const issueConversationId =
    conversation?.contextType === "project" ? conversation.id : null;
  const automationId = conversation?.automationId ?? null;
  const conversationIssuesQuery =
    useAgentConversationIssues(issueConversationId);
  const hasConversationIssues = hasOpenAgentConversationIssues(
    conversationIssuesQuery.data,
  );
  const availableIdeationTabIds = useMemo(
    () =>
      getVisibleIdeationArtifactTabs({
        hasAttachedIdeationSession: Boolean(sessionData),
        hasPlanArtifact: Boolean(planArtifactId),
        canStartPlan,
        hasVerificationEvidence,
        hasExecutionTasks: hasImplementationAttempt,
      }),
    [
      canStartPlan,
      hasImplementationAttempt,
      hasVerificationEvidence,
      planArtifactId,
      sessionData,
    ],
  );
  const availableArtifactTabIds = useMemo<IdeationArtifactTab[]>(() => {
    const tabs =
      conversation?.contextType === "project" && hasConversationIssues
        ? (["issues", ...availableIdeationTabIds] as IdeationArtifactTab[])
        : availableIdeationTabIds;
    const shouldShowReviewTab =
      Boolean(reviewArtifactId) ||
      Boolean(workspaceReviewContext?.shouldShowTab);
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
      ...ARTIFACT_TABS.filter((tab) =>
        availableArtifactTabIds.includes(tab.id),
      ),
      ...(automationId ? [AUTOMATION_TAB] : []),
      ...(showPullRequestTab ? [PR_TAB] : []),
      ...(showJiraTab ? [JIRA_TAB] : []),
      ...(showLinearTab ? [LINEAR_TAB] : []),
      ...(showGranolaTab ? [GRANOLA_TAB] : []),
      ...(availableArtifactTabIds.includes("review") ? [REVIEW_TAB] : []),
      ...(showPublishTab ? [PUBLISH_TAB] : []),
    ],
    [
      availableArtifactTabIds,
      automationId,
      showGranolaTab,
      showJiraTab,
      showLinearTab,
      showPublishTab,
      showPullRequestTab,
    ],
  );
  const fallbackActiveTab =
    automationId && conversation?.agentMode === "automation"
      ? "automation"
      : workspaceReviewContext?.shouldShowTab || reviewArtifactId
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
  const shouldPreferAutomationOverPlan =
    activeTab === "plan" &&
    automationId &&
    conversation?.agentMode === "automation" &&
    visibleTabs.some((tab) => tab.id === "automation");
  const effectiveActiveTab = shouldPreferAutomationOverPlan
    ? "automation"
    : visibleTabs.some((tab) => tab.id === activeTab)
      ? activeTab
      : fallbackActiveTab;
  const runtimeStatusStoreKey = conversation
    ? getAgentConversationStoreKey(conversation)
    : null;
  const runtimeStatusQuery = useAgentConversationRuntimeStatus(conversationId, {
    enabled: Boolean(conversationId && effectiveActiveTab === "review"),
    mirrorToVisibleChatStatus: false,
    storeKey: runtimeStatusStoreKey,
  });
  const isWorkspaceRuntimeGenerating = hasGeneratingConversationRuntime(
    runtimeStatusQuery.data,
  );
  const isWorkspaceReviewActionPending =
    startWorkspaceReviewMutation.isPending &&
    startWorkspaceReviewMutation.variables?.conversationId === conversationId;
  const isWorkspaceReviewFixIssuesPending =
    startWorkspaceReviewFixerMutation.isPending &&
    startWorkspaceReviewFixerMutation.variables?.conversationId ===
      conversationId;
  const workspaceReviewStartResult = workspaceReviewContextForConversation(
    startWorkspaceReviewMutation.data,
    conversationId,
  );
  const workspaceReviewFixerStartResult = workspaceReviewContextForConversation(
    startWorkspaceReviewFixerMutation.data,
    conversationId,
  );
  const reviewDisplayContext = isWorkspaceReviewActionPending
    ? (workspaceReviewStartResult ?? workspaceReviewContext)
    : isWorkspaceReviewFixIssuesPending
      ? (workspaceReviewFixerStartResult ?? workspaceReviewContext)
      : (workspaceReviewContext ??
        workspaceReviewStartResult ??
        workspaceReviewFixerStartResult);
  const isWorkspaceReviewRunning =
    isWorkspaceReviewActionPending ||
    isWorkspaceReviewFixIssuesPending ||
    reviewDisplayContext?.monitor.status === "reviewing" ||
    reviewDisplayContext?.monitor.reviewGateStatus === "reviewing";
  const workspaceReviewBlocked =
    (isWorkspaceReviewActionPending &&
      Boolean(startWorkspaceReviewMutation.error)) ||
    (isWorkspaceReviewFixIssuesPending &&
      Boolean(startWorkspaceReviewFixerMutation.error)) ||
    reviewDisplayContext?.monitor.status === "blocked" ||
    reviewDisplayContext?.monitor.reviewGateStatus === "blocking" ||
    reviewDisplayContext?.monitor.reviewGateStatus === "failed" ||
    reviewDisplayContext?.monitor.reviewOutcome === "blocking" ||
    reviewDisplayContext?.monitor.reviewOutcome === "run_failed";
  const reviewTabIconColor = (() => {
    if (isWorkspaceReviewRunning) return "var(--accent-primary)";
    if (workspaceReviewBlocked) return "var(--status-error)";
    if (hasPassedWorkspaceReview(reviewDisplayContext))
      return "var(--status-success)";
    if (
      reviewDisplayContext?.isOutdated ||
      reviewDisplayContext?.monitor.reviewGateStatus === "required"
    ) {
      return "var(--status-warning)";
    }
    return null;
  })();
  const reviewTabStatusColor = isWorkspaceReviewRunning
    ? reviewTabIconColor
    : null;
  const shouldLoadVerificationData =
    shouldLoadIdeationData && effectiveActiveTab === "verification";
  const shouldLoadDependencyGraph =
    shouldLoadIdeationData &&
    (effectiveActiveTab === "tasks" ||
      (effectiveActiveTab === "plan" && proposalCount > 0));
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
    shouldLoadVerificationData ? (attachedSessionId ?? undefined) : undefined,
  );
  const dependencyQuery = useDependencyGraph(
    shouldLoadDependencyGraph ? (attachedSessionId ?? "") : "",
  );
  const verificationData =
    attachedSessionId && verificationQuery.data?.sessionId === attachedSessionId
      ? verificationQuery.data
      : null;
  const dependencyGraph =
    attachedSessionId && sessionData ? (dependencyQuery.data ?? null) : null;
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
      queryClient.setQueryData(
        ["agents", "artifact", updatedPlan.id],
        updatedPlan,
      );
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
  const handlePlanSeeded = useCallback(
    (result: AgentConversationPlanSeedResult) => {
      queryClient.setQueryData(
        agentWorkspaceKeys.workspace(result.workspace.conversationId),
        result.workspace,
      );
      queryClient.setQueryData(
        ["agents", "artifact", result.artifact.id],
        result.artifact,
      );
      queryClient.setQueryData(
        ["agents", "session-plan", result.sessionId, result.artifact.id],
        result.artifact,
      );
      queryClient.setQueryData(
        ["agents", "plan-approval", result.sessionId],
        result.artifact,
      );
      void invalidateWorkspaceQueries(
        queryClient,
        result.workspace.conversationId,
      );
      void invalidateConversationDataQueries(
        queryClient,
        result.conversation.id,
      );
      void queryClient.invalidateQueries({
        queryKey: ideationKeys.sessionWithData(result.sessionId),
      });
      if (result.conversation.contextType === "project") {
        void queryClient.invalidateQueries({
          queryKey: agentConversationKeys.project(
            result.conversation.contextId,
          ),
        });
      }
    },
    [queryClient],
  );
  const handleStartReview = useCallback(
    (force: boolean) => {
      if (
        !conversationId ||
        isWorkspaceReviewActionPending ||
        isWorkspaceReviewFixIssuesPending ||
        isWorkspaceRuntimeGenerating
      ) {
        return;
      }
      startWorkspaceReviewMutation.mutate({ conversationId, force });
    },
    [
      conversationId,
      isWorkspaceReviewActionPending,
      isWorkspaceReviewFixIssuesPending,
      isWorkspaceRuntimeGenerating,
      startWorkspaceReviewMutation,
    ],
  );
  const handleFixReviewIssues = useCallback(() => {
    if (
      !conversationId ||
      isWorkspaceReviewActionPending ||
      isWorkspaceReviewFixIssuesPending ||
      isWorkspaceRuntimeGenerating ||
      isPublishingWorkspace
    ) {
      return;
    }
    startWorkspaceReviewFixerMutation.mutate({ conversationId });
  }, [
    conversationId,
    isPublishingWorkspace,
    isWorkspaceReviewActionPending,
    isWorkspaceReviewFixIssuesPending,
    isWorkspaceRuntimeGenerating,
    startWorkspaceReviewFixerMutation,
  ]);
  const handleFocusWorkspaceReview = useCallback(() => {
    const reviewConversationId =
      reviewDisplayContext?.monitor.reviewConversationId ?? null;
    if (reviewConversationId) {
      onFocusWorkspaceReview?.(reviewConversationId);
    }
  }, [
    onFocusWorkspaceReview,
    reviewDisplayContext?.monitor.reviewConversationId,
  ]);
  const handleOpenReview = useCallback(() => {
    onTabChange("review");
    handleFocusWorkspaceReview();
  }, [handleFocusWorkspaceReview, onTabChange]);
  const handleOpenPublish = useCallback(() => {
    if (onOpenPublish) {
      onOpenPublish();
      return;
    }
    onTabChange("publish");
  }, [onOpenPublish, onTabChange]);

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
            const count = id === "tasks" ? visibleImplementationTaskCount : 0;

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
              iconColor = reviewTabIconColor ?? undefined;
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
                  if (id === "review") {
                    handleOpenReview();
                    return;
                  }
                  onTabChange(id);
                }}
                className={cn(
                  "relative flex h-full self-stretch items-center gap-1.5 bg-transparent px-3 text-[0.75rem] font-medium transition-colors duration-150 rounded-none shadow-none outline-none ring-0 focus:ring-0 focus:outline-none focus-visible:outline-none focus-visible:ring-0 appearance-none",
                  id === "tasks" ? "hidden xl:flex" : "",
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
                  className={cn(
                    "w-4 h-4 shrink-0",
                    iconPulse ? "animate-pulse" : "",
                  )}
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
                      color: isActive
                        ? "var(--accent-primary)"
                        : "var(--text-muted)",
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
                      color:
                        taskMode === "graph"
                          ? "var(--accent-primary)"
                          : "var(--text-muted)",
                      background:
                        taskMode === "graph"
                          ? "var(--accent-muted)"
                          : "transparent",
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
                      color:
                        taskMode === "kanban"
                          ? "var(--accent-primary)"
                          : "var(--text-muted)",
                      background:
                        taskMode === "kanban"
                          ? "var(--accent-muted)"
                          : "transparent",
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
          automationId={automationId}
          {...(onOpenAutomation ? { onOpenAutomation } : {})}
          projectBaseBranch={projectBaseBranch}
          isLoading={conversationQuery.isLoading || sessionQuery.isLoading}
          attachedSessionId={attachedSessionId}
          projectId={conversationProjectId}
          canStartPlan={canStartPlan}
          session={session}
          sessionTitle={sessionData?.session.title ?? null}
          taskMode={taskMode}
          reviewArtifact={reviewArtifact}
          reviewContext={workspaceReviewContext}
          reviewStartResult={workspaceReviewStartResult}
          reviewStartError={
            isWorkspaceReviewActionPending
              ? startWorkspaceReviewMutation.error
              : isWorkspaceReviewFixIssuesPending
                ? startWorkspaceReviewFixerMutation.error
                : null
          }
          isReviewLoading={
            Boolean(reviewArtifactId) &&
            !reviewArtifact &&
            reviewArtifactQuery.isFetching
          }
          isReviewActionPending={isWorkspaceReviewActionPending}
          isFixIssuesActionPending={isWorkspaceReviewFixIssuesPending}
          isWorkspaceRuntimeGenerating={isWorkspaceRuntimeGenerating}
          onStartReview={handleStartReview}
          onFixIssues={handleFixReviewIssues}
          planArtifact={planArtifact}
          isPlanLoading={isPlanHydrating}
          onPlanUpdated={handlePlanUpdated}
          onPlanSeeded={handlePlanSeeded}
          dependencyGraph={dependencyGraph}
          proposals={proposals}
          implementationTaskCounts={implementationTaskCounts}
          hasImplementationAttempt={hasImplementationAttempt}
          onPublishWorkspace={onPublishWorkspace}
          isPublishingWorkspace={isPublishingWorkspace}
          publishFocusRequest={publishFocusRequest}
          onFocusVerificationSession={onFocusVerificationSession}
          onDisplayedVerificationStatusChange={setDisplayedVerificationStatus}
          {...(onFocusTaskRuntime ? { onFocusTaskRuntime } : {})}
          verificationState={verificationState}
          verificationInProgress={verificationInProgress}
          onOpenReview={handleOpenReview}
          onOpenPublish={handleOpenPublish}
          onOpenVerification={() => onTabChange("verification")}
          onOpenTasks={() => onTabChange("tasks")}
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
  automationId: string | null;
  onOpenAutomation?: (automationId: string) => void;
  projectBaseBranch: string | null;
  isLoading: boolean;
  attachedSessionId: string | null;
  projectId: string | null;
  canStartPlan: boolean;
  session: IdeationSession | null;
  sessionTitle: string | null;
  taskMode: AgentTaskArtifactMode;
  reviewArtifact: Artifact | null;
  reviewContext: AgentWorkspaceReviewContext | null;
  reviewStartResult: StartAgentWorkspaceReviewResult | null;
  reviewStartError: Error | null;
  isReviewLoading: boolean;
  isReviewActionPending: boolean;
  isFixIssuesActionPending: boolean;
  isWorkspaceRuntimeGenerating: boolean;
  onStartReview: (force: boolean) => void;
  onFixIssues: () => void;
  planArtifact: Artifact | null;
  isPlanLoading: boolean;
  onPlanUpdated: (updatedPlan: Artifact) => void;
  onPlanSeeded: (result: AgentConversationPlanSeedResult) => void;
  dependencyGraph: DependencyGraphResponse | null;
  proposals: TaskProposal[];
  implementationTaskCounts: StatusCounts;
  hasImplementationAttempt: boolean;
  onPublishWorkspace: ((conversationId: string) => Promise<void>) | undefined;
  isPublishingWorkspace: boolean;
  publishFocusRequest: AgentPublishFocusRequest | null;
  onFocusVerificationSession:
    ((parentSessionId: string, childSessionId: string) => void) | undefined;
  onDisplayedVerificationStatusChange: (
    status: {
      status: VerificationStatus;
      inProgress: boolean;
    } | null,
  ) => void;
  onFocusTaskRuntime?: (
    taskId: string,
    contextType: AgentTaskRuntimeContextType
  ) => void;
  verificationState: VerificationStatus | null;
  verificationInProgress: boolean;
  onOpenReview: () => void;
  onOpenPublish: () => void;
  onOpenVerification: () => void;
  onOpenTasks: () => void;
  taskArtifactSelectedId: string | null;
  onTaskArtifactSelectedIdChange: (id: string | null) => void;
};

function ArtifactContent({
  activeTab,
  workspace,
  conversationId,
  activeWorkspaceFreshness,
  conversationTitle,
  automationId,
  onOpenAutomation,
  projectBaseBranch,
  isLoading,
  attachedSessionId,
  projectId,
  canStartPlan,
  session,
  sessionTitle,
  taskMode,
  reviewArtifact,
  reviewContext,
  reviewStartResult,
  reviewStartError,
  isReviewLoading,
  isReviewActionPending,
  isFixIssuesActionPending,
  isWorkspaceRuntimeGenerating,
  onStartReview,
  onFixIssues,
  planArtifact,
  isPlanLoading,
  onPlanUpdated,
  onPlanSeeded,
  dependencyGraph,
  proposals,
  implementationTaskCounts,
  hasImplementationAttempt,
  onPublishWorkspace,
  isPublishingWorkspace,
  publishFocusRequest,
  onFocusVerificationSession: _onFocusVerificationSession,
  onDisplayedVerificationStatusChange,
  onFocusTaskRuntime,
  verificationState,
  verificationInProgress,
  onOpenReview,
  onOpenPublish,
  onOpenVerification,
  onOpenTasks,
  taskArtifactSelectedId,
  onTaskArtifactSelectedIdChange,
}: ArtifactContentProps) {
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

  if (activeTab === "automation" && automationId) {
    return (
      <Suspense
        fallback={
          <EmptyArtifactState
            title="Loading automation..."
            testId="agents-automation-panel-loading"
          />
        }
      >
        <LazyAgentsAutomationPanel
          automationId={automationId}
          conversationTitle={conversationTitle}
          {...(onOpenAutomation ? { onOpenAutomation } : {})}
        />
      </Suspense>
    );
  }

  if (activeTab === "publish") {
    return (
      <AgentPublishPanel
        workspace={workspace}
        conversationTitle={conversationTitle}
        projectBaseBranch={projectBaseBranch}
        onPublishWorkspace={onPublishWorkspace}
        isPublishingWorkspace={isPublishingWorkspace}
        publishFocusRequest={publishFocusRequest}
        reviewContext={reviewContext}
        onOpenReview={onOpenReview}
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
      <Suspense fallback={<ArtifactLoadingState title="Loading pull request..." />}>
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
        isFixIssuesActionPending={isFixIssuesActionPending}
        isWorkspaceRuntimeGenerating={isWorkspaceRuntimeGenerating}
        isPublishingWorkspace={isPublishingWorkspace}
        onOpenPublish={onOpenPublish}
        onStartReview={onStartReview}
        onFixIssues={onFixIssues}
      />
    );
  }

  if (activeTab === "plan") {
    if ((isLoading && attachedSessionId) || isPlanLoading) {
      return <EmptyArtifactState title="Loading attached run..." />;
    }
    if (!planArtifact && canStartPlan && conversationId && projectId) {
      return (
        <AgentPlanStartPanel
          conversationId={conversationId}
          projectId={projectId}
          onPlanSeeded={onPlanSeeded}
        />
      );
    }
    if (!attachedSessionId) {
      return (
        <EmptyArtifactState
          title="No ideation run attached"
          detail="Start ideation from this agent chat to populate plan, verification, proposals, and tasks here."
        />
      );
    }
    return (
      <AgentPlanPanel
        workspace={workspace}
        activeWorkspaceFreshness={activeWorkspaceFreshness}
        session={session}
        sessionTitle={sessionTitle}
        planArtifact={planArtifact}
        isPlanLoading={isPlanLoading}
        proposals={proposals}
        dependencyGraph={dependencyGraph}
        implementationTaskCounts={implementationTaskCounts}
        hasImplementationAttempt={hasImplementationAttempt}
        onPlanUpdated={onPlanUpdated}
        verificationState={verificationState}
        verificationInProgress={verificationInProgress}
        onOpenVerification={onOpenVerification}
        onOpenTasks={onOpenTasks}
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

  if (activeTab === "verification") {
    if (!session) {
      return <EmptyArtifactState title="No verification data yet" />;
    }
    return (
      <div className="flex h-full min-h-0 flex-col">
        <Suspense
          fallback={<EmptyArtifactState title="Loading verification..." />}
        >
          <LazyVerificationPanel
            session={session}
            onDisplayedVerificationChildChange={
              handleDisplayedVerificationChildChange
            }
            onDisplayedVerificationStatusChange={
              handleDisplayedVerificationStatusChange
            }
          />
        </Suspense>
      </div>
    );
  }

  return (
    <TaskArtifactSurface
      projectId={projectId}
      sessionId={attachedSessionId}
      mode={taskMode}
      selectedTaskId={taskArtifactSelectedId}
      onSelectedTaskIdChange={onTaskArtifactSelectedIdChange}
      {...(onFocusTaskRuntime ? { onFocusTaskRuntime } : {})}
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
  dependencyGraph,
  implementationTaskCounts,
  hasImplementationAttempt,
  onPlanUpdated,
  verificationState,
  verificationInProgress,
  onOpenVerification,
  onOpenTasks,
}: {
  workspace: AgentConversationWorkspace | null;
  activeWorkspaceFreshness: AgentConversationWorkspaceFreshness | undefined;
  session: IdeationSession | null;
  sessionTitle: string | null;
  planArtifact: Artifact | null;
  isPlanLoading: boolean;
  proposals: TaskProposal[];
  dependencyGraph: DependencyGraphResponse | null;
  implementationTaskCounts: StatusCounts;
  hasImplementationAttempt: boolean;
  onPlanUpdated: (updatedPlan: Artifact) => void;
  verificationState: VerificationStatus | null;
  verificationInProgress: boolean;
  onOpenVerification: () => void;
  onOpenTasks: () => void;
}) {
  const [isEditing, setIsEditing] = useState(false);
  const [isPlanExpanded, setIsPlanExpanded] = useState(true);
  const [planBodyMode, setPlanBodyMode] = useState<PlanDisplayBodyMode>("plan");
  const [exportDialogOpen, setExportDialogOpen] = useState(false);
  const [isApprovingPlan, setIsApprovingPlan] = useState(false);
  const [isStartingPlanVerification, setIsStartingPlanVerification] =
    useState(false);
  const [isImplementingPlanDirectly, setIsImplementingPlanDirectly] =
    useState(false);
  const [viewingProposalId, setViewingProposalId] = useState<string | null>(
    null,
  );
  const [viewingEnrichment, setViewingEnrichment] = useState<
    ProposalDetailEnrichment | undefined
  >(undefined);
  const queryClient = useQueryClient();
  const { confirm, confirmationDialogProps, ConfirmationDialog } =
    useConfirmation();
  const setFocusedAgentProject = useAgentSessionStore(
    (s) => s.setFocusedProject,
  );
  const clearAgentSelection = useAgentSessionStore((s) => s.clearSelection);
  const setStartConversationDraft = useAgentSessionStore(
    (s) => s.setStartConversationDraft,
  );
  const setActiveConversation = useChatStore((s) => s.setActiveConversation);
  const loadActivePlan = usePlanStore((s) => s.loadActivePlan);

  useEffect(() => {
    setIsEditing(false);
    setIsPlanExpanded(true);
    setPlanBodyMode("plan");
    setViewingProposalId(null);
    setViewingEnrichment(undefined);
  }, [planArtifact?.id, planArtifact?.metadata.version, session?.id]);

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
  const criticalPathSet = useMemo(
    () => new Set(dependencyGraph?.criticalPath ?? []),
    [dependencyGraph?.criticalPath],
  );
  const viewingProposal = viewingProposalId
    ? (proposals.find((proposal) => proposal.id === viewingProposalId) ?? null)
    : null;
  const linkedProposalsCount = useMemo(
    () =>
      planArtifact
        ? proposals.filter(
            (proposal) => proposal.planArtifactId === planArtifact.id,
          ).length
        : 0,
    [planArtifact, proposals],
  );
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
  const restartImplementationMutation = useMutation({
    mutationFn: (sessionId: string) =>
      ideationApi.sessions.restartImplementation(sessionId),
  });

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

      await chatApi.sendAgentMessage(
        "ideation",
        session.id,
        PLAN_TO_PROPOSALS_REQUEST,
      );
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
    ? (planArtifact?.planApproval?.status ?? "draft")
    : undefined;
  const planReferenceStatus =
    planArtifact?.planApproval?.status ??
    (session?.status === "accepted"
      ? "accepted"
      : isPlanningSession
        ? "draft"
        : undefined);
  const planReferenceSessionId = session?.id ?? null;
  const planReferenceProjectId =
    session?.projectId ?? workspace?.projectId ?? null;
  const isPlanApproved = planApprovalStatus === "approved";
  const canShowPlanModeControls =
    workspace?.mode === "plan" &&
    activeWorkspaceFreshness?.hasUncommittedChanges !== true;
  const canApprovePlan =
    canShowPlanModeControls &&
    isOwnedCurrentPlan &&
    planApprovalStatus === "draft";
  const canShowApprovedPlanActions =
    canShowPlanModeControls && !isImplementingPlanDirectly;
  const isPlanVerificationSatisfied =
    verificationState === "verified" ||
    verificationState === "imported_verified";
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
  const primaryPlanAction = planComplexityQuery.data?.recommendedAction;
  const isAcceptedPlan = session?.status === "accepted";
  const canRestartImplementation = Boolean(
    isAcceptedPlan && implementationTaskCounts.total > 0 && session?.id,
  );
  const workspaceConversationId = workspace?.conversationId ?? null;

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
      toast.error(
        err instanceof Error ? err.message : "Failed to approve plan",
      );
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
      toast.error(
        err instanceof Error ? err.message : "Failed to start implementation",
      );
    } finally {
      setIsImplementingPlanDirectly(false);
    }
  }, [canImplementDirectly, queryClient, session, workspace]);

  const handleStartNewConversationWithPlan = useCallback(
    (reference: PlanDisplayConversationReference) => {
      if (!planReferenceProjectId || !planReferenceSessionId) {
        return;
      }

      setStartConversationDraft({
        projectId: planReferenceProjectId,
        content: "",
        mode: "edit",
        composerArtifactReferences: [
          {
            kind: "plan",
            artifactId: reference.artifactId,
            title: reference.title,
            sessionId: planReferenceSessionId,
            version: reference.version,
            ...(planReferenceStatus ? { status: planReferenceStatus } : {}),
          },
        ],
      });
      setFocusedAgentProject(planReferenceProjectId);
      clearAgentSelection();
      setActiveConversation(`project:${planReferenceProjectId}`, null);
    },
    [
      clearAgentSelection,
      planReferenceProjectId,
      planReferenceSessionId,
      planReferenceStatus,
      setActiveConversation,
      setFocusedAgentProject,
      setStartConversationDraft,
    ],
  );

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
        queryClient.invalidateQueries({
          queryKey: verificationStatusKey(session.id),
        }),
        queryClient.invalidateQueries({
          queryKey: ideationKeys.sessionWithData(session.id),
        }),
        queryClient.invalidateQueries({ queryKey: ideationKeys.sessions() }),
      ]);
      onOpenVerification();
      toast.success("Plan verification started");
    } catch (err) {
      console.error("Failed to start plan verification:", err);
      toast.error(
        err instanceof Error
          ? err.message
          : "Failed to start plan verification",
      );
    } finally {
      setIsStartingPlanVerification(false);
    }
  }, [
    canVerifyPlan,
    onOpenVerification,
    queryClient,
    session,
    verificationInProgress,
  ]);

  const handleRestartImplementation = useCallback(() => {
    if (!session || !canRestartImplementation) {
      return;
    }

    void confirm({
      title: "Restart implementation?",
      description:
        "Running work will be stopped, the current task attempt will be archived, and implementation will start again from freshly created tasks.",
      confirmText: "Restart Implementation",
      pendingText: "Restarting...",
      variant: "destructive",
      onConfirm: async () => {
        try {
          const result = await restartImplementationMutation.mutateAsync(
            session.id,
          );
          await Promise.all([
            queryClient.invalidateQueries({
              queryKey: ideationKeys.sessionWithData(session.id),
            }),
            queryClient.invalidateQueries({
              queryKey: ideationKeys.sessions(),
            }),
            queryClient.invalidateQueries({ queryKey: taskKeys.lists() }),
            ...(workspaceConversationId
              ? [
                  invalidateWorkspaceQueries(
                    queryClient,
                    workspaceConversationId,
                  ),
                ]
              : []),
          ]);
          await loadActivePlan(session.projectId);
          toast.success(
            `Implementation restarted with ${result.createdTaskIds.length} task${
              result.createdTaskIds.length === 1 ? "" : "s"
            }`,
          );
        } catch (err) {
          toast.error(
            extractErrorMessage(err, "Failed to restart implementation"),
          );
          throw err;
        }
      },
    });
  }, [
    canRestartImplementation,
    confirm,
    loadActivePlan,
    queryClient,
    restartImplementationMutation,
    session,
    workspaceConversationId,
  ]);

  const planLifecycleState = useMemo<PlanLifecycleState | null>(() => {
    if (!planArtifact) {
      return null;
    }
    if (hasImplementationAttempt) {
      return "accepted";
    }
    if (isPlanApproved) {
      return "approved";
    }
    return "needs_approval";
  }, [hasImplementationAttempt, isPlanApproved, planArtifact]);
  const showCreateProposalsLifecycleAction = Boolean(
    canCreateProposals && linkedProposalsCount === 0,
  );
  const planLifecycleActions = useMemo<PlanLifecycleAction[]>(() => {
    if (!planLifecycleState || planLifecycleState === "accepted") {
      return [];
    }

    const actions: PlanLifecycleAction[] = [];
    const verifyPending =
      isStartingPlanVerification || verificationInProgress;
    const verifyAction = canVerifyPlan
      ? ({
          key: "verify",
          label: verifyPending ? "Verifying..." : "Verify Plan",
          onClick: () => {
            void handleVerifyPlan();
          },
          icon: ShieldCheck,
          disabled: isPlanRecommendationPending,
          loading: verifyPending,
          testId: "plan-lifecycle-verify-button",
        } satisfies PlanLifecycleAction)
      : null;

    if (planLifecycleState === "needs_approval") {
      if (canApprovePlan) {
        actions.push({
          key: "approve",
          label: isApprovingPlan ? "Approving..." : "Approve Plan",
          onClick: () => {
            void handleApprovePlan();
          },
          icon: Sparkles,
          loading: isApprovingPlan,
          primary: true,
          testId: "plan-lifecycle-approve-button",
        });
      }
      if (verifyAction) {
        actions.push(verifyAction);
      }
      return actions;
    }

    const createAction: PlanLifecycleAction | null = showCreateProposalsLifecycleAction
      ? ({
          key: "create-proposals",
          label: "Create Proposals",
          onClick: () => {
            void handleCreateProposals();
          },
          icon: ListPlus,
          disabled: isPlanRecommendationPending,
          primary:
            !isPlanRecommendationPending &&
            (primaryPlanAction === "create_proposals" ||
              (!canImplementDirectly && showCreateProposalsLifecycleAction)),
          testId: "plan-lifecycle-create-proposals-button",
        } satisfies PlanLifecycleAction)
      : null;
    const implementAction: PlanLifecycleAction | null = canImplementDirectly
      ? ({
          key: "implement-directly",
          label: isImplementingPlanDirectly ? "Starting..." : "Implement Directly",
          onClick: () => {
            void handleImplementDirectly();
          },
          icon: Rocket,
          disabled: isPlanRecommendationPending,
          loading: isImplementingPlanDirectly,
          primary:
            !isPlanRecommendationPending &&
            (primaryPlanAction === "implement_directly" ||
              (canImplementDirectly && !showCreateProposalsLifecycleAction)),
          testId: "plan-lifecycle-implement-directly-button",
        } satisfies PlanLifecycleAction)
      : null;
    const nextStepActions =
      primaryPlanAction === "implement_directly"
        ? [implementAction, createAction]
        : [createAction, implementAction];

    for (const action of nextStepActions) {
      if (action) {
        actions.push(action);
      }
    }
    if (verifyAction) {
      actions.push(verifyAction);
    }
    return actions;
  }, [
    canApprovePlan,
    canImplementDirectly,
    canVerifyPlan,
    handleApprovePlan,
    handleCreateProposals,
    handleImplementDirectly,
    handleVerifyPlan,
    isApprovingPlan,
    isImplementingPlanDirectly,
    isPlanRecommendationPending,
    isStartingPlanVerification,
    planLifecycleState,
    primaryPlanAction,
    showCreateProposalsLifecycleAction,
    verificationInProgress,
  ]);
  const planLifecycleDescription =
    planLifecycleState === "accepted"
      ? "Implementation work is attached to this plan."
      : planLifecycleState === "approved"
        ? (planActionHint ??
          "Choose the next step for this approved plan.")
        : "Approve this plan before creating proposals or implementation work.";
  const planLifecycleTitle =
    planLifecycleState === "needs_approval"
      ? "Plan needs approval"
      : planLifecycleState === "approved"
        ? "Plan approved"
        : "Plan accepted";

  if (isPlanLoading) {
    return <EmptyArtifactState title="Loading plan..." />;
  }

  return (
    <div className="min-h-full px-4 pb-4 pt-4">
      {planArtifact ? (
        isEditing ? (
          <Suspense
            fallback={<EmptyArtifactState title="Loading plan editor..." />}
          >
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
          <>
            {planLifecycleState && (
              <PlanLifecycleBanner
                state={planLifecycleState}
                title={planLifecycleTitle}
                description={planLifecycleDescription}
                actions={planLifecycleActions}
                {...(planLifecycleState === "accepted" && {
                  counts: implementationTaskCounts,
                  acceptedAt: session?.convertedAt ?? null,
                  onViewWork: onOpenTasks,
                })}
                {...(canRestartImplementation && {
                  onRestartImplementation: handleRestartImplementation,
                  canRestartImplementation,
                  isRestartingImplementation:
                    restartImplementationMutation.isPending,
                })}
              />
            )}
            <Suspense fallback={<EmptyArtifactState title="Loading plan..." />}>
              <LazyPlanDisplay
                plan={planArtifact}
                linkedProposalsCount={linkedProposalsCount}
                bodyMode={planBodyMode}
                hideBody={planBodyMode === "proposals"}
                onBodyModeChange={setPlanBodyMode}
                onEdit={() => setIsEditing(true)}
                onExport={() => setExportDialogOpen(true)}
                {...(planReferenceSessionId && {
                  onStartNewConversationWithPlan:
                    handleStartNewConversationWithPlan,
                })}
                isExpanded={isPlanExpanded}
                onExpandedChange={setIsPlanExpanded}
                chromeless
                {...(teamMetadata !== undefined && { teamMetadata })}
              />
            </Suspense>
            {planBodyMode === "proposals" &&
              session &&
              proposals.length > 0 && (
                <>
                  <Suspense
                    fallback={
                      <EmptyArtifactState title="Loading proposals..." />
                    }
                  >
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
                      {...(viewingProposalId != null && {
                        selectedProposalId: viewingProposalId,
                      })}
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
                        {...(viewingEnrichment !== undefined && {
                          enrichment: viewingEnrichment,
                        })}
                        isReadOnly
                        onClose={handleCloseProposalDetail}
                      />
                    </Suspense>
                  )}
                </>
              )}
            <ConfirmationDialog {...confirmationDialogProps} />
          </>
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
  onFocusTaskRuntime,
}: {
  projectId: string | null;
  sessionId: string;
  mode: AgentTaskArtifactMode;
  selectedTaskId: string | null;
  onSelectedTaskIdChange: (id: string | null) => void;
  onFocusTaskRuntime?: (
    taskId: string,
    contextType: AgentTaskRuntimeContextType
  ) => void;
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
        {...(onFocusTaskRuntime ? { onFocusTaskRuntime } : {})}
      />
    </Suspense>
  ) : null;

  if (mode === "kanban") {
    return (
      <div className="relative h-full min-h-[520px] overflow-hidden bg-[var(--bg-base)]">
        <Suspense
          fallback={<EmptyArtifactState title="Loading task board..." />}
        >
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
