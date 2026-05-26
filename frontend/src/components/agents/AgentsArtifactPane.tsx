import {
  CheckCircle2,
  FileText,
  GitPullRequestArrow,
  LayoutGrid,
  Network,
  ClipboardList,
  X,
} from "lucide-react";
import type { ElementType } from "react";
import { lazy, memo, Suspense, useCallback, useEffect, useMemo, useState } from "react";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";

import { artifactApi } from "@/api/artifact";
import { ideationApi, toTaskProposal } from "@/api/ideation";
import { verificationApi } from "@/api/verification";
import {
  chatApi,
  type AgentConversationWorkspace,
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
import {
  getVisibleIdeationArtifactTabs,
  type IdeationArtifactTab,
} from "./agentArtifactTabs";
import { getLatestIdeationChildId } from "./agentIdeationChildren";
import { resolveAttachedIdeationSessionId } from "./attachedIdeationSession";
import type { ProposalDetailEnrichment } from "@/components/Ideation/ProposalDetailSheet";
import { EmptyArtifactState } from "./AgentsArtifactEmptyState";
import { AgentPublishPanel } from "./AgentsPublishPanel";
import { shouldShowAgentWorkspacePublishSurface } from "./agentWorkspacePublishState";
import type { AgentPublishFocusRequest } from "./agentPublishFocus";
import {
  agentWorkspaceKeys,
  invalidateWorkspaceQueries,
} from "./agentWorkspaceQueries";
import {
  buildPlanActionHint,
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

const ARTIFACT_TABS: Array<{
  id: IdeationArtifactTab;
  label: string;
  icon: ElementType;
}> = [
  { id: "plan", label: "Plan", icon: FileText },
  { id: "verification", label: "Verification", icon: CheckCircle2 },
  { id: "proposal", label: "Proposals", icon: GitPullRequestArrow },
  { id: "tasks", label: "Tasks", icon: ClipboardList },
];

const PUBLISH_TAB = {
  id: "publish" as const,
  label: "Commit & Publish",
  icon: GitPullRequestArrow,
};

const SELECTED_TASK_STORAGE_PREFIX = "agents:artifact:selected-task:";

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
  projectBaseBranch?: string | null;
  focusedIdeationSessionId?: string | null;
  activeTab: AgentArtifactTab;
  taskMode: AgentTaskArtifactMode;
  onTabChange: (tab: AgentArtifactTab) => void;
  onTaskModeChange: (mode: AgentTaskArtifactMode) => void;
  onPublishWorkspace: ((conversationId: string) => Promise<void>) | undefined;
  isPublishingWorkspace?: boolean;
  publishFocusRequest?: AgentPublishFocusRequest | null;
  onFocusVerificationSession: ((parentSessionId: string, childSessionId: string) => void) | undefined;
  onClose: () => void;
}

export const AgentsArtifactPane = memo(function AgentsArtifactPane({
  conversation,
  workspace = null,
  projectBaseBranch = null,
  focusedIdeationSessionId = null,
  activeTab,
  taskMode,
  onTabChange,
  onTaskModeChange,
  onPublishWorkspace,
  isPublishingWorkspace = false,
  publishFocusRequest = null,
  onFocusVerificationSession,
  onClose,
}: AgentsArtifactPaneProps) {
  const queryClient = useQueryClient();
  const canHydrateIdeationArtifacts = Boolean(
    focusedIdeationSessionId ||
      workspace?.mode === "ideation" ||
      workspace?.mode === "plan" ||
      workspace?.linkedIdeationSessionId ||
      workspace?.linkedPlanBranchId,
  );
  const showPublishTab = shouldShowAgentWorkspacePublishSurface(workspace);
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
  const [displayedVerificationStatus, setDisplayedVerificationStatus] = useState<{
    status: VerificationStatus;
    inProgress: boolean;
  } | null>(null);
  const conversationId = conversation?.id ?? null;
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
    },
    [conversationId],
  );
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
  const availableIdeationTabIds = useMemo(
    () =>
      getVisibleIdeationArtifactTabs({
        hasAttachedIdeationSession: Boolean(sessionData),
        hasPlanArtifact: Boolean(planArtifactId),
        hasExecutionTasks: Boolean(
          workspace?.linkedPlanBranchId ||
            sessionData?.session.acceptanceStatus === "accepted" ||
            sessionData?.session.convertedAt,
        ),
      }),
    [
      planArtifactId,
      sessionData,
      workspace?.linkedPlanBranchId,
    ],
  );
  const visibleTabs = useMemo(
    () => [
      ...ARTIFACT_TABS.filter((tab) => availableIdeationTabIds.includes(tab.id)),
      ...(showPublishTab ? [PUBLISH_TAB] : []),
    ],
    [availableIdeationTabIds, showPublishTab],
  );
  const effectiveActiveTab =
    visibleTabs.some((tab) => tab.id === activeTab)
      ? activeTab
      : showPublishTab
        ? "publish"
        : "plan";
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
  const verificationChildrenQuery = useQuery({
    queryKey: ["childSessions", attachedSessionId, "verification"],
    queryFn: () => ideationApi.sessions.getChildren(attachedSessionId!, "verification"),
    enabled: shouldLoadVerificationData && !!attachedSessionId,
    staleTime: 4_000,
  });
  const dependencyQuery = useDependencyGraph(
    shouldLoadDependencyGraph ? attachedSessionId ?? "" : "",
  );
  const verificationData =
    attachedSessionId && verificationQuery.data?.sessionId === attachedSessionId
      ? verificationQuery.data
      : null;
  const dependencyGraph = attachedSessionId && sessionData ? dependencyQuery.data ?? null : null;
  const proposalCount = proposals.length;
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
  const latestVerificationChildId = useMemo(
    () => getLatestIdeationChildId(verificationChildrenQuery.data),
    [verificationChildrenQuery.data],
  );
  useEffect(() => {
    if (
      effectiveActiveTab !== "verification" ||
      !attachedSessionId ||
      !latestVerificationChildId
    ) {
      return;
    }
    onFocusVerificationSession?.(attachedSessionId, latestVerificationChildId);
  }, [
    attachedSessionId,
    effectiveActiveTab,
    latestVerificationChildId,
    onFocusVerificationSession,
  ]);
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
          projectBaseBranch={projectBaseBranch}
          isLoading={conversationQuery.isLoading || sessionQuery.isLoading}
          attachedSessionId={attachedSessionId}
          projectId={conversation?.projectId ?? null}
          session={session}
          sessionTitle={sessionData?.session.title ?? null}
          taskMode={taskMode}
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
  projectBaseBranch: string | null;
  isLoading: boolean;
  attachedSessionId: string | null;
  projectId: string | null;
  session: IdeationSession | null;
  sessionTitle: string | null;
  taskMode: AgentTaskArtifactMode;
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
  projectBaseBranch,
  isLoading,
  attachedSessionId,
  projectId,
  session,
  sessionTitle,
  taskMode,
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
        projectBaseBranch={projectBaseBranch}
        onPublishWorkspace={onPublishWorkspace}
        isPublishingWorkspace={isPublishingWorkspace}
        publishFocusRequest={publishFocusRequest}
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
  const canApprovePlan = isOwnedCurrentPlan && planApprovalStatus === "draft";
  const isPlanVerificationSatisfied =
    verificationState === "verified" || verificationState === "imported_verified";
  const canVerifyPlan =
    isOwnedCurrentPlan && isPlanApproved && !isPlanVerificationSatisfied;
  const canCreateProposals =
    session !== null && (!isPlanningSession || isPlanApproved);
  const canImplementDirectly = Boolean(
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
    enabled: Boolean(session && isOwnedCurrentPlan && isPlanApproved),
    staleTime: 5_000,
    refetchInterval: (query) => (query.state.data ? false : 4_000),
  });
  const planActionHint = buildPlanActionHint({
    assessment: planComplexityQuery.data,
    isAssessing: planComplexityQuery.isFetching && !planComplexityQuery.data,
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
        { conversationId: workspace.conversationId },
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
