import {
  AlertCircle,
  FileText,
  GitPullRequestArrow,
  LayoutGrid,
  ListPlus,
  Network,
  Pause,
  Play,
  Rocket,
  ClipboardList,
  ScrollText,
  ShieldCheck,
  Sparkles,
  Square,
  Ticket,
  UserRound,
  UsersRound,
  Workflow,
  X,
} from "lucide-react";
import type { ElementType } from "react";
import { useIsRemoteEnvironment } from "@/hooks/useActiveEnvironment";
import { RemoteHostOnlyNotice } from "@/components/remote/RemoteHostOnlyNotice";
import {
  memo,
  Suspense,
  useCallback,
  useEffect,
  useId,
  useMemo,
  useRef,
  useState,
} from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";

import { artifactApi, RemotePlanIntentError } from "@/api/artifact";
import { lazyWithRetry } from "@/lib/lazy-with-retry";
import { atlassianApi } from "@/api/atlassian";
import { clickupApi } from "@/api/clickup";
import { granolaApi } from "@/api/granola";
import { linearApi } from "@/api/linear";
import { ticketingApi } from "@/api/ticketing";
import {
  ideationApi,
  toTaskProposal,
  type VerificationStatusResponse,
} from "@/api/ideation";
import { tasksApi } from "@/api/tasks";
import { verificationApi } from "@/api/verification";
import {
  chatApi,
  type AgentConversationPlanSeedResult,
  type AgentConversationWorkspaceMode,
  type AgentConversationWorkspace,
  type AgentConversationWorkspaceFreshness,
  type AgentConversationRuntimeStatus,
  type AgentWorkspacePrReviewContext,
  type AgentWorkspaceReviewContext,
  type AgentWorkspaceReviewStartConfirmation,
  type StartAgentWorkspaceReviewFixerResult,
  type StartAgentWorkspaceReviewResult,
} from "@/api/chat";
import { Button } from "@/components/ui/button";
import {
  ContextMenu,
  ContextMenuContent,
  ContextMenuItem,
  ContextMenuTrigger,
} from "@/components/ui/context-menu";
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
} from "@/components/Ideation/PlanDisplay";
import {
  planBundlePanelId,
  planBundleTabId,
} from "@/components/Ideation/planBundleTabIds";
import { useChatStore } from "@/stores/chatStore";
import { useAgentGate } from "@/hooks/useAgentGate";
import {
  selectActivePlanId,
  selectActiveExecutionPlanId,
  usePlanStore,
} from "@/stores/planStore";
import {
  useAgentSessionStore,
  type AgentArtifactTab,
  type AgentRuntimeSelection,
  type AgentTaskArtifactMode,
} from "@/stores/agentSessionStore";
import {
  invalidateConversationDataQueries,
  useConversationHistoryWindow,
} from "@/hooks/useChat";
import { ideationKeys } from "@/hooks/useIdeation";
import { useIdeationSettings } from "@/hooks/useIdeationSettings";
import { useReviewSettings } from "@/hooks/useReviewSettings";
import { useAgentModels } from "@/hooks/useAgentModels";
import { useFeatureFlags } from "@/hooks/useFeatureFlags";
import { ticketingKeys } from "@/hooks/useTicketing";
import {
  taskKeys,
  useSessionTaskHistoryAvailability,
  useTasks,
} from "@/hooks/useTasks";
import { useDependencyGraph } from "@/hooks/useDependencyGraph";
import { validateDependencyGraph } from "@/hooks/useDependencyGraphComplete";
import {
  useVerificationStatus,
  verificationStatusKey,
} from "@/hooks/useVerificationStatus";
import { useAutomationDetail } from "@/hooks/useAutomations";
import { useConfirmation } from "@/hooks/useConfirmation";
import type { Artifact } from "@/types/artifact";
import type { IdeationSession, TaskProposal } from "@/types/ideation";
import type { Task } from "@/types/task";
import {
  getStatusCounts,
  type InternalStatus,
  type StatusCounts,
} from "@/types/status";
import type { DependencyGraphResponse } from "@/api/ideation.types";
import {
  getAgentConversationStoreKey,
  type AgentConversation,
} from "./agentConversations";
import { AgentReviewPanel } from "./AgentReviewPanel";
import { AgentsTeamPanel } from "./AgentsTeamPanel";
import {
  hasWorkspaceReviewPublishAuthorization,
  isWorkspaceReviewApprovedAnyway,
  isWorkspaceReviewBlockingPublish,
} from "./workspaceReviewAuthorization";
import {
  AgentsArtifactTabCustomizer,
  type AgentArtifactTabCustomizerItem,
} from "./AgentsArtifactTabCustomizer";
import { AgentPlanStartPanel } from "./AgentPlanStartPanel";
import { isPersonaArtifactConversation } from "./personaArtifactTab";
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
import {
  ArtifactLoadingState,
  EmptyArtifactState,
} from "./AgentsArtifactEmptyState";
import { AgentPublishPanel } from "./AgentsPublishPanel";
import { AgentWorkspaceToolbar } from "./AgentWorkspaceToolbar";
import {
  getAgentWorkspaceReviewActionBlocker,
  hasPublishedWorkspacePr,
  shouldShowAgentWorkspacePublishSurface,
} from "./agentWorkspacePublishState";
import type { AgentPublishFocusRequest } from "./agentPublishFocus";
import type {
  AgentPublishSubTab,
  AgentPublishSubTabRequest,
} from "./agentPublishSubTab";
import type { AgentWorkspacePublishAttempt } from "./useAgentWorkspacePublisher";
import type { AgentTaskArtifactFocusRequest } from "./agentTaskArtifactFocus";
import type { AgentTaskRuntimeContextType } from "./agentTaskRuntimeContext";
import type {
  AgentsChatFocus,
  AutomationRunFocusOptions,
  FocusedArtifactIdeationSession,
} from "./agentChatFocus";
import {
  AGENT_WORKSPACE_STALE_MS,
  agentWorkspaceKeys,
  invalidateWorkspaceQueries,
  isLocalWorkspaceReviewModeEligible,
  prReviewContextForConversation,
  refreshWorkspaceReviewContext,
  resolveWorkspaceReviewOwnerConversationId,
  workspaceReviewContextForConversation,
} from "./agentWorkspaceQueries";
import {
  hasOpenAgentConversationIssues,
  useAgentConversationIssues,
} from "./agentConversationIssueQueries";
import { agentGranolaNoteKeys } from "./agentGranolaNoteQueries";
import { agentJiraIssueKeys } from "./agentJiraIssueQueries";
import { agentLinearIssueKeys } from "./agentLinearIssueQueries";
import {
  buildPlanActionHint,
  isPlanRecommendationCheckPending,
} from "./agentPlanModeActions";
import {
  activateAgentPlanProposals,
  PlanContinuationCommittedError,
} from "./agentPlanProposalActivation";
import {
  implementAgentPlanDirectly,
  type DirectImplementationActivationSnapshot,
} from "./implementAgentPlanDirectly";
import { materializeWorkspaceRuntimeSelection } from "./agentPlanRuntime";
import { useApprovedPlanContinuation } from "./useApprovedPlanContinuation";
import { ArtifactSelectionProvider } from "./artifact-selection/ArtifactSelectionProvider";
import { stageComposerExcerptReference } from "./artifact-selection/composerExcerptBridge";
import { useAgentConversationRuntimeStatus } from "./useAgentConversationRuntimeStatus";
import { useWorkspaceReviewActions } from "./useWorkspaceReviewActions";
import { agentConversationKeys } from "./useProjectAgentConversations";
import {
  getAutomationConversationTabPolicy,
  type AutomationConversationPolicyTab,
} from "@/components/automations/automationConversationTabPolicy";
import { isAutomationRunComposerReadOnly } from "@/components/automations/automationRunView";
import {
  deriveTasksSurfaceCapabilities,
  type TasksSurfaceCapabilities,
} from "./tasksSurfaceCapabilities";

const EMPTY_PROPOSAL_HIGHLIGHTS = new Set<string>();
const PLAN_CONTROL_RUNNING_STATUSES = new Set<InternalStatus>([
  "executing",
  "qa_refining",
  "qa_testing",
  "reviewing",
  "re_executing",
  "merging",
  "pending_merge",
]);

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

function getPlanRuntimeControlCounts(tasks: readonly Task[]): {
  paused: number;
  running: number;
} {
  let paused = 0;
  let running = 0;
  for (const task of tasks) {
    if (task.internalStatus === "paused") {
      paused += 1;
    } else if (PLAN_CONTROL_RUNNING_STATUSES.has(task.internalStatus)) {
      running += 1;
    }
  }
  return { paused, running };
}

function hasGeneratingConversationRuntime(
  status: AgentConversationRuntimeStatus | null | undefined,
): boolean {
  return Boolean(
    status?.agentStatus === "generating" ||
    status?.items.some((item) => item.agentStatus === "generating"),
  );
}

const LazyTaskGraphView = lazyWithRetry(() =>
  import("@/components/TaskGraph").then((module) => ({
    default: module.TaskGraphView,
  })),
);
const LazyTaskBoard = lazyWithRetry(() =>
  import("@/components/tasks/TaskBoard").then((module) => ({
    default: module.TaskBoard,
  })),
);
const LazyAgentsTaskDetailOverlay = lazyWithRetry(() =>
  import("@/components/agents/task-details/AgentsTaskDetailOverlay").then(
    (module) => ({
      default: module.AgentsTaskDetailOverlay,
    }),
  ),
);
const LazyExportPlanDialog = lazyWithRetry(() =>
  import("@/components/Ideation/ExportPlanDialog").then((module) => ({
    default: module.ExportPlanDialog,
  })),
);
const LazyPlanDisplay = lazyWithRetry(() =>
  import("@/components/Ideation/PlanDisplay").then((module) => ({
    default: module.PlanDisplay,
  })),
);
const LazyPlanEditor = lazyWithRetry(() =>
  import("@/components/Ideation/PlanEditor").then((module) => ({
    default: module.PlanEditor,
  })),
);
const LazyPlanEmptyState = lazyWithRetry(() =>
  import("@/components/Ideation/PlanEmptyState").then((module) => ({
    default: module.PlanEmptyState,
  })),
);
const LazyProposalsTabContent = lazyWithRetry(() =>
  import("@/components/Ideation/ProposalsTabContent").then((module) => ({
    default: module.ProposalsTabContent,
  })),
);
const LazyProposalDetailSheet = lazyWithRetry(() =>
  import("@/components/Ideation/ProposalDetailSheet").then((module) => ({
    default: module.ProposalDetailSheet,
  })),
);
const LazyAgentsJiraIssuePanel = lazyWithRetry(() =>
  import("@/components/agents/AgentsJiraIssuePanel").then((module) => ({
    default: module.AgentsJiraIssuePanel,
  })),
);
const LazyAgentsLinearIssuePanel = lazyWithRetry(() =>
  import("@/components/agents/AgentsLinearIssuePanel").then((module) => ({
    default: module.AgentsLinearIssuePanel,
  })),
);
const LazyAgentsClickUpIssuePanel = lazyWithRetry(() =>
  import("@/components/agents/AgentsClickUpIssuePanel").then((module) => ({
    default: module.AgentsClickUpIssuePanel,
  })),
);
const LazyAgentsGranolaNotePanel = lazyWithRetry(() =>
  import("@/components/agents/AgentsGranolaNotePanel").then((module) => ({
    default: module.AgentsGranolaNotePanel,
  })),
);
const LazyAgentsIssuesPanel = lazyWithRetry(() =>
  import("@/components/agents/AgentsIssuesPanel").then((module) => ({
    default: module.AgentsIssuesPanel,
  })),
);
const LazyPullRequestDetailPanel = lazyWithRetry(() =>
  import("@/components/pr/PullRequestDetailPanel").then((module) => ({
    default: module.PullRequestDetailPanel,
  })),
);
const LazyAgentsAutomationPanel = lazyWithRetry(() =>
  import("@/components/agents/AgentsAutomationPanel").then((module) => ({
    default: module.AgentsAutomationPanel,
  })),
);
const LazyPersonaArtifactPanel = lazyWithRetry(() =>
  import("@/components/agents/PersonaArtifactPanel").then((module) => ({
    default: module.PersonaArtifactPanel,
  })),
);

function PersonaArtifactSkeletonFallback() {
  return <ArtifactLoadingState title="Loading persona..." />;
}

const ARTIFACT_TABS: Array<{
  id: IdeationArtifactTab;
  label: string;
  icon: ElementType;
}> = [
  { id: "issues", label: "Issues", icon: AlertCircle },
  { id: "plan", label: "Plan", icon: FileText },
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

const PERSONA_TAB = {
  id: "persona" as const,
  label: "Persona",
  icon: UserRound,
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

const CLICKUP_TAB = {
  id: "clickup" as const,
  label: "ClickUp",
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

const TEAM_TAB = {
  id: "team" as const,
  label: "Team",
  icon: UsersRound,
};

const ALL_ARTIFACT_TAB_DEFINITIONS = [
  ...ARTIFACT_TABS,
  AUTOMATION_TAB,
  PERSONA_TAB,
  PR_TAB,
  JIRA_TAB,
  LINEAR_TAB,
  CLICKUP_TAB,
  GRANOLA_TAB,
  TEAM_TAB,
  REVIEW_TAB,
  PUBLISH_TAB,
] as const;

const ARTIFACT_TAB_UNAVAILABLE_REASONS: Record<AgentArtifactTab, string> = {
  issues: "Appears when this conversation has open issues.",
  plan: "Appears when a plan can be created or already exists.",
  verification: "Appears when verification evidence is available.",
  tasks: "Appears when implementation tasks are available.",
  team: "Appears for Team-capable conversations.",
  automation: "Appears in automation conversations.",
  persona: "Appears in persona-builder conversations.",
  pr: "Appears when this workspace has a pull request.",
  jira: "Appears when Jira is connected and a ticket is attached.",
  linear: "Appears when Linear is connected and a ticket is attached.",
  clickup: "Connect ClickUp in Settings to make it available.",
  granola: "Appears when Granola is connected and a note is attached.",
  review: "Appears when a review is created.",
  publish: "Appears when this conversation has an editable workspace.",
};

type VisibleArtifactTab = {
  id: AgentArtifactTab;
  label: string;
  icon: ElementType;
  enabled: boolean;
  disabledReason?: string | undefined;
};

function visibleTab(
  tab: Omit<VisibleArtifactTab, "enabled" | "disabledReason">,
): VisibleArtifactTab {
  return { ...tab, enabled: true };
}

function baseTabDefinition(
  id: AgentArtifactTab,
): Omit<VisibleArtifactTab, "enabled" | "disabledReason"> {
  const tab = [
    ...ARTIFACT_TABS,
    REVIEW_TAB,
    AUTOMATION_TAB,
    PERSONA_TAB,
    PUBLISH_TAB,
    JIRA_TAB,
    LINEAR_TAB,
    CLICKUP_TAB,
    GRANOLA_TAB,
    TEAM_TAB,
    PR_TAB,
  ].find((candidate) => candidate.id === id);
  return tab ?? AUTOMATION_TAB;
}

function visibleTabFromPolicy(
  policyTab: AutomationConversationPolicyTab,
): VisibleArtifactTab {
  return {
    ...baseTabDefinition(policyTab.id),
    enabled: policyTab.enabled,
    disabledReason: policyTab.disabledReason,
  };
}

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

function mergeWorkspaceReviewMutationContext(
  previous: AgentWorkspaceReviewContext | undefined,
  incoming:
    | StartAgentWorkspaceReviewResult
    | StartAgentWorkspaceReviewFixerResult,
): AgentWorkspaceReviewContext | undefined {
  if (!previous) {
    return previous;
  }
  const candidate: AgentWorkspaceReviewContext = { ...previous, ...incoming };
  const previousVersion = previous.monitor.reviewArtifactVersion ?? 0;
  const incomingVersion = candidate.monitor.reviewArtifactVersion ?? 0;
  if (previousVersion > incomingVersion) {
    return previous;
  }
  const previousUpdatedAt = Date.parse(previous.monitor.updatedAt);
  const incomingUpdatedAt = Date.parse(candidate.monitor.updatedAt);
  if (
    (previous.monitor.status === "ready" ||
      previous.monitor.status === "blocked") &&
    candidate.monitor.status === "reviewing" &&
    previousUpdatedAt >= incomingUpdatedAt
  ) {
    return previous;
  }
  return candidate;
}

interface AgentsArtifactPaneProps {
  conversation: AgentConversation | null;
  workspace?: AgentConversationWorkspace | null;
  activeWorkspaceFreshness?: AgentConversationWorkspaceFreshness | undefined;
  projectBaseBranch?: string | null;
  focusedIdeationSession?: FocusedArtifactIdeationSession | null;
  activeTab: AgentArtifactTab;
  hiddenTabs?: readonly AgentArtifactTab[];
  taskMode: AgentTaskArtifactMode;
  onTabChange: (tab: AgentArtifactTab) => void;
  onHideTab?: (
    tab: AgentArtifactTab,
    availableTabs: readonly AgentArtifactTab[],
  ) => void;
  onShowTab?: (tab: AgentArtifactTab) => void;
  onOpenPublish?: () => void;
  onTaskModeChange: (mode: AgentTaskArtifactMode) => void;
  onPublishWorkspace: ((conversationId: string) => Promise<void>) | undefined;
  isPublishingWorkspace?: boolean;
  publishAttempt?: AgentWorkspacePublishAttempt | null;
  publishFocusRequest?: AgentPublishFocusRequest | null;
  publishSubTabRequest?: AgentPublishSubTabRequest | null;
  taskFocusRequest?: AgentTaskArtifactFocusRequest | null;
  automationRunFocusTarget?: Extract<
    AgentsChatFocus,
    { type: "automation_run" }
  > | null;
  onOpenAutomation?: (automationId: string) => void;
  onConversationModeSwitched?: (
    conversationId: string,
    mode: AgentConversationWorkspaceMode,
    workspace: AgentConversationWorkspace | null,
  ) => void;
  onFocusIdeationSessionForConversation?: (
    conversationId: string,
    sessionId: string,
  ) => void;
  onFocusAutomationRun?: (
    automationId: string,
    runId: string,
    conversationId: string,
    options?: AutomationRunFocusOptions,
  ) => void;
  onFocusVerificationSession:
    ((parentSessionId: string, childSessionId: string) => void) | undefined;
  onFocusWorkspaceReview?: (
    conversationId: string,
    runtimeHint?: AgentRuntimeSelection,
  ) => void;
  onFocusTaskRuntime?: (
    taskId: string,
    contextType: AgentTaskRuntimeContextType,
  ) => void;
  onTaskArtifactSelectionChange?: (taskId: string | null) => void;
  onClose: () => void;
}

export const AgentsArtifactPane = memo(function AgentsArtifactPane({
  conversation,
  workspace = null,
  activeWorkspaceFreshness,
  projectBaseBranch = null,
  focusedIdeationSession = null,
  activeTab,
  hiddenTabs = [],
  taskMode,
  onTabChange,
  onHideTab,
  onShowTab,
  onOpenPublish,
  onTaskModeChange,
  onPublishWorkspace,
  isPublishingWorkspace = false,
  publishAttempt = null,
  publishFocusRequest = null,
  publishSubTabRequest = null,
  taskFocusRequest = null,
  automationRunFocusTarget = null,
  onOpenAutomation,
  onConversationModeSwitched,
  onFocusIdeationSessionForConversation,
  onFocusAutomationRun,
  onFocusVerificationSession,
  onFocusWorkspaceReview,
  onFocusTaskRuntime,
  onTaskArtifactSelectionChange,
  onClose,
}: AgentsArtifactPaneProps) {
  const isRemoteEnvironment = useIsRemoteEnvironment();
  const queryClient = useQueryClient();
  const { data: featureFlags } = useFeatureFlags();
  const { registry: modelRegistry } = useAgentModels();
  const ideationSettingsQuery = useIdeationSettings();
  const tasksEnabled =
    !ideationSettingsQuery.isLoading &&
    !ideationSettingsQuery.isError &&
    ideationSettingsQuery.settings.tasksEnabled;
  const tasksFeatureState =
    !ideationSettingsQuery.isLoading && !ideationSettingsQuery.isError
      ? ideationSettingsQuery.settings.tasksFeatureState
      : "disabled";
  const automationId = conversation?.automationId ?? null;
  const focusedRunTarget =
    automationRunFocusTarget?.automationId === automationId
      ? automationRunFocusTarget
      : null;
  const focusedAutomationRunId =
    conversation?.automationRunId ?? focusedRunTarget?.runId ?? null;
  const focusedAutomationRunConversationId =
    conversation?.automationRunId && conversation?.id
      ? conversation.id
      : (focusedRunTarget?.conversationId ?? null);
  const focusedRunWorkspaceQuery = useQuery({
    queryKey: agentWorkspaceKeys.workspace(focusedAutomationRunConversationId),
    queryFn: () =>
      chatApi.getAgentConversationWorkspace(
        focusedAutomationRunConversationId!,
      ),
    enabled: Boolean(focusedRunTarget && focusedAutomationRunConversationId),
    staleTime: AGENT_WORKSPACE_STALE_MS,
  });
  const focusedWorkspaceResolution = focusedRunTarget
    ? focusedRunWorkspaceQuery.isError ||
      (focusedRunWorkspaceQuery.isSuccess && !focusedRunWorkspaceQuery.data)
      ? "unavailable"
      : focusedRunWorkspaceQuery.data
        ? "resolved"
        : "loading"
    : "resolved";
  const scopedWorkspace = focusedRunTarget
    ? focusedWorkspaceResolution === "resolved"
      ? (focusedRunWorkspaceQuery.data ?? null)
      : null
    : workspace;
  const scopedLocalFreshness =
    scopedWorkspace &&
    activeWorkspaceFreshness?.conversationId === scopedWorkspace.conversationId
      ? activeWorkspaceFreshness
      : undefined;
  const conversationId = conversation?.id ?? workspace?.conversationId ?? null;
  const focusedIdeationSessionId =
    focusedIdeationSession?.conversationId === conversationId
      ? focusedIdeationSession.sessionId
      : null;
  const canHydrateIdeationArtifacts = Boolean(
    conversation?.contextType === "ideation" ||
    conversation?.agentMode === "ideation" ||
    conversation?.agentMode === "tasks" ||
    conversation?.agentMode === "plan" ||
    focusedIdeationSessionId ||
    focusedRunTarget ||
    scopedWorkspace?.mode === "ideation" ||
    scopedWorkspace?.mode === "tasks" ||
    scopedWorkspace?.mode === "plan" ||
    scopedWorkspace?.taskPipelineSessionId ||
    scopedWorkspace?.linkedIdeationSessionId ||
    scopedWorkspace?.linkedPlanBranchId,
  );
  const showPublishTab =
    shouldShowAgentWorkspacePublishSurface(scopedWorkspace);
  const showPullRequestTab = workspaceHasPullRequest(scopedWorkspace);
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
  const teamStoreKey = conversation ? getAgentConversationStoreKey(conversation) : null;
  const activeTeamRunId = useChatStore((state) =>
    teamStoreKey ? state.activeAgentRunIds[teamStoreKey] ?? null : null,
  );
  const showTeamTab = Boolean(
    featureFlags.agentConversationTeam &&
      conversation?.coordinationMode === "rx_native_team" &&
      conversationId,
  );
  const attachedSessionId = useMemo(
    () =>
      focusedIdeationSessionId ??
      (shouldLoadIdeationData
        ? resolveAttachedIdeationSessionId(
            conversation,
            conversationMessages,
            scopedWorkspace?.taskPipelineSessionId ??
              scopedWorkspace?.linkedIdeationSessionId ??
              null,
          )
        : null),
    [
      conversation,
      conversationMessages,
      focusedIdeationSessionId,
      shouldLoadIdeationData,
      scopedWorkspace?.linkedIdeationSessionId,
      scopedWorkspace?.taskPipelineSessionId,
    ],
  );
  const atlassianSettingsQuery = useQuery({
    queryKey: ["atlassian", "settings"],
    queryFn: () => atlassianApi.getSettings(),
    staleTime: 30_000,
  });
  const jiraIntegrationAvailable = Boolean(
    atlassianSettingsQuery.data?.enabled &&
    atlassianSettingsQuery.data?.jiraAvailable,
  );
  const jiraIssueQuery = useQuery({
    queryKey: agentJiraIssueKeys.issue(conversationId),
    queryFn: () =>
      atlassianApi.getAgentConversationJiraIssue({
        conversationId: conversationId!,
      }),
    enabled: Boolean(conversationId && jiraIntegrationAvailable),
    staleTime: 5_000,
  });
  const showJiraTab = Boolean(jiraIntegrationAvailable && jiraIssueQuery.data);
  const linearSettingsQuery = useQuery({
    queryKey: ["linear", "settings"],
    queryFn: () => linearApi.getSettings(),
    staleTime: 30_000,
  });
  const linearIntegrationAvailable = Boolean(
    linearSettingsQuery.data?.enabled &&
    linearSettingsQuery.data?.issueSearchAvailable,
  );
  const linearIssueQuery = useQuery({
    queryKey: agentLinearIssueKeys.issue(conversationId),
    queryFn: () =>
      linearApi.getAgentConversationLinearIssue({
        conversationId: conversationId!,
      }),
    enabled: Boolean(conversationId && linearIntegrationAvailable),
    staleTime: 5_000,
  });
  const showLinearTab = Boolean(
    linearIntegrationAvailable && linearIssueQuery.data,
  );
  const clickupSettingsQuery = useQuery({
    queryKey: ["clickup-integration", "settings"],
    queryFn: () => clickupApi.getSettings(),
    staleTime: 30_000,
  });
  const clickupIntegrationAvailable = Boolean(
    clickupSettingsQuery.data?.enabled &&
    clickupSettingsQuery.data.hasApiToken &&
    clickupSettingsQuery.data.validationStatus === "valid" &&
    clickupSettingsQuery.data.taskSearchAvailable,
  );
  const clickupTicketQuery = useQuery({
    queryKey: ticketingKeys.conversationTicket(conversationId ?? "none"),
    queryFn: () => ticketingApi.getConversationTicket(conversationId!),
    enabled: Boolean(conversationId && clickupIntegrationAvailable),
    staleTime: 5_000,
  });
  const showClickUpTab = Boolean(
    clickupIntegrationAvailable &&
    clickupTicketQuery.data?.ticketRef.provider === "clickup",
  );
  const granolaSettingsQuery = useQuery({
    queryKey: ["granola", "settings"],
    queryFn: () => granolaApi.getSettings(),
    staleTime: 30_000,
  });
  const granolaIntegrationAvailable = Boolean(
    granolaSettingsQuery.data?.enabled &&
    granolaSettingsQuery.data?.validationStatus === "valid",
  );
  const granolaNoteQuery = useQuery({
    queryKey: agentGranolaNoteKeys.note(conversationId),
    queryFn: () =>
      granolaApi.getAgentConversationGranolaNote({
        conversationId: conversationId!,
      }),
    enabled: Boolean(conversationId && granolaIntegrationAvailable),
    staleTime: 5_000,
  });
  const showGranolaTab = Boolean(
    granolaIntegrationAvailable && granolaNoteQuery.data,
  );
  const conversationProjectId =
    conversation?.projectId ??
    scopedWorkspace?.projectId ??
    workspace?.projectId ??
    null;
  const canStartPlan = Boolean(
    conversationId &&
    conversationProjectId &&
    !automationId &&
    (scopedWorkspace
      ? scopedWorkspace.mode === "edit" || scopedWorkspace.mode === "plan"
      : conversation?.contextType === "project"),
  );
  const prReviewConversationId =
    workspace?.mode === "review_pr" ? workspace.conversationId : null;
  const isReviewPrWorkspace = workspace?.mode === "review_pr";
  const nestsWorkspaceReview = Boolean(
    showPublishTab &&
    scopedWorkspace &&
    isLocalWorkspaceReviewModeEligible(scopedWorkspace.mode) &&
    !isReviewPrWorkspace,
  );
  const hasPublishedPr = hasPublishedWorkspacePr(scopedWorkspace);
  const [publishSubTabByConversation, setPublishSubTabByConversation] =
    useState<Record<string, AgentPublishSubTab>>(() =>
      conversationId
        ? {
            [conversationId]:
              nestsWorkspaceReview && activeTab === "review"
                ? "review"
                : "changes",
          }
        : {},
    );
  const lastHandledPublishSubTabRequestIdRef = useRef(0);
  const [pendingReviewFocusConversationId, setPendingReviewFocusConversationId] =
    useState<string | null>(null);
  const rememberedPublishSubTab = conversationId
    ? publishSubTabByConversation[conversationId]
    : null;
  const publishSubTab = rememberedPublishSubTab
    ? (rememberedPublishSubTab === "review" && !nestsWorkspaceReview) ||
      (rememberedPublishSubTab === "checks" && !hasPublishedPr)
      ? "changes"
      : rememberedPublishSubTab
    : nestsWorkspaceReview && activeTab === "review"
      ? "review"
      : "changes";
  const selectPublishSubTab = useCallback(
    (tab: AgentPublishSubTab) => {
      if (!conversationId) return;
      setPublishSubTabByConversation((current) => ({
        ...current,
        [conversationId]: tab,
      }));
    },
    [conversationId],
  );
  useEffect(() => {
    if (
      !showPublishTab ||
      publishSubTabRequest?.conversationId !== conversationId ||
      (publishSubTabRequest.tab === "review" && !nestsWorkspaceReview) ||
      (publishSubTabRequest.tab === "checks" && !hasPublishedPr) ||
      publishSubTabRequest.requestId <=
        lastHandledPublishSubTabRequestIdRef.current
    ) {
      return;
    }
    lastHandledPublishSubTabRequestIdRef.current =
      publishSubTabRequest.requestId;
    selectPublishSubTab(publishSubTabRequest.tab);
    if (publishSubTabRequest.tab === "review") {
      setPendingReviewFocusConversationId(conversationId);
    } else {
      setPendingReviewFocusConversationId(null);
    }
  }, [
    conversationId,
    hasPublishedPr,
    nestsWorkspaceReview,
    publishSubTabRequest,
    selectPublishSubTab,
    showPublishTab,
  ]);
  useEffect(() => {
    if (!nestsWorkspaceReview || activeTab !== "review") {
      return;
    }
    selectPublishSubTab("review");
    onTabChange("publish");
  }, [activeTab, nestsWorkspaceReview, onTabChange, selectPublishSubTab]);
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
  const [autoApprovePreference, setAutoApprovePreference] = useState<{
    conversationId: string;
    enabled: boolean;
  } | null>(null);
  useEffect(() => {
    setAutoApprovePreference(null);
  }, [prReviewConversationId]);
  const autoApproveEnabled =
    autoApprovePreference?.conversationId === prReviewConversationId
      ? autoApprovePreference.enabled
      : (prReviewContext?.monitor?.autoApproveEnabled ?? true);
  const autoApproveMutation = useMutation({
    mutationFn: ({
      conversationId,
      enabled,
    }: {
      conversationId: string;
      enabled: boolean;
    }) => chatApi.setAgentWorkspacePrReviewAutoApprove(conversationId, enabled),
    onSuccess: (result, variables) => {
      setAutoApprovePreference({
        conversationId: variables.conversationId,
        enabled: result.monitor.autoApproveEnabled,
      });
      queryClient.setQueryData(
        agentWorkspaceKeys.prReview(variables.conversationId),
        (previous: AgentWorkspacePrReviewContext | undefined) =>
          previous ? { ...previous, monitor: result.monitor } : previous,
      );
      void invalidateWorkspaceQueries(queryClient, variables.conversationId);
    },
    onError: (error, variables) => {
      setAutoApprovePreference((previous) =>
        previous?.conversationId === variables.conversationId ? null : previous,
      );
      toast.error(extractErrorMessage(error, "Failed to update Auto Approve"));
    },
  });
  const prReviewMonitoringMutation = useMutation({
    mutationFn: ({
      conversationId,
      enabled,
      activeReviewPolicy,
    }: {
      conversationId: string;
      enabled: boolean;
      activeReviewPolicy?: "finish_current" | "cancel_current";
    }) =>
      activeReviewPolicy
        ? chatApi.setAgentWorkspacePrReviewMonitoring(
            conversationId,
            enabled,
            activeReviewPolicy,
          )
        : chatApi.setAgentWorkspacePrReviewMonitoring(conversationId, enabled),
    onSuccess: (result, variables) => {
      queryClient.setQueryData(
        agentWorkspaceKeys.prReview(variables.conversationId),
        (previous: AgentWorkspacePrReviewContext | undefined) =>
          previous ? { ...previous, monitor: result.monitor } : previous,
      );
      toast.success(
        result.monitor.monitorEnabled
          ? "PR review monitoring restarted"
          : "New-head PR reviews paused; lifecycle monitoring continues",
      );
      void invalidateWorkspaceQueries(queryClient, variables.conversationId);
    },
    onError: (error) => {
      toast.error(
        extractErrorMessage(error, "Failed to update PR review monitoring"),
      );
    },
  });
  const workspaceReviewConversationId = resolveWorkspaceReviewOwnerConversationId({
    activeConversationContextType:
      conversation?.contextType ?? (scopedWorkspace ? "project" : null),
    activeConversationId: conversation?.id ?? workspace?.conversationId,
    activeConversationParentId: conversation?.parentConversationId,
    activeConversationMode: scopedWorkspace?.mode,
    activeWorkspaceConversationId: scopedWorkspace?.conversationId,
  });
  const shouldLoadWorkspaceReviewContext = Boolean(
    workspaceReviewConversationId &&
    scopedWorkspace &&
    isLocalWorkspaceReviewModeEligible(scopedWorkspace.mode),
  );
  const workspaceReviewContextQuery = useQuery({
    queryKey: agentWorkspaceKeys.workspaceReview(
      workspaceReviewConversationId ?? "",
    ),
    queryFn: ({ signal }) =>
      chatApi.getAgentWorkspaceReviewContext(workspaceReviewConversationId!, {
        signal,
      }),
    enabled: shouldLoadWorkspaceReviewContext,
    staleTime: 5_000,
  });
  const workspaceReviewContext = workspaceReviewContextForConversation(
    workspaceReviewContextQuery.data,
    workspaceReviewConversationId,
  );
  const workspaceReviewArtifactId = isReviewPrWorkspace
    ? null
    : (workspaceReviewContext?.monitor.reviewArtifactId ?? null);
  const workspaceReviewRequestedChangesArtifactId = isReviewPrWorkspace
    ? null
    : (workspaceReviewContext?.monitor.reviewRequestedChangesArtifactId ??
      null);
  const prReviewArtifactId = prReviewContext?.monitor?.reviewArtifactId ?? null;
  const reviewArtifactId = isReviewPrWorkspace
    ? prReviewArtifactId
    : workspaceReviewArtifactId;
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
  const reviewRequestedChangesArtifactQuery = useQuery({
    queryKey: [
      "agents",
      "artifact",
      workspaceReviewRequestedChangesArtifactId,
    ],
    queryFn: () =>
      artifactApi.get(workspaceReviewRequestedChangesArtifactId!),
    enabled: Boolean(workspaceReviewRequestedChangesArtifactId),
    staleTime: 5_000,
  });
  const reviewRequestedChangesArtifact =
    workspaceReviewRequestedChangesArtifactId &&
    reviewRequestedChangesArtifactQuery.data?.id ===
      workspaceReviewRequestedChangesArtifactId
      ? reviewRequestedChangesArtifactQuery.data
      : null;
  const startWorkspaceReviewMutation = useMutation({
    mutationFn: ({
      conversationId,
      force,
      confirmation,
      runtimeOverride,
      enableReviewAutomation,
    }: {
      conversationId: string;
      force: boolean;
      confirmation?: AgentWorkspaceReviewStartConfirmation;
      runtimeOverride?: import("@/api/manual-role-defaults.types").ManualRoleRuntimeSelection;
      enableReviewAutomation?: boolean;
    }) =>
      chatApi.startAgentWorkspaceReview(
        conversationId,
        confirmation
          ? {
              force,
              confirmation,
              ...(runtimeOverride ? { runtimeOverride } : {}),
              ...(enableReviewAutomation ? { enableReviewAutomation } : {}),
            }
          : { force, ...(enableReviewAutomation ? { enableReviewAutomation } : {}) },
      ),
    onSuccess: (result, variables) => {
      queryClient.setQueryData(
        agentWorkspaceKeys.workspaceReview(variables.conversationId),
        (previous: AgentWorkspaceReviewContext | undefined) =>
          mergeWorkspaceReviewMutationContext(previous, result),
      );
      const reviewConversationId = result.monitor.reviewConversationId;
      if (reviewConversationId) {
        invalidateConversationDataQueries(queryClient, reviewConversationId);
        const runtimeHint =
          result.started &&
          !result.wasQueued &&
          variables.runtimeOverride
            ? materializeWorkspaceRuntimeSelection(
                variables.runtimeOverride,
                modelRegistry,
              )
            : null;
        if (runtimeHint) {
          onFocusWorkspaceReview?.(reviewConversationId, runtimeHint);
        } else {
          onFocusWorkspaceReview?.(reviewConversationId);
        }
      }
      const artifactId = result.monitor.reviewArtifactId;
      if (artifactId) {
        void queryClient.invalidateQueries({
          queryKey: ["agents", "artifact", artifactId],
        });
      }
      const requestedChangesArtifactId =
        result.monitor.reviewRequestedChangesArtifactId;
      if (requestedChangesArtifactId) {
        void queryClient.invalidateQueries({
          queryKey: ["agents", "artifact", requestedChangesArtifactId],
        });
      }
    },
  });
  const startWorkspaceReviewFixerMutation = useMutation({
    mutationFn: ({
      conversationId,
      confirmation,
      runtimeOverride,
    }: {
      conversationId: string;
      confirmation: import("@/api/chat").AgentWorkspaceReviewFixerConfirmation;
      runtimeOverride: import("@/api/manual-role-defaults.types").ManualRoleRuntimeSelection;
    }) =>
      chatApi.startAgentWorkspaceReviewFixer(conversationId, {
        confirmation,
        runtimeOverride,
      }),
    onSuccess: (result, variables) => {
      queryClient.setQueryData(
        agentWorkspaceKeys.workspaceReview(variables.conversationId),
        (previous: AgentWorkspaceReviewContext | undefined) =>
          mergeWorkspaceReviewMutationContext(previous, result),
      );
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
      const requestedChangesArtifactId =
        result.monitor.reviewRequestedChangesArtifactId;
      if (requestedChangesArtifactId) {
        void queryClient.invalidateQueries({
          queryKey: ["agents", "artifact", requestedChangesArtifactId],
        });
      }
    },
  });
  const approveWorkspaceReviewAnywayMutation = useMutation({
    mutationFn: ({
      conversationId,
      targetScope,
      diffFingerprint,
      artifactId,
      artifactVersion,
    }: {
      conversationId: string;
      targetScope: "selected_source" | "workspace_delta";
      diffFingerprint: string;
      artifactId: string;
      artifactVersion: number;
    }) =>
      chatApi.approveAgentWorkspaceReviewAnyway(conversationId, {
        targetScope,
        diffFingerprint,
        artifactId,
        artifactVersion,
      }),
    onSuccess: (result, variables) => {
      queryClient.setQueryData(
        agentWorkspaceKeys.workspaceReview(variables.conversationId),
        (previous: AgentWorkspaceReviewContext | undefined) =>
          previous ? { ...previous, monitor: result.monitor } : previous,
      );
      toast.success("Review approved anyway for the current changes");
      void invalidateWorkspaceQueries(queryClient, variables.conversationId);
    },
    onError: (error, variables) => {
      toast.error(
        extractErrorMessage(
          error,
          "The Review changed before it could be approved. Refresh and try again.",
        ),
      );
      void refreshWorkspaceReviewContext(
        queryClient,
        variables.conversationId,
        "full_target",
      ).catch(() => undefined);
    },
  });
  const [taskArtifactSelectedId, setTaskArtifactSelectedIdState] = useState<
    string | null
  >(() => readSelectedTaskForConversation(conversationId));
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
    scopedWorkspace?.projectId ??
    workspace?.projectId ??
    null;
  const taskHistoryQuery = useSessionTaskHistoryAvailability(
    taskProjectId ?? "",
    attachedSessionId,
  );
  const tasksSurfaceCapabilities = useMemo(
    () =>
      deriveTasksSurfaceCapabilities({
        featureState: tasksFeatureState,
        hasHistory: taskHistoryQuery.data?.hasHistory ?? false,
        historyUnavailable: taskHistoryQuery.isError,
      }),
    [
      taskHistoryQuery.data?.hasHistory,
      taskHistoryQuery.isError,
      tasksFeatureState,
    ],
  );
  const activePlanSessionId = usePlanStore(
    selectActivePlanId(taskProjectId ?? ""),
  );
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
      scopedWorkspace?.linkedPlanBranchId),
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
  const issueConversationId =
    conversation?.contextType === "project" ? conversation.id : null;
  const isAutomationRunConversation = Boolean(focusedAutomationRunId);
  const automationDetailQuery = useAutomationDetail(automationId, {
    enabled: Boolean(automationId),
  });
  const focusedAutomationRun = useMemo(() => {
    if (!focusedAutomationRunId) {
      return null;
    }
    return (
      automationDetailQuery.data?.runs.find(
        (run) => run.id === focusedAutomationRunId,
      ) ?? null
    );
  }, [automationDetailQuery.data?.runs, focusedAutomationRunId]);
  const runPlanArtifactId = focusedAutomationRun?.planArtifactId ?? null;
  const setupSpecArtifactId = isAutomationRunConversation
    ? null
    : (automationDetailQuery.data?.automation.specArtifactId ?? null);
  const planArtifactId =
    runPlanArtifactId ??
    setupSpecArtifactId ??
    (shouldLoadIdeationData
      ? (sessionData?.session.planArtifactId ??
        sessionData?.session.inheritedPlanArtifactId ??
        null)
      : null);
  const proposalCount = proposals.length;
  const automationRunTabPolicy = useMemo(
    () =>
      getAutomationConversationTabPolicy({
        surface: "run",
        runStatus: focusedAutomationRun?.status ?? null,
        judgeState: focusedAutomationRun?.judgeState ?? null,
        workspaceMode: scopedWorkspace?.mode ?? null,
        availability: {
          hasPlanArtifact: Boolean(planArtifactId),
          hasPullRequest: Boolean(
            focusedAutomationRun?.prNumber || focusedAutomationRun?.prUrl,
          ),
          hasPublishWorkspace: showPublishTab,
          canStartPlan: false,
        },
      }),
    [
      focusedAutomationRun?.judgeState,
      focusedAutomationRun?.prNumber,
      focusedAutomationRun?.prUrl,
      focusedAutomationRun?.status,
      planArtifactId,
      showPublishTab,
      scopedWorkspace?.mode,
    ],
  );
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
        hasVerificationEvidence: false,
        hasExecutionTasks: tasksSurfaceCapabilities.hasHistory,
      }),
    [
      canStartPlan,
      tasksSurfaceCapabilities.hasHistory,
      planArtifactId,
      sessionData,
    ],
  );
  const availableArtifactTabIds = useMemo<IdeationArtifactTab[]>(() => {
    const tabs =
      conversation?.contextType === "project" && hasConversationIssues
        ? (["issues", ...availableIdeationTabIds] as IdeationArtifactTab[])
        : availableIdeationTabIds;
    const shouldShowReviewTab = isReviewPrWorkspace && Boolean(prReviewContext);
    if (!shouldShowReviewTab || tabs.includes("review")) {
      return tabs;
    }
    return [...tabs, "review"];
  }, [
    availableIdeationTabIds,
    conversation?.contextType,
    hasConversationIssues,
    isReviewPrWorkspace,
    prReviewContext,
  ]);
  const personaArtifactOnly = isPersonaArtifactConversation(conversation);
  const availableTabs = useMemo<VisibleArtifactTab[]>(
    () =>
      personaArtifactOnly
        ? [visibleTab(PERSONA_TAB)]
        : isAutomationRunConversation
          ? automationRunTabPolicy.tabs.map(visibleTabFromPolicy)
          : [
            ...ARTIFACT_TABS.filter((tab) =>
              availableArtifactTabIds.includes(tab.id),
            ).map(visibleTab),
            ...(automationId ? [visibleTab(AUTOMATION_TAB)] : []),
            ...(showPullRequestTab ? [visibleTab(PR_TAB)] : []),
            ...(showJiraTab ? [visibleTab(JIRA_TAB)] : []),
            ...(showLinearTab ? [visibleTab(LINEAR_TAB)] : []),
            ...(showClickUpTab ? [visibleTab(CLICKUP_TAB)] : []),
            ...(showGranolaTab ? [visibleTab(GRANOLA_TAB)] : []),
            ...(showTeamTab ? [visibleTab(TEAM_TAB)] : []),
            ...(availableArtifactTabIds.includes("review")
              ? [visibleTab(REVIEW_TAB)]
              : []),
            ...(showPublishTab ? [visibleTab(PUBLISH_TAB)] : []),
          ],
    [
      availableArtifactTabIds,
      automationId,
      automationRunTabPolicy.tabs,
      isAutomationRunConversation,
      personaArtifactOnly,
      showClickUpTab,
      showGranolaTab,
      showJiraTab,
      showLinearTab,
      showPublishTab,
      showPullRequestTab,
      showTeamTab,
    ],
  );
  const shownTabs = useMemo(
    () =>
      personaArtifactOnly
        ? availableTabs
        : availableTabs.filter((tab) => !hiddenTabs.includes(tab.id)),
    [availableTabs, hiddenTabs, personaArtifactOnly],
  );
  const shownEnabledTabs = useMemo(
    () => shownTabs.filter((tab) => tab.enabled),
    [shownTabs],
  );
  const enabledAvailableTabIds = useMemo(
    () => availableTabs.filter((tab) => tab.enabled).map((tab) => tab.id),
    [availableTabs],
  );
  const customizerTabs = useMemo<AgentArtifactTabCustomizerItem[]>(
    () =>
      ALL_ARTIFACT_TAB_DEFINITIONS.map((definition) => {
        const availableTab = availableTabs.find(
          (tab) => tab.id === definition.id,
        );
        return {
          ...definition,
          available: availableTab?.enabled === true,
          unavailableReason:
            availableTab?.disabledReason ??
            (isRemoteEnvironment && ["jira", "linear", "clickup", "granola"].includes(definition.id)
              ? `${definition.label} integration runs on the host.`
              : ARTIFACT_TAB_UNAVAILABLE_REASONS[definition.id]),
        };
      }),
    [availableTabs, isRemoteEnvironment],
  );
  const allAvailableTabsHidden =
    enabledAvailableTabIds.length > 0 && shownEnabledTabs.length === 0;
  const normalizedActiveTab =
    nestsWorkspaceReview && activeTab === "review" ? "publish" : activeTab;
  const requestedFallbackActiveTab = isAutomationRunConversation
    ? automationRunTabPolicy.defaultTab
    : automationId && conversation?.agentMode === "automation"
      ? "automation"
      : isReviewPrWorkspace && (prReviewContext || reviewArtifactId)
        ? "review"
        : nestsWorkspaceReview &&
            (workspaceReviewContext?.shouldShowTab || workspaceReviewArtifactId)
          ? "publish"
          : showPullRequestTab
          ? "pr"
          : showJiraTab
            ? "jira"
            : showLinearTab
              ? "linear"
              : showClickUpTab
                ? "clickup"
                : showGranolaTab
                  ? "granola"
                  : shownTabs.some((tab) => tab.id === "plan")
                    ? "plan"
                    : shownTabs.some((tab) => tab.id === "issues")
                      ? "issues"
                      : shownTabs.some((tab) => tab.id === "review")
                        ? "review"
                        : "plan";
  const fallbackActiveTab =
    shownEnabledTabs.find(
      (tab) => tab.id === requestedFallbackActiveTab && tab.enabled,
    )?.id ??
    shownEnabledTabs[0]?.id ??
    "automation";
  const effectiveActiveTab = shownTabs.some(
    (tab) => tab.id === normalizedActiveTab && tab.enabled,
  )
    ? normalizedActiveTab
    : fallbackActiveTab;
  const runtimeStatusStoreKey = conversation
    ? getAgentConversationStoreKey(conversation)
    : null;
  const runtimeStatusQuery = useAgentConversationRuntimeStatus(conversationId, {
    enabled: Boolean(
      conversationId &&
      (effectiveActiveTab === "review" ||
        (effectiveActiveTab === "publish" && publishSubTab === "review")),
    ),
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
  const workspaceReviewStartError =
    startWorkspaceReviewMutation.variables?.conversationId === conversationId
      ? startWorkspaceReviewMutation.error
      : null;
  const workspaceReviewFixIssuesError =
    startWorkspaceReviewFixerMutation.variables?.conversationId ===
    conversationId
      ? startWorkspaceReviewFixerMutation.error
      : null;
  const isWorkspaceReviewApproveAnywayPending =
    approveWorkspaceReviewAnywayMutation.isPending &&
    approveWorkspaceReviewAnywayMutation.variables?.conversationId ===
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
    Boolean(workspaceReviewStartError) ||
    Boolean(workspaceReviewFixIssuesError) ||
    isWorkspaceReviewBlockingPublish(reviewDisplayContext);
  const reviewTabIconColor = (() => {
    if (isWorkspaceReviewRunning) return "var(--accent-primary)";
    if (workspaceReviewBlocked) return "var(--status-error)";
    if (isWorkspaceReviewApprovedAnyway(reviewDisplayContext))
      return "var(--status-warning)";
    if (hasWorkspaceReviewPublishAuthorization(reviewDisplayContext))
      return "var(--status-success)";
    if (
      reviewDisplayContext?.reviewArtifactIsOutdated ||
      reviewDisplayContext?.monitor.reviewGateStatus === "required"
    ) {
      return "var(--status-warning)";
    }
    return null;
  })();
  const reviewTabStatusColor = isWorkspaceReviewRunning
    ? reviewTabIconColor
    : null;
  const reviewTabStatusLabel = (() => {
    if (isWorkspaceReviewRunning) return "Running";
    if (workspaceReviewBlocked) {
      return workspaceReviewStartError ||
        workspaceReviewFixIssuesError ||
        reviewDisplayContext?.monitor.reviewGateStatus === "failed" ||
        reviewDisplayContext?.monitor.reviewOutcome === "run_failed"
        ? "Failed"
        : "Issues";
    }
    if (isWorkspaceReviewApprovedAnyway(reviewDisplayContext)) return "Approved";
    if (hasWorkspaceReviewPublishAuthorization(reviewDisplayContext)) {
      return "Passed";
    }
    if (reviewDisplayContext?.reviewArtifactIsOutdated) return "Outdated";
    if (reviewDisplayContext?.monitor.reviewGateStatus === "required") {
      return "Required";
    }
    return null;
  })();
  const shouldLoadDependencyGraph =
    shouldLoadIdeationData &&
    (effectiveActiveTab === "tasks" ||
      (effectiveActiveTab === "plan" && proposalCount > 0));
  const shouldUseSessionPlanQuery =
    shouldLoadIdeationData && !!attachedSessionId && !!sessionData?.session;
  const planArtifactQueryKey = shouldUseSessionPlanQuery
    ? ["agents", "session-plan", attachedSessionId, planArtifactId]
    : ["agents", "artifact", planArtifactId];
  const planArtifactQuery = useQuery({
    queryKey: planArtifactQueryKey,
    queryFn: () =>
      shouldUseSessionPlanQuery
        ? artifactApi.getSessionPlan(attachedSessionId!)
        : artifactApi.get(planArtifactId!),
    enabled: shouldUseSessionPlanQuery ? !!attachedSessionId : !!planArtifactId,
    staleTime: 5_000,
  });
  const planArtifact = planArtifactQuery.data ?? null;
  const isPlanHydrating =
    effectiveActiveTab === "plan" &&
    !planArtifact &&
    (planArtifactQuery.isFetching ||
      (shouldUseSessionPlanQuery && sessionQuery.isFetching));
  const dependencyQuery = useDependencyGraph(
    shouldLoadDependencyGraph ? (attachedSessionId ?? "") : "",
  );
  const verificationQuery = useVerificationStatus(
    shouldLoadIdeationData && effectiveActiveTab === "plan" && planArtifactId
      ? (attachedSessionId ?? undefined)
      : undefined,
    conversationId,
  );
  const dependencyGraph =
    attachedSessionId && sessionData ? (dependencyQuery.data ?? null) : null;
  const verificationState = verificationQuery.data?.status ?? null;
  const verificationInProgress = verificationQuery.data?.inProgress ?? false;
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
        void queryClient.invalidateQueries({
          queryKey: verificationStatusKey(attachedSessionId),
        });
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
      if (result.blueprintArtifact) {
        queryClient.setQueryData(
          ["agents", "artifact", result.blueprintArtifact.id],
          result.blueprintArtifact,
        );
        queryClient.setQueryData(
          [
            "agents",
            "session-plan",
            result.sessionId,
            result.blueprintArtifact.id,
          ],
          result.blueprintArtifact,
        );
      }
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
  const startWorkspaceReviewWithConfirmation = useCallback(
    ({
      force,
      confirmation,
      runtimeOverride,
      enableReviewAutomation,
    }: {
      force: boolean;
      confirmation?: AgentWorkspaceReviewStartConfirmation;
      runtimeOverride?: import("@/api/manual-role-defaults.types").ManualRoleRuntimeSelection;
      enableReviewAutomation?: boolean;
    }) => {
      if (!workspaceReviewConversationId) {
        return Promise.resolve();
      }
      return confirmation
        ? startWorkspaceReviewMutation.mutateAsync({
            conversationId: workspaceReviewConversationId,
            force,
            confirmation,
            ...(runtimeOverride ? { runtimeOverride } : {}),
            ...(enableReviewAutomation ? { enableReviewAutomation } : {}),
          })
        : startWorkspaceReviewMutation.mutateAsync({
            conversationId: workspaceReviewConversationId,
            force,
          });
    },
    [startWorkspaceReviewMutation, workspaceReviewConversationId],
  );
  const reviewSettingsQuery = useReviewSettings();
  const reviewAutomation = useMemo(() => {
    if (!scopedWorkspace || !reviewSettingsQuery.data) {
      return null;
    }
    const override = scopedWorkspace.reviewAutomationOverride;
    const effectiveAutofix =
      override ?? reviewSettingsQuery.data.autofix_workspace_review_blocking_findings;
    const effectiveAutoReview =
      override ?? reviewSettingsQuery.data.require_workspace_review;
    return {
      effectiveLoopActive: effectiveAutofix && effectiveAutoReview,
      overrideOn: override === true,
    };
  }, [reviewSettingsQuery.data, scopedWorkspace]);
  const {
    startReview: confirmAndStartWorkspaceReview,
    prefetchStartReview: prefetchWorkspaceReview,
    startFixer: confirmAndStartWorkspaceReviewFixer,
    confirmationDialogProps: workspaceReviewConfirmationDialogProps,
    ConfirmationDialog: WorkspaceReviewConfirmationDialog,
  } = useWorkspaceReviewActions({
    conversationId: workspaceReviewConversationId,
    onStartReview: startWorkspaceReviewWithConfirmation,
    projectId: scopedWorkspace?.projectId ?? null,
    reviewAutomation,
    onStartFixer: ({ confirmation, runtimeOverride }) => {
      if (!workspaceReviewConversationId) return Promise.resolve();
      return startWorkspaceReviewFixerMutation.mutateAsync({
        conversationId: workspaceReviewConversationId,
        confirmation,
        runtimeOverride,
      });
    },
  });
  const handleStartReview = useCallback(
    (force: boolean) => {
      if (
        !workspaceReviewConversationId ||
        isWorkspaceReviewActionPending ||
        isWorkspaceReviewFixIssuesPending ||
        isWorkspaceRuntimeGenerating
      ) {
        return;
      }
      confirmAndStartWorkspaceReview(force);
    },
    [
      workspaceReviewConversationId,
      isWorkspaceReviewActionPending,
      isWorkspaceReviewFixIssuesPending,
      isWorkspaceRuntimeGenerating,
      confirmAndStartWorkspaceReview,
    ],
  );
  const handleFixReviewIssues = useCallback(() => {
    if (
      !workspaceReviewConversationId ||
      isWorkspaceReviewActionPending ||
      isWorkspaceReviewFixIssuesPending ||
      isWorkspaceRuntimeGenerating ||
      isPublishingWorkspace
    ) {
      return;
    }
    if (workspaceReviewContext) {
      confirmAndStartWorkspaceReviewFixer(workspaceReviewContext);
    }
  }, [
    isPublishingWorkspace,
    isWorkspaceReviewActionPending,
    isWorkspaceReviewFixIssuesPending,
    isWorkspaceRuntimeGenerating,
    confirmAndStartWorkspaceReviewFixer,
    workspaceReviewContext,
    workspaceReviewConversationId,
  ]);
  const handleApproveReviewAnyway = useCallback(async () => {
    const target = reviewDisplayContext?.target;
    const monitor = reviewDisplayContext?.monitor;
    if (
      !workspaceReviewConversationId ||
      !target ||
      !monitor?.reviewArtifactId ||
      !monitor.reviewArtifactVersion ||
      isWorkspaceReviewApproveAnywayPending ||
      isWorkspaceRuntimeGenerating ||
      isPublishingWorkspace
    ) {
      return;
    }
    await approveWorkspaceReviewAnywayMutation.mutateAsync({
      conversationId: workspaceReviewConversationId,
      targetScope: target.scope,
      diffFingerprint: target.diffFingerprint,
      artifactId: monitor.reviewArtifactId,
      artifactVersion: monitor.reviewArtifactVersion,
    });
  }, [
    approveWorkspaceReviewAnywayMutation,
    isPublishingWorkspace,
    isWorkspaceReviewApproveAnywayPending,
    isWorkspaceRuntimeGenerating,
    reviewDisplayContext,
    workspaceReviewConversationId,
  ]);
  useEffect(() => {
    if (
      pendingReviewFocusConversationId !== conversationId ||
      !nestsWorkspaceReview ||
      effectiveActiveTab !== "publish" ||
      publishSubTab !== "review"
    ) {
      return;
    }
    const reviewConversationId =
      reviewDisplayContext?.monitor.reviewConversationId ?? null;
    if (!reviewConversationId) {
      return;
    }
    onFocusWorkspaceReview?.(reviewConversationId);
    setPendingReviewFocusConversationId(null);
  }, [
    conversationId,
    effectiveActiveTab,
    nestsWorkspaceReview,
    onFocusWorkspaceReview,
    pendingReviewFocusConversationId,
    publishSubTab,
    reviewDisplayContext?.monitor.reviewConversationId,
  ]);
  const handleOpenPublish = useCallback(() => {
    selectPublishSubTab("changes");
    setPendingReviewFocusConversationId(null);
    if (onOpenPublish) {
      onOpenPublish();
      return;
    }
    onTabChange("publish");
  }, [onOpenPublish, onTabChange, selectPublishSubTab]);
  const handleOpenReview = useCallback(() => {
    if (!nestsWorkspaceReview) {
      setPendingReviewFocusConversationId(null);
      if (activeTab !== "review") {
        onTabChange("review");
      }
      return;
    }
    selectPublishSubTab("review");
    setPendingReviewFocusConversationId(conversationId);
    if (activeTab !== "publish") {
      onTabChange("publish");
    }
  }, [
    activeTab,
    conversationId,
    nestsWorkspaceReview,
    onTabChange,
    selectPublishSubTab,
  ]);
  const handlePublishSubTabChange = useCallback(
    (tab: AgentPublishSubTab) => {
      if (tab === "review") {
        handleOpenReview();
        return;
      }
      if (tab === "changes") {
        handleOpenPublish();
        return;
      }
      selectPublishSubTab(tab);
      setPendingReviewFocusConversationId(null);
      if (activeTab !== "publish") {
        if (onOpenPublish) {
          onOpenPublish();
          return;
        }
        onTabChange("publish");
      }
    },
    [
      activeTab,
      handleOpenPublish,
      handleOpenReview,
      onOpenPublish,
      onTabChange,
      selectPublishSubTab,
    ],
  );
  const handleAddArtifactExcerpt = useCallback(
    (reference: Parameters<typeof stageComposerExcerptReference>[1]) => {
      if (conversationId)
        stageComposerExcerptReference(conversationId, reference);
    },
    [conversationId],
  );
  const artifactSelectionEnabled = Boolean(
    conversationId &&
    effectiveActiveTab !== "persona" &&
    (!isAutomationRunConversation ||
      (focusedAutomationRun &&
        !isAutomationRunComposerReadOnly(focusedAutomationRun))),
  );

  return (
    <>
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
            {shownTabs.map(
              ({ id, label, icon: Icon, enabled, disabledReason }) => {
                const isActive = effectiveActiveTab === id;
                const count =
                  id === "tasks" ? visibleImplementationTaskCount : 0;

                let iconColor: string | undefined;
                let iconPulse = false;
                let tabStatusColor: string | null = null;
                if (id === "review") {
                  iconColor = reviewTabIconColor ?? undefined;
                  iconPulse = isWorkspaceReviewRunning;
                  tabStatusColor = reviewTabStatusColor;
                }

                const tabButton = (
                  <button
                    key={id}
                    type="button"
                    aria-disabled={enabled ? undefined : "true"}
                    onClick={() => {
                      if (!enabled) {
                        return;
                      }
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
                      !enabled ? "cursor-not-allowed opacity-60" : "",
                    )}
                    style={{
                      color: isActive
                        ? "var(--text-primary)"
                        : "var(--text-muted)",
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
                if (!enabled && disabledReason) {
                  return (
                    <Tooltip key={id}>
                      <TooltipTrigger asChild>{tabButton}</TooltipTrigger>
                      <TooltipContent side="bottom" className="text-xs">
                        {disabledReason}
                      </TooltipContent>
                    </Tooltip>
                  );
                }
                if (personaArtifactOnly) {
                  return tabButton;
                }
                return (
                  <ContextMenu key={id}>
                    <ContextMenuTrigger asChild>{tabButton}</ContextMenuTrigger>
                    <ContextMenuContent
                      style={{
                        backgroundColor: "var(--bg-elevated)",
                        borderColor: "var(--overlay-medium)",
                        borderWidth: 1,
                        borderStyle: "solid",
                      }}
                    >
                      <ContextMenuItem
                        onSelect={() => onHideTab?.(id, enabledAvailableTabIds)}
                      >
                        Hide “{label}”
                      </ContextMenuItem>
                    </ContextMenuContent>
                  </ContextMenu>
                );
              },
            )}
          </div>

          <div className="ml-auto flex items-center gap-1">
            {availableTabs.length > 0 && !personaArtifactOnly ? (
              <AgentsArtifactTabCustomizer
                tabs={customizerTabs}
                hiddenTabs={hiddenTabs}
                onHide={(tab) => onHideTab?.(tab, enabledAvailableTabIds)}
                onShow={(tab) => onShowTab?.(tab)}
              />
            ) : null}
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

        <AgentWorkspaceToolbar
          workspace={scopedWorkspace}
          resolutionState={focusedWorkspaceResolution}
        />

        <div
          key={
            personaArtifactOnly
              ? (conversationId ?? "no-conversation")
              : "artifact-content"
          }
          className="flex-1 min-h-0 overflow-y-auto"
          data-testid={
            allAvailableTabsHidden
              ? "agents-artifact-content-hidden"
              : `agents-artifact-content-${effectiveActiveTab}`
          }
        >
          {allAvailableTabsHidden ? (
            <div className="flex h-full min-h-[240px] flex-col items-center justify-center gap-3 px-6 text-center">
              <div>
                <h2
                  className="text-sm font-semibold"
                  style={{ color: "var(--text-primary)" }}
                >
                  All tabs are hidden
                </h2>
                <p
                  className="mt-1 text-sm"
                  style={{ color: "var(--text-muted)" }}
                >
                  Choose which artifact tabs you want to see in this
                  conversation.
                </p>
              </div>
              <AgentsArtifactTabCustomizer
                triggerVariant="button"
                tabs={customizerTabs}
                hiddenTabs={hiddenTabs}
                onHide={(tab) => onHideTab?.(tab, enabledAvailableTabIds)}
                onShow={(tab) => onShowTab?.(tab)}
              />
            </div>
          ) : (
            <ArtifactSelectionProvider
              enabled={artifactSelectionEnabled}
              onAddExcerpt={handleAddArtifactExcerpt}
            >
              <ArtifactContent
                activeTab={effectiveActiveTab}
                tasksEnabled={tasksEnabled}
                tasksSurfaceCapabilities={tasksSurfaceCapabilities}
                conversation={conversation}
                workspace={scopedWorkspace}
                isRemoteEnvironment={isRemoteEnvironment}
                conversationId={conversationId}
                activeWorkspaceFreshness={scopedLocalFreshness}
                conversationTitle={conversation?.title ?? null}
                automationId={automationId}
                isAutomationRunConversation={isAutomationRunConversation}
                {...(onOpenAutomation ? { onOpenAutomation } : {})}
                {...(onFocusAutomationRun ? { onFocusAutomationRun } : {})}
                projectBaseBranch={projectBaseBranch}
                isLoading={
                  conversationQuery.isLoading || sessionQuery.isLoading
                }
                attachedSessionId={attachedSessionId}
                projectId={conversationProjectId}
                canStartPlan={canStartPlan}
                session={session}
                sessionTitle={sessionData?.session.title ?? null}
                taskMode={taskMode}
                reviewArtifact={reviewArtifact}
                reviewRequestedChangesArtifact={
                  reviewRequestedChangesArtifact
                }
                reviewContext={
                  isReviewPrWorkspace ? null : workspaceReviewContext
                }
                isReviewPrWorkspace={isReviewPrWorkspace}
                autoApproveEnabled={autoApproveEnabled}
                isAutoApproveSaving={autoApproveMutation.isPending}
                onAutoApproveChange={(enabled) => {
                  if (!prReviewConversationId) return;
                  setAutoApprovePreference({
                    conversationId: prReviewConversationId,
                    enabled,
                  });
                  autoApproveMutation.mutate({
                    conversationId: prReviewConversationId,
                    enabled,
                  });
                }}
                prReviewMonitor={prReviewContext?.monitor ?? null}
                isPrReviewMonitorSaving={prReviewMonitoringMutation.isPending}
                onPrReviewMonitorChange={async (
                  enabled,
                  activeReviewPolicy,
                ) => {
                  if (!prReviewConversationId) {
                    throw new Error("PR review monitoring is unavailable");
                  }
                  await prReviewMonitoringMutation.mutateAsync({
                    conversationId: prReviewConversationId,
                    enabled,
                    ...(activeReviewPolicy ? { activeReviewPolicy } : {}),
                  });
                }}
                reviewStartResult={
                  isReviewPrWorkspace ? null : workspaceReviewStartResult
                }
                reviewStartError={
                  isReviewPrWorkspace
                    ? null
                    : (workspaceReviewStartError ??
                      workspaceReviewFixIssuesError)
                }
                isReviewLoading={
                  (Boolean(reviewArtifactId) &&
                    !reviewArtifact &&
                    reviewArtifactQuery.isFetching) ||
                  (Boolean(workspaceReviewRequestedChangesArtifactId) &&
                    !reviewRequestedChangesArtifact &&
                    reviewRequestedChangesArtifactQuery.isFetching)
                }
                isReviewActionPending={
                  isReviewPrWorkspace ? false : isWorkspaceReviewActionPending
                }
                isFixIssuesActionPending={
                  isReviewPrWorkspace
                    ? false
                    : isWorkspaceReviewFixIssuesPending
                }
                isApproveAnywayActionPending={
                  isReviewPrWorkspace
                    ? false
                    : isWorkspaceReviewApproveAnywayPending
                }
                isWorkspaceRuntimeGenerating={
                  isReviewPrWorkspace ? false : isWorkspaceRuntimeGenerating
                }
                onStartReview={
                  isReviewPrWorkspace ? () => {} : handleStartReview
                }
                {...(!isReviewPrWorkspace
                  ? { onStartReviewIntent: prefetchWorkspaceReview }
                  : {})}
                onFixIssues={
                  isReviewPrWorkspace ? () => {} : handleFixReviewIssues
                }
                onApproveAnyway={
                  isReviewPrWorkspace
                    ? async () => {}
                    : handleApproveReviewAnyway
                }
                {...(!isReviewPrWorkspace &&
                reviewDisplayContext?.monitor.reviewConversationId
                  ? { onViewTranscript: handleOpenReview }
                  : {})}
                planArtifact={planArtifact}
                isPlanLoading={isPlanHydrating}
                onPlanUpdated={handlePlanUpdated}
                onPlanSeeded={handlePlanSeeded}
                dependencyGraph={dependencyGraph}
                proposals={proposals}
                visibleImplementationTasks={visibleImplementationTasks}
                activeExecutionPlanId={activeExecutionPlanId}
                implementationTaskCounts={implementationTaskCounts}
                hasImplementationAttempt={hasImplementationAttempt}
                onPublishWorkspace={onPublishWorkspace}
                isPublishingWorkspace={isPublishingWorkspace}
                publishAttempt={publishAttempt}
                publishFocusRequest={publishFocusRequest}
                publishSubTab={publishSubTab}
                showPublishReviewTab={nestsWorkspaceReview}
                onPublishSubTabChange={handlePublishSubTabChange}
                reviewTabStatusColor={reviewTabIconColor}
                reviewTabStatusLabel={reviewTabStatusLabel}
                isReviewTabRunning={isWorkspaceReviewRunning}
                onConversationModeSwitched={onConversationModeSwitched}
                onFocusIdeationSessionForConversation={
                  onFocusIdeationSessionForConversation
                }
                onFocusVerificationSession={onFocusVerificationSession}
                {...(onFocusTaskRuntime ? { onFocusTaskRuntime } : {})}
                verificationState={verificationState}
                verificationInProgress={verificationInProgress}
                onOpenReview={handleOpenReview}
                onOpenPublish={handleOpenPublish}
                onOpenTasks={() => onTabChange("tasks")}
                taskArtifactSelectedId={taskArtifactSelectedId}
                onTaskArtifactSelectedIdChange={setTaskArtifactSelectedId}
                activeTeamRunId={activeTeamRunId}
              />
            </ArtifactSelectionProvider>
          )}
        </div>
      </aside>
      <WorkspaceReviewConfirmationDialog
        {...workspaceReviewConfirmationDialogProps}
      />
    </>
  );
});

type ArtifactContentProps = {
  activeTab: AgentArtifactTab;
  tasksEnabled: boolean;
  tasksSurfaceCapabilities: TasksSurfaceCapabilities;
  conversation: AgentConversation | null;
  workspace: AgentConversationWorkspace | null;
  isRemoteEnvironment: boolean;
  conversationId: string | null;
  activeWorkspaceFreshness: AgentConversationWorkspaceFreshness | undefined;
  conversationTitle: string | null;
  automationId: string | null;
  isAutomationRunConversation: boolean;
  onOpenAutomation?: (automationId: string) => void;
  onFocusAutomationRun?: (
    automationId: string,
    runId: string,
    conversationId: string,
    options?: AutomationRunFocusOptions,
  ) => void;
  projectBaseBranch: string | null;
  isLoading: boolean;
  attachedSessionId: string | null;
  projectId: string | null;
  canStartPlan: boolean;
  session: IdeationSession | null;
  sessionTitle: string | null;
  taskMode: AgentTaskArtifactMode;
  reviewArtifact: Artifact | null;
  reviewRequestedChangesArtifact: Artifact | null;
  reviewContext: AgentWorkspaceReviewContext | null;
  isReviewPrWorkspace: boolean;
  autoApproveEnabled: boolean;
  isAutoApproveSaving: boolean;
  onAutoApproveChange: (enabled: boolean) => void;
  prReviewMonitor: AgentWorkspacePrReviewContext["monitor"];
  isPrReviewMonitorSaving: boolean;
  onPrReviewMonitorChange: (
    enabled: boolean,
    activeReviewPolicy?: "finish_current" | "cancel_current",
  ) => Promise<void>;
  reviewStartResult: StartAgentWorkspaceReviewResult | null;
  reviewStartError: Error | null;
  isReviewLoading: boolean;
  isReviewActionPending: boolean;
  isFixIssuesActionPending: boolean;
  isApproveAnywayActionPending: boolean;
  isWorkspaceRuntimeGenerating: boolean;
  onStartReview: (force: boolean) => void;
  onStartReviewIntent?: () => void;
  onFixIssues: () => void;
  onApproveAnyway: () => Promise<void>;
  onViewTranscript?: () => void;
  planArtifact: Artifact | null;
  isPlanLoading: boolean;
  onPlanUpdated: (updatedPlan: Artifact) => void;
  onPlanSeeded: (result: AgentConversationPlanSeedResult) => void;
  dependencyGraph: DependencyGraphResponse | null;
  proposals: TaskProposal[];
  visibleImplementationTasks: readonly Task[];
  activeExecutionPlanId: string | null;
  implementationTaskCounts: StatusCounts;
  hasImplementationAttempt: boolean;
  onPublishWorkspace: ((conversationId: string) => Promise<void>) | undefined;
  isPublishingWorkspace: boolean;
  publishAttempt?: AgentWorkspacePublishAttempt | null;
  publishFocusRequest: AgentPublishFocusRequest | null;
  publishSubTab: AgentPublishSubTab;
  showPublishReviewTab: boolean;
  onPublishSubTabChange: (tab: AgentPublishSubTab) => void;
  reviewTabStatusColor: string | null;
  reviewTabStatusLabel: string | null;
  isReviewTabRunning: boolean;
  onConversationModeSwitched:
    | ((
        conversationId: string,
        mode: AgentConversationWorkspaceMode,
        workspace: AgentConversationWorkspace | null,
      ) => void)
    | undefined;
  onFocusIdeationSessionForConversation:
    ((conversationId: string, sessionId: string) => void) | undefined;
  onFocusVerificationSession:
    ((parentSessionId: string, childSessionId: string) => void) | undefined;
  onFocusTaskRuntime?: (
    taskId: string,
    contextType: AgentTaskRuntimeContextType,
  ) => void;
  verificationState: VerificationStatusResponse["status"] | null;
  verificationInProgress: boolean;
  onOpenReview: () => void;
  onOpenPublish: () => void;
  onOpenTasks: () => void;
  taskArtifactSelectedId: string | null;
  onTaskArtifactSelectedIdChange: (id: string | null) => void;
  activeTeamRunId: string | null;
};

function ArtifactContent({
  activeTab,
  tasksEnabled,
  tasksSurfaceCapabilities,
  conversation,
  workspace,
  isRemoteEnvironment,
  conversationId,
  activeWorkspaceFreshness,
  conversationTitle,
  automationId,
  isAutomationRunConversation,
  onOpenAutomation,
  onFocusAutomationRun,
  projectBaseBranch,
  isLoading,
  attachedSessionId,
  projectId,
  canStartPlan,
  session,
  sessionTitle,
  taskMode,
  reviewArtifact,
  reviewRequestedChangesArtifact,
  reviewContext,
  isReviewPrWorkspace,
  autoApproveEnabled,
  isAutoApproveSaving,
  onAutoApproveChange,
  prReviewMonitor,
  isPrReviewMonitorSaving,
  onPrReviewMonitorChange,
  reviewStartResult,
  reviewStartError,
  isReviewLoading,
  isReviewActionPending,
  isFixIssuesActionPending,
  isApproveAnywayActionPending,
  isWorkspaceRuntimeGenerating,
  onStartReview,
  onStartReviewIntent,
  onFixIssues,
  onApproveAnyway,
  onViewTranscript,
  planArtifact,
  isPlanLoading,
  onPlanUpdated,
  onPlanSeeded,
  dependencyGraph,
  proposals,
  visibleImplementationTasks,
  activeExecutionPlanId,
  implementationTaskCounts,
  hasImplementationAttempt,
  onPublishWorkspace,
  isPublishingWorkspace,
  publishAttempt,
  publishFocusRequest,
  publishSubTab,
  showPublishReviewTab,
  onPublishSubTabChange,
  reviewTabStatusColor,
  reviewTabStatusLabel,
  isReviewTabRunning,
  onConversationModeSwitched,
  onFocusIdeationSessionForConversation,
  onFocusVerificationSession: _onFocusVerificationSession,
  onFocusTaskRuntime,
  verificationState,
  verificationInProgress,
  onOpenReview,
  onOpenPublish,
  onOpenTasks,
  taskArtifactSelectedId,
  onTaskArtifactSelectedIdChange,
  activeTeamRunId,
}: ArtifactContentProps) {
  const reviewActionBlocker = getAgentWorkspaceReviewActionBlocker(workspace);
  const renderReviewPanel = (embedded: boolean) => (
    <AgentReviewPanel
      reviewArtifact={reviewArtifact}
      reviewRequestedChangesArtifact={reviewRequestedChangesArtifact}
      reviewContext={reviewContext}
      isReviewPrWorkspace={isReviewPrWorkspace}
      autoApproveEnabled={autoApproveEnabled}
      isAutoApproveSaving={isAutoApproveSaving}
      onAutoApproveChange={onAutoApproveChange}
      prReviewMonitor={prReviewMonitor}
      isPrReviewMonitorSaving={isPrReviewMonitorSaving}
      onPrReviewMonitorChange={onPrReviewMonitorChange}
      reviewStartResult={reviewStartResult}
      reviewStartError={reviewStartError}
      isReviewLoading={isReviewLoading}
      isReviewActionPending={isReviewActionPending}
      isFixIssuesActionPending={isFixIssuesActionPending}
      isApproveAnywayActionPending={isApproveAnywayActionPending}
      isWorkspaceRuntimeGenerating={isWorkspaceRuntimeGenerating}
      isPublishingWorkspace={isPublishingWorkspace}
      reviewActionBlocker={reviewActionBlocker}
      onOpenPublish={onOpenPublish}
      onStartReview={onStartReview}
      {...(onStartReviewIntent ? { onStartReviewIntent } : {})}
      onFixIssues={onFixIssues}
      onApproveAnyway={onApproveAnyway}
      embedded={embedded}
      {...(onViewTranscript ? { onViewTranscript } : {})}
    />
  );

  if (
    activeTab === "persona" &&
    conversation &&
    isPersonaArtifactConversation(conversation)
  ) {
    return (
      <Suspense fallback={<PersonaArtifactSkeletonFallback />}>
        <LazyPersonaArtifactPanel
          key={conversation.id}
          conversation={conversation}
        />
      </Suspense>
    );
  }

  if (activeTab === "team" && conversationId) {
    return (
      <AgentsTeamPanel
        conversationId={conversationId}
        projectId={projectId}
        activeAgentRunId={activeTeamRunId}
      />
    );
  }

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
          {...(onFocusAutomationRun ? { onFocusAutomationRun } : {})}
        />
      </Suspense>
    );
  }

  if (activeTab === "publish") {
    if (isRemoteEnvironment) {
      return <RemoteHostOnlyNotice subject="Workspace publishing and pull requests" />;
    }
    return (
      <AgentPublishPanel
        workspace={workspace}
        conversationTitle={conversationTitle}
        projectBaseBranch={projectBaseBranch}
        onPublishWorkspace={onPublishWorkspace}
        publishAttempt={publishAttempt ?? null}
        publishFocusRequest={publishFocusRequest}
        reviewContext={reviewContext}
        onOpenReview={onOpenReview}
        activeSubTab={publishSubTab}
        showReviewTab={showPublishReviewTab}
        onSubTabChange={onPublishSubTabChange}
        reviewContent={renderReviewPanel(true)}
        reviewTabStatusColor={reviewTabStatusColor}
        reviewTabStatusLabel={reviewTabStatusLabel}
        isReviewTabRunning={isReviewTabRunning}
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

  if (activeTab === "clickup") {
    return (
      <Suspense fallback={<EmptyArtifactState title="Loading ClickUp..." />}>
        <LazyAgentsClickUpIssuePanel conversationId={conversationId} />
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
      <Suspense
        fallback={<ArtifactLoadingState title="Loading pull request..." />}
      >
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
    return renderReviewPanel(false);
  }

  if (activeTab === "plan") {
    if ((isLoading && attachedSessionId) || isPlanLoading) {
      return <EmptyArtifactState title="Loading attached run..." />;
    }
    if (
      !planArtifact &&
      canStartPlan &&
      !isAutomationRunConversation &&
      conversationId &&
      projectId
    ) {
      return (
        <AgentPlanStartPanel
          conversationId={conversationId}
          projectId={projectId}
          onPlanSeeded={onPlanSeeded}
        />
      );
    }
    if (!attachedSessionId && !planArtifact) {
      return (
        <EmptyArtifactState
          title="No ideation run attached"
          detail="Start ideation from this agent chat to populate plan, verification, proposals, and tasks here."
        />
      );
    }
    return (
      <AgentPlanPanel
        tasksEnabled={tasksEnabled}
        workspace={workspace}
        activeWorkspaceFreshness={activeWorkspaceFreshness}
        session={session}
        sessionTitle={sessionTitle}
        planArtifact={planArtifact}
        isAutomationRunConversation={isAutomationRunConversation}
        isPlanLoading={isPlanLoading}
        proposals={proposals}
        dependencyGraph={dependencyGraph}
        visibleImplementationTasks={visibleImplementationTasks}
        activeExecutionPlanId={activeExecutionPlanId}
        implementationTaskCounts={implementationTaskCounts}
        hasImplementationAttempt={hasImplementationAttempt}
        onPlanUpdated={onPlanUpdated}
        verificationState={verificationState}
        verificationInProgress={verificationInProgress}
        onConversationModeSwitched={onConversationModeSwitched}
        onFocusIdeationSessionForConversation={
          onFocusIdeationSessionForConversation
        }
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

  return (
    <TaskArtifactSurface
      projectId={projectId}
      sessionId={attachedSessionId}
      mode={taskMode}
      selectedTaskId={taskArtifactSelectedId}
      onSelectedTaskIdChange={onTaskArtifactSelectedIdChange}
      capabilities={tasksSurfaceCapabilities}
      {...(onFocusTaskRuntime ? { onFocusTaskRuntime } : {})}
    />
  );
}

function AgentPlanPanel({
  tasksEnabled,
  workspace,
  activeWorkspaceFreshness,
  session,
  sessionTitle,
  planArtifact,
  isAutomationRunConversation,
  isPlanLoading,
  proposals,
  dependencyGraph,
  visibleImplementationTasks,
  activeExecutionPlanId,
  implementationTaskCounts,
  hasImplementationAttempt,
  onPlanUpdated,
  verificationState,
  verificationInProgress,
  onConversationModeSwitched,
  onFocusIdeationSessionForConversation,
  onOpenTasks,
}: {
  tasksEnabled: boolean;
  workspace: AgentConversationWorkspace | null;
  activeWorkspaceFreshness: AgentConversationWorkspaceFreshness | undefined;
  session: IdeationSession | null;
  sessionTitle: string | null;
  planArtifact: Artifact | null;
  isAutomationRunConversation: boolean;
  isPlanLoading: boolean;
  proposals: TaskProposal[];
  dependencyGraph: DependencyGraphResponse | null;
  visibleImplementationTasks: readonly Task[];
  activeExecutionPlanId: string | null;
  implementationTaskCounts: StatusCounts;
  hasImplementationAttempt: boolean;
  onPlanUpdated: (updatedPlan: Artifact) => void;
  verificationState: VerificationStatusResponse["status"] | null;
  verificationInProgress: boolean;
  onConversationModeSwitched:
    | ((
        conversationId: string,
        mode: AgentConversationWorkspaceMode,
        workspace: AgentConversationWorkspace | null,
      ) => void)
    | undefined;
  onFocusIdeationSessionForConversation:
    ((conversationId: string, sessionId: string) => void) | undefined;
  onOpenTasks: () => void;
}) {
  const planApproveGate = useAgentGate("planApprove");
  const planDirectImplementationGate = useAgentGate("planDirectImplementation");
  const planTaskPipelineGate = useAgentGate("planTaskPipeline");
  const executionPauseGate = useAgentGate("executionPlanPause");
  const executionResumeGate = useAgentGate("executionPlanResume");
  const executionStopGate = useAgentGate("executionPlanStop");
  const generatedPlanBundleTabsId = useId();
  const planBundleTabsId = `agents-plan-bundle-${generatedPlanBundleTabsId.replace(
    /:/g,
    "",
  )}`;
  const [isEditing, setIsEditing] = useState(false);
  const [isPlanExpanded, setIsPlanExpanded] = useState(true);
  const [planBodyMode, setPlanBodyMode] =
    useState<PlanDisplayBodyMode>("overview");
  const [exportDialogOpen, setExportDialogOpen] = useState(false);
  const [isApprovingPlan, setIsApprovingPlan] = useState(false);
  const [isStartingPlanVerification, setIsStartingPlanVerification] =
    useState(false);
  const [isImplementingPlanDirectly, setIsImplementingPlanDirectly] =
    useState(false);
  const [isStartingTasks, setIsStartingTasks] = useState(false);
  const [viewingProposalId, setViewingProposalId] = useState<string | null>(
    null,
  );
  const [viewingEnrichment, setViewingEnrichment] = useState<
    ProposalDetailEnrichment | undefined
  >(undefined);
  const queryClient = useQueryClient();
  const { registry: modelRegistry } = useAgentModels();
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
  const {
    confirmImplementDirectly,
    confirmCreateProposals,
    confirmationDialogProps: planContinuationDialogProps,
    ConfirmationDialog: PlanContinuationDialog,
  } = useApprovedPlanContinuation({
    conversationId: workspace?.conversationId ?? null,
    projectId: workspace?.projectId ?? session?.projectId ?? null,
  });

  useEffect(() => {
    setIsEditing(false);
    setIsPlanExpanded(true);
    setPlanBodyMode("overview");
    setViewingProposalId(null);
    setViewingEnrichment(undefined);
  }, [planArtifact?.id, planArtifact?.metadata.version, session?.id]);

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
  const pauseExecutionPlanMutation = useMutation({
    mutationFn: (input: {
      projectId: string;
      sessionId: string;
      executionPlanId?: string | null;
    }) => tasksApi.pauseExecutionPlan(input),
  });
  const resumeExecutionPlanMutation = useMutation({
    mutationFn: (input: {
      projectId: string;
      sessionId: string;
      executionPlanId?: string | null;
    }) => tasksApi.resumeExecutionPlan(input),
  });
  const stopExecutionPlanMutation = useMutation({
    mutationFn: (input: {
      projectId: string;
      sessionId: string;
      executionPlanId?: string | null;
    }) => tasksApi.stopExecutionPlan(input),
  });

  const handleCreateProposals = useCallback(() => {
    if (planTaskPipelineGate.gated || !session) return;
    let workspaceActivationCompleted = workspace?.mode === "tasks";
    let committedRuntimeOverride:
      | import("@/api/manual-role-defaults.types").ManualRoleRuntimeSelection
      | null = null;
    const perform = async (
      runtimeOverride?: import("@/api/manual-role-defaults.types").ManualRoleRuntimeSelection,
    ) => {
      const runtimeForAttempt = committedRuntimeOverride ?? runtimeOverride;
      try {
      await activateAgentPlanProposals({
        sessionId: session.id,
        workspace,
        queryClient,
        canPromoteWorkspace: session.sessionFlow === "planning",
        ...(onConversationModeSwitched ? { onConversationModeSwitched } : {}),
        ...(onFocusIdeationSessionForConversation
          ? { onFocusIdeationSessionForConversation }
          : {}),
        ...(runtimeForAttempt ? { runtimeOverride: runtimeForAttempt } : {}),
        workspaceActivationCompleted,
        onWorkspaceActivated: () => {
          workspaceActivationCompleted = true;
          if (runtimeForAttempt) {
            committedRuntimeOverride = { ...runtimeForAttempt };
          }
        },
      });
      } catch (err) {
        console.error("Failed to create proposals:", err);
        toast.error("Failed to request proposal creation");
        throw err;
      }
    };
    if (workspace?.mode === "plan") {
      void confirmCreateProposals((runtimeOverride) => perform(runtimeOverride));
    } else {
      void perform();
    }
  }, [
    onConversationModeSwitched,
    onFocusIdeationSessionForConversation,
    planTaskPipelineGate.gated,
    queryClient,
    session,
    workspace,
    confirmCreateProposals,
  ]);

  const isPlanningSession = session?.sessionFlow === "planning";
  const isOwnedCurrentPlan = Boolean(
    isPlanningSession &&
    session?.planArtifactId &&
    planArtifact?.id === session.planArtifactId,
  );
  const isPlanBundleComplete =
    planArtifact?.planContractVersion !== 2 || Boolean(planArtifact.blueprint);
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
    isPlanBundleComplete &&
    planApprovalStatus === "draft";
  const canShowApprovedPlanActions =
    canShowPlanModeControls && !isImplementingPlanDirectly;
  const canShowManualPlanContinuationActions =
    canShowApprovedPlanActions && !isAutomationRunConversation;
  const canRetryTaskDecomposition = Boolean(
    tasksEnabled &&
    workspace?.mode === "tasks" &&
    session !== null &&
    workspace.taskPipelineSessionId === session.id &&
    !hasImplementationAttempt,
  );
  const isPlanVerificationSatisfied = verificationState === "verified";
  const canVerifyPlan =
    canShowApprovedPlanActions &&
    isOwnedCurrentPlan &&
    isPlanBundleComplete &&
    verificationState !== null;
  const canCreateProposals =
    (canShowManualPlanContinuationActions || canRetryTaskDecomposition) &&
    session !== null &&
    isPlanBundleComplete &&
    (!isPlanningSession || isPlanApproved) &&
    tasksEnabled;
  const canImplementDirectly = Boolean(
    canShowManualPlanContinuationActions &&
    isOwnedCurrentPlan &&
    isPlanBundleComplete &&
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
      planArtifact?.blueprint?.id,
      planArtifact?.blueprint?.metadata.version,
    ],
    queryFn: () => artifactApi.getPlanComplexityAssessment(session!.id),
    enabled: Boolean(
      tasksEnabled &&
      session &&
      isOwnedCurrentPlan &&
      isPlanApproved &&
      canShowManualPlanContinuationActions,
    ),
    staleTime: 5_000,
    refetchInterval: (query) => (query.state.data ? false : 4_000),
  });
  const isPlanRecommendationPending =
    tasksEnabled &&
    isPlanRecommendationCheckPending({
      assessment: planComplexityQuery.data,
      isFetching:
        (planComplexityQuery.isFetching || planComplexityQuery.isLoading) &&
        !planComplexityQuery.data,
      approvedAt: planArtifact?.planApproval?.approvedAt,
    });
  const planActionHint =
    !tasksEnabled && isPlanApproved
      ? "Tasks is off. Implement this approved plan directly."
      : buildPlanActionHint({
          assessment: planComplexityQuery.data,
          isAssessing: isPlanRecommendationPending,
          canChoose: canImplementDirectly && canCreateProposals,
        });
  const primaryPlanAction = tasksEnabled
    ? planComplexityQuery.data?.recommendedAction
    : "implement_directly";
  const isAcceptedPlan = session?.status === "accepted";
  const planRuntimeControlCounts = useMemo(
    () => getPlanRuntimeControlCounts(visibleImplementationTasks),
    [visibleImplementationTasks],
  );
  const canRestartImplementation = Boolean(
    isAcceptedPlan &&
    implementationTaskCounts.total > 0 &&
    session?.id &&
    tasksEnabled,
  );
  const canPauseExecutionPlan = Boolean(
    isAcceptedPlan &&
    session?.id &&
    session.projectId &&
    planRuntimeControlCounts.running > 0,
  );
  const canStopExecutionPlan = canPauseExecutionPlan;
  const canResumeExecutionPlan = Boolean(
    isAcceptedPlan &&
    session?.id &&
    session.projectId &&
    tasksEnabled &&
    planRuntimeControlCounts.running === 0 &&
    planRuntimeControlCounts.paused > 0,
  );
  const isExecutionPlanControlPending =
    pauseExecutionPlanMutation.isPending ||
    resumeExecutionPlanMutation.isPending ||
    stopExecutionPlanMutation.isPending;
  const workspaceConversationId = workspace?.conversationId ?? null;

  const handleApprovePlan = useCallback(async () => {
    if (planApproveGate.gated || !session || !planArtifact || !canApprovePlan) {
      return;
    }
    setIsApprovingPlan(true);
    try {
      const approvedPlan = await artifactApi.approvePlanArtifact({
        sessionId: session.id,
        artifactId: planArtifact.id,
        ...(planArtifact.blueprint && {
          blueprintArtifactId: planArtifact.blueprint.id,
          blueprintArtifactVersion: planArtifact.blueprint.metadata.version,
        }),
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
      if (
        err instanceof RemotePlanIntentError &&
        err.errorCode === "REMOTE_PLAN_APPROVAL_PLAN_CHANGED"
      ) {
        await queryClient.invalidateQueries({
          queryKey: ["agents", "session-plan", session.id],
        });
      }
      toast.error(
        err instanceof Error ? err.message : "Failed to approve plan",
      );
    } finally {
      setIsApprovingPlan(false);
    }
  }, [canApprovePlan, onPlanUpdated, planApproveGate.gated, planArtifact, queryClient, session]);

  const handleImplementDirectly = useCallback(() => {
    if (
      planDirectImplementationGate.gated ||
      !session ||
      !workspace?.conversationId ||
      !canImplementDirectly
    ) {
      return;
    }
    let pinnedActivation: DirectImplementationActivationSnapshot | undefined;
    let committedRuntimeOverride:
      | import("@/api/manual-role-defaults.types").ManualRoleRuntimeSelection
      | null = null;
    void confirmImplementDirectly(async (runtimeOverride) => {
      const runtimeForAttempt = committedRuntimeOverride ?? runtimeOverride;
      setIsImplementingPlanDirectly(true);
      try {
        await implementAgentPlanDirectly({
          projectId: session.projectId,
          workspace: pinnedActivation?.workspace ?? workspace,
          queryClient,
          ...(onConversationModeSwitched ? { onConversationModeSwitched } : {}),
          ...(pinnedActivation ? { pinnedActivation } : {}),
          onActivated: (snapshot) => {
            if (!pinnedActivation) {
              pinnedActivation = snapshot;
              committedRuntimeOverride = { ...runtimeForAttempt };
            }
          },
          sendOptions: { runtimeOverride: runtimeForAttempt },
        });
        useAgentSessionStore
          .getState()
          .setRuntimeForConversation(
            workspace.conversationId,
            session.projectId,
            materializeWorkspaceRuntimeSelection(runtimeForAttempt, modelRegistry),
          );
        useAgentSessionStore
          .getState()
          .setServiceTierForConversation(
            workspace.conversationId,
            runtimeForAttempt.serviceTier,
          );
        toast.success("Implementation started");
      } catch (err) {
        console.error("Failed to implement plan directly:", err);
        if (!(err instanceof PlanContinuationCommittedError)) {
          toast.error(
            err instanceof Error ? err.message : "Failed to start implementation",
          );
        }
        throw err;
      } finally {
        setIsImplementingPlanDirectly(false);
      }
    });
  }, [
    canImplementDirectly,
    planDirectImplementationGate.gated,
    confirmImplementDirectly,
    modelRegistry,
    onConversationModeSwitched,
    queryClient,
    session,
    workspace,
  ]);

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
    if (isPlanVerificationSatisfied) {
      const confirmed = await confirm({
        title: "Verify this plan again?",
        description:
          "The current plan is already verified. This queues another visible review turn and keeps the existing proof unless the plan changes.",
        confirmText: "Verify again",
      });
      if (!confirmed) {
        return;
      }
    }
    setIsStartingPlanVerification(true);
    try {
      await verificationApi.confirm(session.id);
      await Promise.all([
        queryClient.invalidateQueries({
          queryKey: verificationStatusKey(session.id),
        }),
        queryClient.invalidateQueries({
          queryKey: ideationKeys.sessionWithData(session.id),
        }),
        queryClient.invalidateQueries({ queryKey: ideationKeys.sessions() }),
      ]);
      toast.success("Verify Plan queued in this conversation");
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
    confirm,
    isPlanVerificationSatisfied,
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
        "The accepted plan will remain unchanged. Running work will stop, and the current implementation attempt, Kanban tasks, and uncommitted implementation changes will be discarded. RalphX will close or reconcile any existing PR, reset the branch to the latest fetched base, and create fresh tasks.",
      confirmText: "Restart Implementation",
      pendingText: "Restarting…",
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

  const invalidateExecutionPlanControlQueries = useCallback(async () => {
    if (!session) {
      return;
    }
    await Promise.all([
      queryClient.invalidateQueries({
        queryKey: ideationKeys.sessionWithData(session.id),
      }),
      queryClient.invalidateQueries({
        queryKey: ideationKeys.sessions(),
      }),
      queryClient.invalidateQueries({ queryKey: taskKeys.lists() }),
      ...(workspaceConversationId
        ? [invalidateWorkspaceQueries(queryClient, workspaceConversationId)]
        : []),
    ]);
    await loadActivePlan(session.projectId);
  }, [loadActivePlan, queryClient, session, workspaceConversationId]);

  const handlePauseExecutionPlan = useCallback(() => {
    if (!session || !canPauseExecutionPlan || executionPauseGate.gated) {
      return;
    }

    void confirm({
      title: "Pause this implementation plan?",
      description:
        "Running work for this plan will pause and queued work for this plan will wait until you resume it. Other project work will continue.",
      confirmText: "Pause Plan",
      pendingText: "Pausing...",
      onConfirm: async () => {
        try {
          await pauseExecutionPlanMutation.mutateAsync({
            projectId: session.projectId,
            sessionId: session.id,
            executionPlanId: activeExecutionPlanId,
          });
          await invalidateExecutionPlanControlQueries();
          toast.success("Plan paused");
        } catch (err) {
          toast.error(extractErrorMessage(err, "Failed to pause plan"));
          throw err;
        }
      },
    });
  }, [
    activeExecutionPlanId,
    canPauseExecutionPlan,
    confirm,
    invalidateExecutionPlanControlQueries,
    pauseExecutionPlanMutation,
    executionPauseGate.gated,
    session,
  ]);

  const handleResumeExecutionPlan = useCallback(() => {
    if (!session || !canResumeExecutionPlan || executionResumeGate.gated) {
      return;
    }

    void confirm({
      title: "Resume this implementation plan?",
      description:
        "Paused work for this plan will resume using the same scheduler and capacity limits as the execution bar. Other project work is unchanged.",
      confirmText: "Resume Plan",
      pendingText: "Resuming...",
      onConfirm: async () => {
        try {
          await resumeExecutionPlanMutation.mutateAsync({
            projectId: session.projectId,
            sessionId: session.id,
            executionPlanId: activeExecutionPlanId,
          });
          await invalidateExecutionPlanControlQueries();
          toast.success("Plan resumed");
        } catch (err) {
          toast.error(extractErrorMessage(err, "Failed to resume plan"));
          throw err;
        }
      },
    });
  }, [
    activeExecutionPlanId,
    canResumeExecutionPlan,
    confirm,
    invalidateExecutionPlanControlQueries,
    resumeExecutionPlanMutation,
    executionResumeGate.gated,
    session,
  ]);

  const handleStopExecutionPlan = useCallback(() => {
    if (!session || !canStopExecutionPlan || executionStopGate.gated) {
      return;
    }

    void confirm({
      title: "Stop this implementation plan?",
      description:
        "Running work for this plan will stop and queued work for this plan will not continue automatically. Other project work will continue.",
      confirmText: "Stop Plan",
      pendingText: "Stopping...",
      variant: "destructive",
      onConfirm: async () => {
        try {
          await stopExecutionPlanMutation.mutateAsync({
            projectId: session.projectId,
            sessionId: session.id,
            executionPlanId: activeExecutionPlanId,
          });
          await invalidateExecutionPlanControlQueries();
          toast.success("Plan stopped");
        } catch (err) {
          toast.error(extractErrorMessage(err, "Failed to stop plan"));
          throw err;
        }
      },
    });
  }, [
    activeExecutionPlanId,
    canStopExecutionPlan,
    confirm,
    invalidateExecutionPlanControlQueries,
    session,
    stopExecutionPlanMutation,
    executionStopGate.gated,
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
    if (
      workspace?.mode === "plan" &&
      isOwnedCurrentPlan &&
      planApprovalStatus === "draft"
    ) {
      return "needs_approval";
    }
    return null;
  }, [
    hasImplementationAttempt,
    isOwnedCurrentPlan,
    isPlanApproved,
    planApprovalStatus,
    planArtifact,
    workspace?.mode,
  ]);
  const showCreateProposalsLifecycleAction = Boolean(
    canCreateProposals && linkedProposalsCount === 0,
  );
  const taskGraphValidation = useMemo(
    () => validateDependencyGraph(proposals, dependencyGraph),
    [dependencyGraph, proposals],
  );
  const canStartTasks = Boolean(
    tasksEnabled &&
    workspace?.mode === "tasks" &&
    session?.id &&
    workspace.taskPipelineSessionId === session.id &&
    proposals.length > 0 &&
    taskGraphValidation.isComplete &&
    !hasImplementationAttempt,
  );
  const handleStartTasks = useCallback(async () => {
    if (
      planTaskPipelineGate.gated ||
      !canStartTasks ||
      !session ||
      !workspace?.conversationId
    ) {
      return;
    }
    setIsStartingTasks(true);
    try {
      const result = await chatApi.startAgentTaskPipeline({
        conversationId: workspace.conversationId,
        sessionId: session.id,
        proposalIds: proposals.map((proposal) => proposal.id),
      });
      await Promise.all([
        queryClient.invalidateQueries({
          queryKey: ideationKeys.sessionWithData(session.id),
        }),
        queryClient.invalidateQueries({ queryKey: taskKeys.lists() }),
        invalidateWorkspaceQueries(queryClient, workspace.conversationId),
      ]);
      toast.success(
        "Started " +
          result.tasksCreated +
          " task" +
          (result.tasksCreated === 1 ? "" : "s"),
      );
    } catch (err) {
      toast.error(extractErrorMessage(err, "Failed to start tasks"));
    } finally {
      setIsStartingTasks(false);
    }
  }, [
    canStartTasks,
    planTaskPipelineGate.gated,
    proposals,
    queryClient,
    session,
    workspace,
  ]);
  const planLifecycleActions = useMemo<PlanLifecycleAction[]>(() => {
    if (!planLifecycleState || planLifecycleState === "accepted") {
      return [];
    }

    const actions: PlanLifecycleAction[] = [];
    if (canStartTasks) {
      actions.push({
        key: "start-tasks",
        label: isStartingTasks
          ? "Starting..."
          : "Start Tasks (" + proposals.length + ")",
        onClick: () => {
          void handleStartTasks();
        },
        icon: Play,
        disabled: planTaskPipelineGate.gated,
        loading: isStartingTasks,
        primary: true,
        testId: "plan-lifecycle-start-tasks-button",
      });
      return actions;
    }
    const verifyPending = isStartingPlanVerification || verificationInProgress;
    const verifyAction = canVerifyPlan
      ? ({
          key: "verify",
          label: verifyPending
            ? "Verifying..."
            : isPlanVerificationSatisfied
              ? "Verified"
              : "Verify Plan",
          onClick: () => {
            void handleVerifyPlan();
          },
          icon: ShieldCheck,
          disabled: isPlanRecommendationPending,
          loading: verifyPending,
          tone: isPlanVerificationSatisfied ? "success" : "default",
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
          disabled: planApproveGate.gated,
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

    const createAction: PlanLifecycleAction | null =
      showCreateProposalsLifecycleAction
        ? ({
            key: "create-proposals",
            label: "Create Proposals",
            onClick: () => {
              void handleCreateProposals();
            },
            icon: ListPlus,
            disabled: isPlanRecommendationPending || planTaskPipelineGate.gated,
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
          label: isImplementingPlanDirectly
            ? "Starting..."
            : "Implement Directly",
          onClick: () => {
            void handleImplementDirectly();
          },
          icon: Rocket,
          disabled:
            isPlanRecommendationPending || planDirectImplementationGate.gated,
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
    canStartTasks,
    canImplementDirectly,
    canVerifyPlan,
    handleApprovePlan,
    handleCreateProposals,
    handleImplementDirectly,
    handleStartTasks,
    handleVerifyPlan,
    isApprovingPlan,
    isImplementingPlanDirectly,
    isPlanRecommendationPending,
    isPlanVerificationSatisfied,
    isStartingPlanVerification,
    isStartingTasks,
    planLifecycleState,
    planApproveGate.gated,
    planDirectImplementationGate.gated,
    planTaskPipelineGate.gated,
    primaryPlanAction,
    proposals.length,
    showCreateProposalsLifecycleAction,
    verificationInProgress,
  ]);
  const acceptedFooterActions = useMemo<PlanLifecycleAction[]>(() => {
    if (planLifecycleState !== "accepted") {
      return [];
    }

    const disabled =
      isExecutionPlanControlPending || restartImplementationMutation.isPending;
    const actions: PlanLifecycleAction[] = [];
    if (canResumeExecutionPlan) {
      actions.push({
        key: "resume-plan",
        label: resumeExecutionPlanMutation.isPending ? "Resuming..." : "Resume",
        onClick: handleResumeExecutionPlan,
        icon: Play,
        disabled: disabled || executionResumeGate.gated,
        disabledReason: executionResumeGate.reason,
        loading: resumeExecutionPlanMutation.isPending,
        primary: true,
        testId: "plan-lifecycle-resume-button",
      });
    }
    if (canPauseExecutionPlan) {
      actions.push({
        key: "pause-plan",
        label: pauseExecutionPlanMutation.isPending ? "Pausing..." : "Pause",
        onClick: handlePauseExecutionPlan,
        icon: Pause,
        disabled: disabled || executionPauseGate.gated,
        disabledReason: executionPauseGate.reason,
        loading: pauseExecutionPlanMutation.isPending,
        testId: "plan-lifecycle-pause-button",
      });
    }
    if (canStopExecutionPlan) {
      actions.push({
        key: "stop-plan",
        label: stopExecutionPlanMutation.isPending ? "Stopping..." : "Stop",
        onClick: handleStopExecutionPlan,
        icon: Square,
        disabled: disabled || executionStopGate.gated,
        disabledReason: executionStopGate.reason,
        loading: stopExecutionPlanMutation.isPending,
        tone: "danger",
        testId: "plan-lifecycle-stop-button",
      });
    }
    return actions;
  }, [
    canPauseExecutionPlan,
    canResumeExecutionPlan,
    canStopExecutionPlan,
    executionPauseGate.gated,
    executionPauseGate.reason,
    executionResumeGate.gated,
    executionResumeGate.reason,
    executionStopGate.gated,
    executionStopGate.reason,
    handlePauseExecutionPlan,
    handleResumeExecutionPlan,
    handleStopExecutionPlan,
    isExecutionPlanControlPending,
    pauseExecutionPlanMutation.isPending,
    planLifecycleState,
    restartImplementationMutation.isPending,
    resumeExecutionPlanMutation.isPending,
    stopExecutionPlanMutation.isPending,
  ]);
  const planLifecycleDescription =
    planLifecycleState === "accepted"
      ? "Implementation work is attached to this plan."
      : planLifecycleState === "approved"
        ? (planActionHint ??
          (workspace?.mode === "plan"
            ? "Choose the next step for this approved plan."
            : "This approved plan is guiding the current workspace agent."))
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
  const selectedPlanArtifact =
    planBodyMode === "blueprint" && planArtifact?.blueprint
      ? planArtifact.blueprint
      : planArtifact;

  return (
    <div className="min-h-full px-4 pb-4 pt-4">
      {planArtifact ? (
        isEditing ? (
          <Suspense
            fallback={<EmptyArtifactState title="Loading plan editor..." />}
          >
            <LazyPlanEditor
              plan={selectedPlanArtifact ?? planArtifact}
              onSave={(updated) => {
                if (planBodyMode === "blueprint" && session) {
                  queryClient.setQueryData(
                    ["agents", "artifact", updated.id],
                    updated,
                  );
                  void queryClient.invalidateQueries({
                    queryKey: ["agents", "session-plan", session.id],
                  });
                  void queryClient.invalidateQueries({
                    queryKey: ["agents", "plan-approval", session.id],
                  });
                } else {
                  onPlanUpdated(updated);
                }
                setIsEditing(false);
              }}
              onCancel={() => setIsEditing(false)}
            />
          </Suspense>
        ) : (
          <>
            {planLifecycleState && (
              <>
                {planLifecycleState === "approved" &&
                  (planDirectImplementationGate.status === "unavailable" ||
                    planTaskPipelineGate.status === "unavailable") && (
                    <RemoteHostOnlyNotice subject="Plan implementation continuations" />
                  )}
                <PlanLifecycleBanner
                  state={planLifecycleState}
                  title={planLifecycleTitle}
                  description={planLifecycleDescription}
                  actions={planLifecycleActions}
                  {...(planLifecycleState === "accepted" && {
                    counts: implementationTaskCounts,
                    acceptedRuntimeCounts: planRuntimeControlCounts,
                    acceptedFooterActions,
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
              </>
            )}
            {isAutomationRunConversation ? (
              <div
                className="rounded-md px-3 py-2 text-xs"
                style={{
                  backgroundColor: "var(--bg-surface)",
                  borderColor: "var(--border-default)",
                  borderStyle: "solid",
                  borderWidth: "1px",
                  color: "var(--text-secondary)",
                }}
              >
                RalphX continues this run automatically after approval.
              </div>
            ) : null}
            <Suspense fallback={<EmptyArtifactState title="Loading plan..." />}>
              <LazyPlanDisplay
                plan={selectedPlanArtifact ?? planArtifact}
                linkedProposalsCount={linkedProposalsCount}
                bodyMode={planBodyMode}
                bodyTabsIdPrefix={planBundleTabsId}
                hideBody={
                  planBodyMode === "proposals" ||
                  (planBodyMode === "blueprint" && !planArtifact.blueprint)
                }
                onBodyModeChange={setPlanBodyMode}
                onEdit={() => setIsEditing(true)}
                onExport={() => setExportDialogOpen(true)}
                {...(planReferenceSessionId &&
                  !isAutomationRunConversation &&
                  planBodyMode === "overview" && {
                    onStartNewConversationWithPlan:
                      handleStartNewConversationWithPlan,
                    disableHistoricalNewConversation:
                      planArtifact.planContractVersion === 2,
                  })}
                isExpanded={isPlanExpanded}
                onExpandedChange={setIsPlanExpanded}
                chromeless
              />
            </Suspense>
            {planBodyMode === "blueprint" && !planArtifact.blueprint ? (
              <div
                id={planBundlePanelId(
                  planBundleTabsId,
                  "blueprint",
                )}
                role="tabpanel"
                aria-labelledby={planBundleTabId(
                  planBundleTabsId,
                  "blueprint",
                )}
                tabIndex={0}
              >
                <EmptyArtifactState
                  title={
                    planArtifact.planContractVersion === 1
                      ? "Blueprint not created for this legacy plan"
                      : "Implementation blueprint is not available yet"
                  }
                />
              </div>
            ) : null}
            {planBodyMode === "proposals" ? (
              <>
                <div
                  id={planBundlePanelId(
                    planBundleTabsId,
                    "proposals",
                  )}
                  role="tabpanel"
                  aria-labelledby={planBundleTabId(
                    planBundleTabsId,
                    "proposals",
                  )}
                  tabIndex={0}
                >
                  {session && proposals.length > 0 ? (
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
                  ) : (
                    <EmptyArtifactState title="No linked proposals" />
                  )}
                </div>
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
            ) : null}
            <ConfirmationDialog {...confirmationDialogProps} />
            <PlanContinuationDialog {...planContinuationDialogProps} />
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
            overviewArtifact={planArtifact}
            blueprintArtifact={planArtifact?.blueprint ?? null}
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
  capabilities,
}: {
  projectId: string | null;
  sessionId: string;
  mode: AgentTaskArtifactMode;
  selectedTaskId: string | null;
  onSelectedTaskIdChange: (id: string | null) => void;
  onFocusTaskRuntime?: (
    taskId: string,
    contextType: AgentTaskRuntimeContextType,
  ) => void;
  capabilities: TasksSurfaceCapabilities;
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
        readOnly={capabilities.isReadOnly}
        {...(onFocusTaskRuntime ? { onFocusTaskRuntime } : {})}
      />
    </Suspense>
  ) : null;

  const readOnlyBanner = capabilities.isReadOnly ? (
    <div
      data-testid="tasks-read-only-banner"
      className="shrink-0 px-4 py-2 text-sm"
      style={{
        backgroundColor: "var(--status-warning-muted)",
        borderBottomColor: "var(--status-warning)",
        borderBottomStyle: "solid",
        borderBottomWidth: "1px",
        color: "var(--text-primary)",
      }}
    >
      {capabilities.reason === "history_unavailable"
        ? "Task history could not be checked. History remains visible in read-only mode while you retry."
        : capabilities.reason === "tasks_draining"
          ? "Tasks are shutting down. Existing history is read-only while active work is paused."
          : "Tasks are off. Existing task history is available here in read-only mode."}
    </div>
  ) : null;

  if (mode === "kanban") {
    return (
      <div className="relative flex h-full min-h-[520px] flex-col overflow-hidden bg-[var(--bg-base)]">
        {readOnlyBanner}
        <Suspense
          fallback={<EmptyArtifactState title="Loading task board..." />}
        >
          <LazyTaskBoard
            projectId={projectId}
            ideationSessionId={sessionId}
            onTaskSelect={handleTaskSelect}
            fillWidth
            readOnly={capabilities.isReadOnly}
          />
        </Suspense>
        {detailOverlay}
      </div>
    );
  }

  return (
    <div className="relative flex h-full min-h-[520px] flex-col overflow-hidden bg-[var(--bg-base)]">
      {readOnlyBanner}
      <Suspense fallback={<EmptyArtifactState title="Loading task graph..." />}>
        <LazyTaskGraphView
          projectId={projectId}
          ideationSessionId={sessionId}
          hidePlanSelector
          onTaskSelect={handleTaskSelect}
          readOnly={capabilities.isReadOnly}
        />
      </Suspense>
      {detailOverlay}
    </div>
  );
}
