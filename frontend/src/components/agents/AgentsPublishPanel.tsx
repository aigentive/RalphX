import {
  AlertTriangle,
  CheckCircle2,
  Files,
  GitPullRequestArrow,
  GitBranch,
  History,
  ListChecks,
  Loader2,
  MoreVertical,
  ShieldCheck,
  Zap,
  XCircle,
} from "lucide-react";
import {
  type ReactNode,
  Suspense,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { toast } from "sonner";

import { diffApi } from "@/api/diff";
import { lazyWithRetry } from "@/lib/lazy-with-retry";
import {
  chatApi,
  type AgentConversationWorkspace,
  type AgentConversationWorkspacePublicationEvent,
  type AgentWorkspaceReviewContext,
} from "@/api/chat";
import type {
  Commit as DiffViewerCommit,
  FileChange as DiffViewerFileChange,
} from "@/components/diff";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogTitle,
} from "@/components/ui/dialog";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import { selectProjectById, useProjectStore } from "@/stores/projectStore";
import { getProjectWorkspacePublishMode } from "@/types/project";
import { GitAuthRepairPanel } from "@/components/git/GitAuthRepairPanel";
import { BranchBasePicker } from "@/components/shared/BranchBasePicker";
import {
  fallbackBranchBaseOptions,
  loadBranchBaseOptions,
} from "@/components/shared/branchBaseOptions";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { useConfirmation } from "@/hooks/useConfirmation";
import {
  prKeys,
  usePullRequestDetail,
} from "@/hooks/usePullRequestDetail";
import { useReviewSettings } from "@/hooks/useReviewSettings";
import {
  pullRequestSelectorFromShell,
  pullRequestShellFromWorkspace,
} from "@/components/pr/PullRequestDetailShell";
import { summarizeChecks } from "@/components/pr/pullRequestChecksSummary";
import { useDeferredAgentHydration } from "./useDeferredAgentHydration";
import { EmptyArtifactState } from "./AgentsArtifactEmptyState";
import {
  AgentsPublishActionBar,
  STATUS_ACTION_BUTTON_CLASSNAME,
  statusActionButtonStyle,
} from "./AgentsPublishActionBar";
import { AgentsPublishChecksTab } from "./AgentsPublishChecksTab";
import { AgentsPublishAutomationTab } from "./AgentsPublishAutomationTab";
import {
  deriveAgentsPublishAutomationSnapshot,
  type AgentsPublishAutomationSnapshot,
} from "./agentsPublishAutomationSnapshot";
import { PublishEventLog, selectPublishHistory } from "./AgentsPublishEventLog";
import { PublishPipelineSteps } from "./AgentsPublishPipelineSteps";
import {
  PublishWorkspaceDialog,
  type PublishWorkspaceDialogPhase,
} from "./AgentsPublishWorkspaceDialog";
import { AgentsPublishInlineDiffs } from "./AgentsPublishInlineDiffs";
import { AgentsPublishRepairState } from "./AgentsPublishRepairState";
import {
  blocksAgentWorkspaceGitInspection,
  canInspectAgentWorkspaceBaseFreshness,
  canInspectAgentWorkspacePublishDiffs,
  isAgentWorkspaceAutoMergeDeferred,
  isAgentWorkspaceAutoMergeRequestPending,
  getAgentWorkspacePrConflictSummary,
  getAgentWorkspaceDescriptionFailurePresentation,
  getAgentWorkspacePublishReceiptPresentation,
  getAgentWorkspaceTerminalPublicationLabel,
  getAgentWorkspaceTerminalPublicationStatus,
  getAgentWorkspaceEffectiveBaseLabel,
  getAgentWorkspaceMaintenancePresentation,
  hasPublishedWorkspacePr,
  isAgentWorkspacePublishActive,
  isAgentWorkspaceMaintenanceActive,
  isPipelineOwnedAgentWorkspace,
  isAgentWorkspacePublishCurrent,
  shouldAutoRefreshCleanAgentWorkspaceFromBase,
} from "./agentWorkspacePublishState";
import {
  AGENT_WORKSPACE_STALE_MS,
  agentWorkspaceKeys,
  invalidateWorkspaceQueries,
} from "./agentWorkspaceQueries";
import type { AgentPublishFocusRequest } from "./agentPublishFocus";
import type { AgentPublishSubTab } from "./agentPublishSubTab";
import {
  getAgentWorkspaceChangeFacts,
  mapReviewCommitsToDiffViewerCommits,
} from "./useAgentWorkspaceChangeSummary";
import { useAgentWorkspaceBaseUpdate } from "./useAgentWorkspaceBaseUpdate";
import { useAgentWorkspaceFullFreshness } from "./useAgentWorkspaceFullFreshness";
import type { AgentWorkspacePublishAttempt } from "./useAgentWorkspacePublisher";
import {
  agentWorkspaceOperationErrorDetail,
  agentWorkspaceOperationToastId,
  startAgentWorkspaceOperationToast,
} from "./agentWorkspaceOperationToast";

const LazyDiffViewer = lazyWithRetry(() =>
  import("@/components/diff").then((module) => ({ default: module.DiffViewer })),
);

const PUBLISH_EVENT_START_SKEW_MS = 5_000;
const PUBLISH_PIPELINE_EVENT_STEPS = new Set([
  "checking",
  "committing",
  "refreshing",
  "refreshed",
  "describing",
  "description_failed",
  "pushing",
  "pushed",
  "published",
]);

function latestPublicationEventForActivePublish(
  events: AgentConversationWorkspacePublicationEvent[],
  publishStartedAtMs: number | null,
  currentAttemptId: string | null,
): AgentConversationWorkspacePublicationEvent | null {
  const attemptEvents = currentAttemptId
    ? events.filter((event) => event.attemptId === currentAttemptId)
    : events;
  const candidates =
    publishStartedAtMs === null
      ? attemptEvents
      : attemptEvents.filter((event) => {
          const createdAtMs = new Date(event.createdAt).getTime();
          return (
            Number.isNaN(createdAtMs) ||
            createdAtMs >= publishStartedAtMs - PUBLISH_EVENT_START_SKEW_MS
          );
        });
  return candidates.length > 0 ? candidates[candidates.length - 1] ?? null : null;
}

function pipelineStatusFromPublicationEvent(
  event: AgentConversationWorkspacePublicationEvent | null,
): string | null {
  if (!event || !PUBLISH_PIPELINE_EVENT_STEPS.has(event.step)) {
    return null;
  }
  return event.step === "published" ? "pushed" : event.step;
}

function workspaceReviewAutoMergeGuardSummary(
  reviewContext: AgentWorkspaceReviewContext | null | undefined,
): { label: string; detail: string; status: "active" | "error" | "pending" } | null {
  const monitor = reviewContext?.monitor;
  switch (monitor?.autoMergeGuardStatus) {
    case "pausing":
      return {
        label: "Auto-merge pausing",
        detail: "GitHub auto-merge is being paused before Workspace Review starts.",
        status: "pending",
      };
    case "paused_for_review":
      return {
        label: "Auto-merge paused",
        detail: "GitHub auto-merge is paused while Workspace Review is active.",
        status: "active",
      };
    case "awaiting_publish":
      return {
        label: "Auto-merge awaiting publish",
        detail:
          "GitHub auto-merge will resume after these reviewed changes are published.",
        status: "active",
      };
    case "restoring":
      return {
        label: "Auto-merge restoring",
        detail: "GitHub auto-merge is being restored after Workspace Review.",
        status: "pending",
      };
    case "restore_failed":
      return {
        label: "Auto-merge restore failed",
        detail:
          monitor.autoMergeGuardLastError ??
          "GitHub auto-merge is still paused and restoration will retry.",
        status: "error",
      };
    default:
      return null;
  }
}

export function AgentPublishPanel({
  workspace,
  conversationTitle,
  projectBaseBranch,
  onPublishWorkspace,
  publishAttempt,
  publishFocusRequest,
  reviewContext,
  onOpenReview,
  activeSubTab,
  showReviewTab,
  onSubTabChange,
  reviewContent,
  reviewTabStatusColor,
  reviewTabStatusLabel,
  isReviewTabRunning,
}: {
  workspace: AgentConversationWorkspace | null;
  conversationTitle?: string | null;
  projectBaseBranch?: string | null;
  onPublishWorkspace: ((conversationId: string) => Promise<void>) | undefined;
  publishAttempt: AgentWorkspacePublishAttempt | null;
  publishFocusRequest?: AgentPublishFocusRequest | null;
  reviewContext?: AgentWorkspaceReviewContext | null;
  onOpenReview?: () => void;
  activeSubTab: AgentPublishSubTab;
  showReviewTab: boolean;
  onSubTabChange: (tab: AgentPublishSubTab) => void;
  reviewContent: ReactNode;
  reviewTabStatusColor?: string | null;
  reviewTabStatusLabel?: string | null;
  isReviewTabRunning?: boolean;
}) {
  const queryClient = useQueryClient();
  const [reviewOpen, setReviewOpen] = useState(false);
  const [commitFiles, setCommitFiles] = useState<DiffViewerFileChange[]>([]);
  const [isLoadingCommitFiles, setIsLoadingCommitFiles] = useState(false);
  const [rebaseDialogOpen, setRebaseDialogOpen] = useState(false);
  const [publishDialogState, setPublishDialogState] = useState<{
    conversationId: string;
    open: boolean;
    phase: PublishWorkspaceDialogPhase;
  } | null>(null);
  const [automationSnapshot, setAutomationSnapshot] =
    useState<AgentsPublishAutomationSnapshot | null>(null);
  const prDescriptionPrecomputeKeysRef = useRef<Set<string>>(new Set());
  const autoRefreshFromBaseKeysRef = useRef<Set<string>>(new Set());
  const [selectedRebaseBaseKey, setSelectedRebaseBaseKey] = useState("");
  const { confirm, confirmationDialogProps, ConfirmationDialog } = useConfirmation();
  const reviewSettingsQuery = useReviewSettings();
  const conversationId = workspace?.conversationId ?? null;
  const project = useProjectStore(
    selectProjectById(workspace?.projectId ?? ""),
  );
  const localCommitTokenRef = useRef(0);
  const localCommitAttemptRef = useRef<{
    conversationId: string;
    attemptToken: string;
  } | null>(null);
  const activeConversationIdRef = useRef<string | null>(conversationId);
  useEffect(() => {
    activeConversationIdRef.current = conversationId;
    if (
      localCommitAttemptRef.current &&
      localCommitAttemptRef.current.conversationId !== conversationId
    ) {
      localCommitAttemptRef.current = null;
      localCommitTokenRef.current += 1;
    }
  }, [conversationId]);
  const [mountedSubTabs, setMountedSubTabs] = useState<{
    automation: boolean;
    changes: boolean;
    checks: boolean;
    conversationId: string | null;
    history: boolean;
    review: boolean;
  }>(() => ({
    automation: activeSubTab === "automation",
    changes: activeSubTab === "changes",
    checks: activeSubTab === "checks",
    conversationId,
    history: activeSubTab === "history",
    review: activeSubTab === "review",
  }));
  const mountedSubTabsForConversation =
    mountedSubTabs.conversationId === conversationId
      ? mountedSubTabs
      : {
          automation: activeSubTab === "automation",
          changes: activeSubTab === "changes",
          checks: activeSubTab === "checks",
          conversationId,
          history: activeSubTab === "history",
          review: activeSubTab === "review",
        };
  useEffect(() => {
    setMountedSubTabs((current) => {
      const sameConversation = current.conversationId === conversationId;
      return {
        automation:
          (sameConversation && current.automation) ||
          activeSubTab === "automation",
        changes:
          (sameConversation && current.changes) || activeSubTab === "changes",
        checks:
          (sameConversation && current.checks) || activeSubTab === "checks",
        conversationId,
        history:
          (sameConversation && current.history) || activeSubTab === "history",
        review: (sameConversation && current.review) || activeSubTab === "review",
      };
    });
  }, [activeSubTab, conversationId]);
  useEffect(() => {
    setAutomationSnapshot((current) =>
      current?.conversationId === conversationId ? current : null,
    );
  }, [conversationId]);
  const maintenancePresentation = getAgentWorkspaceMaintenancePresentation(workspace);
  const isMaintenanceActive = isAgentWorkspaceMaintenanceActive(workspace);
  const blocksGitInspection = blocksAgentWorkspaceGitInspection(workspace);
  const isPublishingWorkspace =
    publishAttempt !== null || isAgentWorkspacePublishActive(workspace);
  const publishStartedAtMs = publishAttempt?.startedAtMs ?? null;
  const currentPublishDialogState =
    publishDialogState?.conversationId === conversationId ? publishDialogState : null;
  const publishDialogOpen = currentPublishDialogState?.open ?? false;
  const publishDialogPhase = currentPublishDialogState?.phase ?? "confirm";
  const {
    isUpdatingFromBase,
    runUpdateFromBase,
    syncMaintenanceOperation,
  } = useAgentWorkspaceBaseUpdate({ conversationTitle });
  useEffect(() => {
    syncMaintenanceOperation(workspace);
  }, [syncMaintenanceOperation, workspace]);
  const canHydratePublishFacts = useDeferredAgentHydration(conversationId);
  const isRepairPending =
    !workspace?.maintenanceOperation &&
    workspace?.publicationPushStatus === "needs_agent" &&
    !getAgentWorkspaceTerminalPublicationStatus(workspace);
  const hasPublishedPr = hasPublishedWorkspacePr(workspace);
  const workspacePublishMode = getProjectWorkspacePublishMode(
    project,
    hasPublishedPr,
  );
  const repositoryInspectionFailed =
    workspacePublishMode.kind === "unavailable";
  const checksShell = hasPublishedPr
    ? pullRequestShellFromWorkspace(workspace)
    : null;
  const checksSelector = pullRequestSelectorFromShell(checksShell);
  const checksHydrationKey =
    activeSubTab === "checks" && conversationId && checksSelector
      ? `${conversationId}:${prKeys.detail(checksSelector).join(":")}`
      : null;
  const canHydrateChecks = useDeferredAgentHydration(checksHydrationKey);
  const checksDetailQuery = usePullRequestDetail(checksSelector, {
    enabled: activeSubTab === "checks" && canHydrateChecks,
  });
  const checksDetail = checksDetailQuery.data ?? null;
  const checksSummary = useMemo(
    () => summarizeChecks(checksDetail?.checks ?? []),
    [checksDetail?.checks],
  );
  const checksAvailable =
    checksDetail?.state === "loaded" &&
    !checksDetail.sourcesUnavailable.includes("checks") &&
    !checksDetailQuery.isError;
  const checksAttentionCount = checksAvailable
    ? checksSummary.failed + checksSummary.pending
    : 0;
  const terminalPublicationStatus =
    getAgentWorkspaceTerminalPublicationStatus(workspace);
  // Workspace-only flag computed early so reviewQuery can decide whether the
  // inline diff view will be visible.
  const inlineDiffsCandidate = canInspectAgentWorkspacePublishDiffs(workspace, {
    includeTerminalPublished: true,
  });
  const reviewQuery = useQuery({
    queryKey: agentWorkspaceKeys.review(conversationId),
    queryFn: () => diffApi.getAgentConversationWorkspaceReview(conversationId!),
    // Pane-wide: feeds the no-changes publish guard, header presentation, and
    // Changes badge even while the Review subtab is the first to mount.
    enabled:
      canHydratePublishFacts &&
      !!conversationId &&
      !isRepairPending &&
      !blocksGitInspection &&
      (reviewOpen || inlineDiffsCandidate),
    staleTime: 2_000,
  });
  const changeSummaryQuery = useQuery({
    queryKey: agentWorkspaceKeys.changeSummary(conversationId),
    queryFn: () =>
      diffApi.getAgentConversationWorkspaceChangeSummary(conversationId!),
    enabled:
      canHydratePublishFacts &&
      !!conversationId &&
      inlineDiffsCandidate &&
      !isRepairPending &&
      !blocksGitInspection &&
      !terminalPublicationStatus,
    staleTime: AGENT_WORKSPACE_STALE_MS,
  });
  const publicationEventsQuery = useQuery({
    queryKey: ["agents", "conversation-workspace-publication-events", conversationId],
    queryFn: () =>
      chatApi.listAgentConversationWorkspacePublicationEvents(conversationId!),
    enabled: canHydratePublishFacts && !!conversationId,
    staleTime: 0,
    refetchInterval: isPublishingWorkspace || isMaintenanceActive ? 1_500 : false,
  });
  const prAnnotationsQuery = useQuery({
    queryKey: agentWorkspaceKeys.prAnnotations(conversationId),
    queryFn: () => diffApi.getAgentConversationWorkspacePrAnnotations(conversationId!),
    enabled: canHydratePublishFacts && !!conversationId && hasPublishedPr,
    staleTime: 30_000,
    refetchInterval: isPublishingWorkspace || isMaintenanceActive ? 5_000 : false,
  });
  const workspaceReviewHunkAnnotationsQuery = useQuery({
    queryKey: agentWorkspaceKeys.workspaceReviewHunkAnnotations(conversationId),
    queryFn: () =>
      diffApi.getAgentConversationWorkspaceReviewHunkAnnotations(conversationId!),
    enabled:
      canHydratePublishFacts &&
      !!conversationId &&
      !isRepairPending &&
      !blocksGitInspection &&
      (reviewOpen || inlineDiffsCandidate),
    staleTime: 2_000,
    refetchInterval: isPublishingWorkspace || isMaintenanceActive ? 5_000 : false,
  });
  const terminalPublicationLabel =
    getAgentWorkspaceTerminalPublicationLabel(workspace);
  const inlineDiffDefaultMode = terminalPublicationStatus
    ? "cumulative"
    : undefined;
  const cumulativeModeLabel =
    terminalPublicationStatus === "merged"
      ? "Published changes"
      : terminalPublicationStatus === "closed"
        ? "Pull request changes"
        : undefined;
  const isPipelineOwnedWorkspace = isPipelineOwnedAgentWorkspace(workspace);
  const canCommitLocally =
    workspace?.mode === "edit" && !isPipelineOwnedWorkspace;
  const isLocalCommitPrimary =
    workspacePublishMode.kind === "localCommit" && canCommitLocally;
  const isPipelinePrAutomationWorkspace =
    workspace?.mode === "ideation" && isPipelineOwnedWorkspace && hasPublishedPr;
  const shouldShowPrSupervisionControls =
    (workspacePublishMode.kind === "newPr" ||
      workspacePublishMode.kind === "persistedPr") &&
    (workspace?.mode === "edit" || isPipelinePrAutomationWorkspace);
  useEffect(() => {
    if (activeSubTab === "automation" && !shouldShowPrSupervisionControls) {
      onSubTabChange("changes");
    }
  }, [activeSubTab, onSubTabChange, shouldShowPrSupervisionControls]);
  useEffect(() => {
    if (activeSubTab === "checks" && !checksHydrationKey) {
      onSubTabChange("changes");
    }
  }, [activeSubTab, checksHydrationKey, onSubTabChange]);
  const canInspectBaseFreshness =
    canInspectAgentWorkspaceBaseFreshness(workspace);
  const freshnessQuery = useAgentWorkspaceFullFreshness(conversationId, {
    enabled:
      canHydratePublishFacts &&
      !!conversationId &&
      !isRepairPending &&
      !blocksGitInspection &&
      canInspectBaseFreshness &&
      !terminalPublicationStatus,
    isOperationActive:
      isPublishingWorkspace || isMaintenanceActive || isUpdatingFromBase,
  });
  const freshness = freshnessQuery.data;
  const shouldAutoRefreshFromBase = shouldAutoRefreshCleanAgentWorkspaceFromBase(
    workspace,
    freshness,
  );
  const baseStatus = freshness?.baseStatus ?? "valid";
  const baseBlocked = baseStatus === "blocked";
  const fallbackRebaseOptions = useMemo(
    () => fallbackBranchBaseOptions(projectBaseBranch),
    [projectBaseBranch],
  );
  const rebaseBaseOptionsQuery = useQuery({
    queryKey: [
      "agents",
      "conversation-workspace-rebase-base-options",
      conversationId,
      workspace?.worktreePath,
      workspace?.branchName,
      projectBaseBranch,
    ],
    queryFn: async () => {
      const result = await loadBranchBaseOptions({
        projectId: workspace!.projectId,
        workingDirectory: workspace!.worktreePath,
        projectBaseBranch,
        includeAgentBranches: false,
      });
      const options = result.options.filter(
        (option) => option.selection.ref !== workspace!.branchName,
      );
      const projectDefaultKey =
        options.find((option) => option.source === "project")?.key ??
        options[0]?.key ??
        result.selectedKey;
      return {
        options,
        selectedKey: projectDefaultKey,
      };
    },
    enabled:
      canHydratePublishFacts &&
      !!conversationId &&
      !!workspace?.worktreePath &&
      !blocksGitInspection &&
      baseBlocked,
    staleTime: 10_000,
  });
  const rebaseBaseOptionsResult =
    rebaseBaseOptionsQuery.data ?? fallbackRebaseOptions;
  const rebaseBaseOptions = rebaseBaseOptionsResult.options;
  const resolvedRebaseBaseKey = rebaseBaseOptions.some(
    (option) => option.key === selectedRebaseBaseKey,
  )
    ? selectedRebaseBaseKey
    : rebaseBaseOptionsResult.selectedKey;
  const selectedRebaseBase =
    rebaseBaseOptions.find((option) => option.key === resolvedRebaseBaseKey) ??
    null;
  useEffect(() => {
    if (rebaseBaseOptionsQuery.data) {
      setSelectedRebaseBaseKey(rebaseBaseOptionsQuery.data.selectedKey);
    }
  }, [rebaseBaseOptionsQuery.data]);
  useEffect(() => {
    autoRefreshFromBaseKeysRef.current.clear();
  }, [conversationId]);
  useEffect(() => {
    if (
      !workspace ||
      !conversationId ||
      !shouldAutoRefreshFromBase ||
      blocksGitInspection ||
      isPublishingWorkspace ||
      isUpdatingFromBase
    ) {
      return;
    }

    const refreshKey = [
      conversationId,
      freshness?.targetRef ?? workspace.baseRef,
      freshness?.targetBaseCommit ?? "",
    ].join(":");
    if (autoRefreshFromBaseKeysRef.current.has(refreshKey)) {
      return;
    }
    autoRefreshFromBaseKeysRef.current.add(refreshKey);

    runUpdateFromBase({
      conversationId,
      detail: `From ${getAgentWorkspaceEffectiveBaseLabel(workspace, freshness)}`,
      kind: "update-from-base",
      title: "Refreshing branch",
      workspace,
    });
  }, [
    conversationId,
    freshness,
    blocksGitInspection,
    isPublishingWorkspace,
    isRepairPending,
    isUpdatingFromBase,
    runUpdateFromBase,
    shouldAutoRefreshFromBase,
    workspace,
  ]);
  const closePrMutation = useMutation<AgentConversationWorkspace, Error>({
    mutationFn: () => chatApi.closeAgentWorkspacePr(conversationId!),
    onSuccess: async (updatedWorkspace) => {
      queryClient.setQueryData(
        ["agents", "conversation-workspace", updatedWorkspace.conversationId],
        updatedWorkspace,
      );
      await invalidateWorkspaceQueries(queryClient, updatedWorkspace.conversationId);
      toast.success("Pull request closed");
    },
    onError: (error) => {
      toast.error(
        error instanceof Error ? error.message : "Failed to close pull request",
      );
    },
  });
  const commitLocallyMutation = useMutation({
    mutationFn: async () => {
      if (!conversationId || !workspace) {
        throw new Error("No workspace selected");
      }
      const initiatingConversationId = conversationId;
      const expectedHeadSha = reviewContext?.monitor.workspaceHeadSha;
      if (!expectedHeadSha) {
        throw new Error("Refresh workspace changes before committing locally.");
      }
      const attemptToken = String(++localCommitTokenRef.current);
      localCommitAttemptRef.current = {
        conversationId: initiatingConversationId,
        attemptToken,
      };
      const toastController = startAgentWorkspaceOperationToast({
        conversationTitle,
        detail: "Commit isolated workspace branch",
        id: agentWorkspaceOperationToastId(
          initiatingConversationId,
          "local-commit",
        ),
        title: "Committing locally",
      });
      try {
        const result = await chatApi.commitAgentConversationWorkspaceLocally(
          initiatingConversationId,
          {
            expectedHeadSha,
            reviewArtifactId: reviewContext?.monitor.reviewArtifactId ?? null,
            reviewArtifactVersion:
              reviewContext?.monitor.reviewArtifactVersion ?? null,
            reviewedHeadSha: reviewContext?.monitor.reviewedHeadSha ?? null,
            reviewedDiffFingerprint:
              reviewContext?.monitor.reviewedDiffFingerprint ?? null,
            attemptToken,
          },
        );
        const isCurrentAttempt =
          activeConversationIdRef.current === initiatingConversationId &&
          localCommitAttemptRef.current?.conversationId ===
            initiatingConversationId &&
          localCommitAttemptRef.current?.attemptToken === attemptToken &&
          result.attemptToken === attemptToken;
        if (!isCurrentAttempt) {
          toastController.dismiss();
          return { attemptToken, initiatingConversationId, result };
        }
        const shortSha = result.commitSha.slice(0, 7);
        if (result.outcome === "committed_local") {
          toastController.success(`Committed locally on ${result.branchName}`, {
            detail: shortSha,
          });
        } else if (result.outcome === "already_committed") {
          toastController.info("Already committed locally", {
            detail: shortSha,
          });
        } else {
          toastController.info("No local changes to commit");
        }
        return { attemptToken, initiatingConversationId, result };
      } catch (error) {
        if (
          activeConversationIdRef.current === initiatingConversationId &&
          localCommitAttemptRef.current?.conversationId ===
            initiatingConversationId &&
          localCommitAttemptRef.current?.attemptToken === attemptToken
        ) {
          toastController.error("Failed to commit locally", {
            detail: agentWorkspaceOperationErrorDetail(
              error,
              "Failed to commit locally",
            ),
          });
        } else {
          toastController.dismiss();
        }
        throw error;
      }
    },
    onSuccess: async ({
      attemptToken,
      initiatingConversationId,
      result,
    }) => {
      if (
        activeConversationIdRef.current !== initiatingConversationId ||
        localCommitAttemptRef.current?.conversationId !==
          initiatingConversationId ||
        localCommitAttemptRef.current?.attemptToken !== attemptToken ||
        result.attemptToken !== attemptToken
      ) {
        return;
      }
      queryClient.setQueryData(
        agentWorkspaceKeys.workspace(initiatingConversationId),
        result.workspace,
      );
      await invalidateWorkspaceQueries(
        queryClient,
        initiatingConversationId,
      );
    },
  });
  const changesError = reviewQuery.error;
  const changes = reviewQuery.data?.changes ?? [];
  const commits = useMemo<DiffViewerCommit[]>(
    () => mapReviewCommitsToDiffViewerCommits(reviewQuery.data),
    [reviewQuery.data],
  );
  const publicationEvents = publicationEventsQuery.data ?? [];
  const prAnnotations = prAnnotationsQuery.data?.annotations ?? [];
  const workspaceReviewHunkAnnotations =
    workspaceReviewHunkAnnotationsQuery.data?.annotations ?? [];
  const prAnnotationSourcesUnavailable =
    prAnnotationsQuery.data?.sourcesUnavailable ?? [];
  const isChangesLoading =
    Boolean(conversationId) &&
    inlineDiffsCandidate &&
    (!canHydratePublishFacts || reviewQuery.isLoading);
  const isPublicationEventsLoading =
    Boolean(conversationId) &&
    (!canHydratePublishFacts || publicationEventsQuery.isLoading);
  const hasNoDetectedChanges = reviewQuery.isSuccess && changes.length === 0;
  const isManagedByTaskPipeline = isPipelineOwnedWorkspace && !isPipelinePrAutomationWorkspace;
  useEffect(() => {
    if (
      !conversationId ||
      !workspace ||
      !reviewQuery.isSuccess ||
      !reviewQuery.data ||
      reviewQuery.data.changes.length === 0 ||
      isAgentWorkspacePublishCurrent(workspace, freshness) ||
      (freshness?.baseStatus ?? "valid") === "blocked" ||
      Boolean(freshness?.isBaseAhead) ||
      isPipelineOwnedAgentWorkspace(workspace) ||
      Boolean(getAgentWorkspaceTerminalPublicationStatus(workspace)) ||
      workspace.status === "missing"
    ) {
      return;
    }
    const precomputeKey = [
      conversationId,
      reviewQuery.data.baseRef,
      reviewQuery.data.headRef,
      reviewQuery.data.commits.length,
      reviewQuery.data.changes.length,
    ].join(":");
    if (prDescriptionPrecomputeKeysRef.current.has(precomputeKey)) {
      return;
    }
    prDescriptionPrecomputeKeysRef.current.add(precomputeKey);
    void chatApi
      .precomputeAgentConversationWorkspacePrDescription(conversationId)
      .catch(() => {
        prDescriptionPrecomputeKeysRef.current.delete(precomputeKey);
      });
  }, [
    conversationId,
    freshness,
    reviewQuery.data,
    reviewQuery.isSuccess,
    workspace,
  ]);

  if (!workspace) {
    return <EmptyArtifactState title="No workspace selected" />;
  }

  const branch = workspace.branchName;
  const base = getAgentWorkspaceEffectiveBaseLabel(workspace, freshness);
  const publishTargetPullRequestLabel = workspace.publicationPrNumber
    ? `PR #${workspace.publicationPrNumber}`
    : workspace.publicationPrUrl
      ? "the linked pull request"
      : null;
  const baseRetargeted = baseStatus === "retargeted";
  const isBranchUpdateNeeded =
    !baseBlocked && !terminalPublicationStatus && Boolean(freshness?.isBaseAhead);
  const isPublishCurrent = isAgentWorkspacePublishCurrent(workspace, freshness);
  const isPublishingThisWorkspace = isPublishingWorkspace;
  const effectivePublishing =
    isPublishingThisWorkspace || isUpdatingFromBase || isMaintenanceActive;
  const publishHistoryCount = selectPublishHistory(
    publicationEvents,
    effectivePublishing,
    workspace.publicationMetadataAttemptId,
  ).visibleEvents.length;
  const isDescriptionFailed = workspace.publicationPushStatus === "description_failed";
  const receiptPresentation = getAgentWorkspacePublishReceiptPresentation(workspace);
  const latestActivePublishEvent = latestPublicationEventForActivePublish(
    publicationEvents,
    publishStartedAtMs,
    workspace.publicationMetadataAttemptId,
  );
  const eventPipelineStatus = isPublishingThisWorkspace
    ? pipelineStatusFromPublicationEvent(latestActivePublishEvent)
    : null;
  const localPublishFallbackStatus =
    publishStartedAtMs !== null && !eventPipelineStatus ? "checking" : null;
  const workspacePipelineStatus =
    isPublishingThisWorkspace &&
    !PUBLISH_PIPELINE_EVENT_STEPS.has(workspace.publicationPushStatus ?? "")
      ? "checking"
      : workspace.publicationPushStatus;
  const pipelineStatus = isUpdatingFromBase
    ? "refreshing"
    : eventPipelineStatus ?? localPublishFallbackStatus ?? workspacePipelineStatus;
  const baseActionLabel =
    freshness?.effectiveBaseDisplayName ??
    freshness?.effectiveBaseRef ??
    freshness?.baseRef ??
    workspace.baseRef ??
    base;
  const baselineAutomationSnapshot = deriveAgentsPublishAutomationSnapshot({
    workspace,
    hasPublishedPr,
  });
  const effectiveAutomationSnapshot =
    automationSnapshot?.conversationId === workspace.conversationId
      ? automationSnapshot
      : baselineAutomationSnapshot;
  const {
    autoPublishEnabled,
    isAutoPublishSaving,
    isPrSupervisionSaving,
    prAutofixEnabled,
    prAutoMergeCurrent,
    prAutoMergeDesired,
    prSupervisionStatus,
  } = effectiveAutomationSnapshot;
  const isAutomationPreferenceSaving =
    isAutoPublishSaving || isPrSupervisionSaving;
  const prConflictSummary = getAgentWorkspacePrConflictSummary(workspace);
  const hasPrConflict = prConflictSummary !== null;
  const workspaceReviewRequired =
    reviewSettingsQuery.data?.require_workspace_review ?? true;
  const reviewGateStatus = reviewContext?.monitor.reviewGateStatus ?? null;
  const reviewIsRunning = Boolean(
    isReviewTabRunning || reviewGateStatus === "reviewing",
  );
  const autoMergeGuardSummary =
    workspaceReviewAutoMergeGuardSummary(reviewContext);
  const reviewBlocksPublish =
    workspaceReviewRequired &&
    (reviewIsRunning ||
      reviewGateStatus === "required" ||
      reviewGateStatus === "blocking" ||
      reviewGateStatus === "failed");
  const reviewGateSummary = (() => {
    if (!workspaceReviewRequired) {
      return null;
    }
    if (reviewIsRunning) {
      return "Workspace Review is running. Open the Review tab to inspect it before publishing.";
    }
    if (reviewGateStatus === "blocking") {
      return (
        reviewContext?.monitor.reviewBlockingSummary ??
        "Workspace Review found blocking issues. Publishing is blocked until the agent addresses them and a new Review passes."
      );
    }
    if (reviewGateStatus === "failed") {
      return "Workspace Review failed. Retry Review before publishing.";
    }
    if (reviewGateStatus === "required") {
      return "Workspace Review is required before publishing.";
    }
    return null;
  })();
  const autoMergeArgs = {
    autoMergeDesired: prAutoMergeDesired,
    autoMergeCurrent: prAutoMergeCurrent,
    hasPublishedPr,
    prSupervisionStatus,
    publicationPushStatus: workspace.publicationPushStatus,
    terminalPublicationStatus,
  };
  const shouldShowAutoMergeProgress =
    isAgentWorkspaceAutoMergeRequestPending(autoMergeArgs);
  const shouldShowAutoMergeDeferred =
    isAgentWorkspaceAutoMergeDeferred(autoMergeArgs);
  const shouldShowPublishPipeline =
    !isRepairPending &&
    !blocksGitInspection &&
    (effectivePublishing ||
      workspace.publicationPushStatus === "description_failed" ||
      shouldShowAutoMergeProgress ||
      shouldShowAutoMergeDeferred);
  const publishDisabled =
    !onPublishWorkspace ||
    isManagedByTaskPipeline ||
    effectivePublishing ||
    isAutomationPreferenceSaving ||
    baseBlocked ||
    reviewBlocksPublish ||
    hasPrConflict ||
    (isRepairPending && !isPipelineOwnedWorkspace) ||
    isPublishCurrent ||
    Boolean(terminalPublicationStatus) ||
    repositoryInspectionFailed ||
    (hasNoDetectedChanges && !isPipelinePrAutomationWorkspace) ||
    workspace.status === "missing";
  const publishButtonLabel = (() => {
    if (isPublishingThisWorkspace) return "Publishing";
    if (terminalPublicationLabel) return terminalPublicationLabel;
    if (isManagedByTaskPipeline) return "Managed by Tasks";
    if (reviewBlocksPublish && reviewIsRunning) return "Reviewing";
    if (reviewBlocksPublish && reviewGateStatus === "required") return "Review required";
    if (reviewBlocksPublish && reviewGateStatus === "blocking") return "Review blocking";
    if (reviewBlocksPublish && reviewGateStatus === "failed") return "Review failed";
    if (isPublishCurrent) return "PR is up to date";
    return "Commit & Publish";
  })();
  const localCommitDisabled =
    !canCommitLocally ||
    commitLocallyMutation.isPending ||
    effectivePublishing ||
    reviewBlocksPublish ||
    isRepairPending ||
    workspace.status === "missing" ||
    !reviewContext?.monitor.workspaceHeadSha;
  const canClosePr =
    hasPublishedPr &&
    !isRepairPending &&
    !isMaintenanceActive &&
    !terminalPublicationStatus;
  const isClosingPr = closePrMutation.isPending;
  const shouldShowPublishNotices = !isRepairPending && !maintenancePresentation;
  const canConfigurePrSupervision =
    shouldShowPrSupervisionControls &&
    workspace.status !== "missing" &&
    !isMaintenanceActive &&
    !terminalPublicationStatus;
  const prSupervisionStatusLabel = (() => {
    if (terminalPublicationStatus) return null;
    if (autoMergeGuardSummary) return autoMergeGuardSummary.label;
    if (isAutoPublishSaving) return "Saving Auto Publish";
    if (isPrSupervisionSaving) return "Saving PR supervision";
    if (!hasPublishedPr && autoPublishEnabled) return "Auto Publish armed";
    if (hasPrConflict) return "PR conflicts";
    if (!autoPublishEnabled && hasPublishedPr) return "Auto Publish paused";
    if (prSupervisionStatus === "fixing") return "Fixing PR";
    if (prSupervisionStatus === "waiting_for_checks") return "Waiting for checks";
    if (prSupervisionStatus === "blocked") return "PR supervision blocked";
    if (prAutofixEnabled || prAutoMergeDesired) return "Monitoring PR";
    return null;
  })();
  const AutoMergeGuardIcon =
    autoMergeGuardSummary?.status === "pending" ? Loader2 : AlertTriangle;
  const autoMergeGuardColor =
    autoMergeGuardSummary?.status === "error"
      ? "var(--status-error)"
      : autoMergeGuardSummary?.status === "pending"
        ? "var(--accent-primary)"
        : "var(--status-warning)";
  const autoMergeGuardBorderColor =
    autoMergeGuardSummary?.status === "error"
      ? "var(--status-error-border)"
      : "var(--status-warning-border)";
  const terminalPrLabel =
    workspace.publicationPrNumber != null
      ? `PR #${workspace.publicationPrNumber}`
      : "This pull request";
  const publishPresentation = (() => {
    if (terminalPublicationStatus === "merged") {
      return {
        title: "Pull Request Merged",
        summary: `${terminalPrLabel} has been merged. By continuing this conversation, a new workspace branch will be created automatically.`,
        tone: "success" as const,
      };
    }
    if (terminalPublicationStatus === "closed") {
      return {
        title: "Pull Request Closed",
        summary: `${terminalPrLabel} is closed. By continuing this conversation, a new workspace branch will be created automatically.`,
        tone: "neutral" as const,
      };
    }
    if (maintenancePresentation) {
      return {
        ...maintenancePresentation,
        summary: [
          maintenancePresentation.summary,
          maintenancePresentation.automaticContinuation,
        ]
          .filter((value): value is string => Boolean(value))
          .join(" "),
      };
    }
    if (isRepairPending) {
      return {
        title: "Repair in progress",
        summary:
          "RalphX routed this workspace to the agent for repair. Publishing will resume after the repair completes.",
        tone: "warning" as const,
      };
    }
    if (isPublishingThisWorkspace) {
      return {
        title: "Publishing workspace",
        summary:
          "Follow the publish pipeline below while RalphX commits and publishes this workspace.",
        tone: "neutral" as const,
        busy: true,
      };
    }
    if (hasPrConflict) {
      return {
        title: "Pull request conflicts",
        summary: autoPublishEnabled
          ? "Auto Publish is waiting for PR conflicts to be resolved. Resolve conflicts to update the branch from base."
          : "This pull request has conflicts. Resolve conflicts to update the branch from base before publishing can continue.",
        tone: "warning" as const,
      };
    }
    if (isBranchUpdateNeeded) {
      return {
        title: isUpdatingFromBase ? "Updating branch" : "Update from base required",
        summary: `Base branch ${baseActionLabel} has new commits. Publishing will continue after this branch is updated.`,
        tone: "warning" as const,
        ...(isUpdatingFromBase ? { busy: true } : {}),
      };
    }
    if (baseBlocked) {
      return {
        title: "Publishing blocked",
        summary: "Publishing is blocked until the workspace base branch is resolved.",
        tone: "warning" as const,
      };
    }
    if (reviewGateSummary && reviewGateStatus) {
      const title =
        reviewGateStatus === "reviewing"
          ? "Workspace Review in progress"
          : reviewGateStatus === "blocking"
            ? "Workspace Review blocking"
            : reviewGateStatus === "failed"
              ? "Workspace Review failed"
              : reviewGateStatus === "required"
                ? "Workspace Review required"
                : null;
      if (title) {
        return {
          title,
          summary: reviewGateSummary,
          tone:
            reviewGateStatus === "failed"
              ? ("error" as const)
              : reviewGateStatus === "reviewing"
                ? ("neutral" as const)
                : ("warning" as const),
          ...(reviewGateStatus === "reviewing" ? { busy: true } : {}),
        };
      }
    }
    if (isManagedByTaskPipeline) {
      return {
        title: "Managed by task pipeline",
        summary:
          workspace.publicationPrNumber || workspace.publicationPrUrl
            ? `${terminalPrLabel} is managed by this ideation plan's task pipeline.`
            : "Publishing is managed by this ideation plan's task pipeline.",
        tone: "neutral" as const,
      };
    }
    if (workspacePublishMode.kind === "unavailable") {
      return {
        title: "Repository configuration unavailable",
        summary: workspacePublishMode.guidance,
        tone: "warning" as const,
      };
    }
    if (workspacePublishMode.kind === "localCommit") {
      return {
        title: "Ready to commit locally",
        summary: workspacePublishMode.guidance,
        tone: "neutral" as const,
      };
    }
    if (receiptPresentation) {
      return receiptPresentation;
    }
    if (isDescriptionFailed) {
      const descriptionFailure = getAgentWorkspaceDescriptionFailurePresentation(
        publishTargetPullRequestLabel,
        workspace.publicationMetadataState,
      );
      return {
        ...descriptionFailure,
        tone: "error" as const,
      };
    }
    if (hasPublishedPr && !autoPublishEnabled) {
      return {
        title: "Automatic publishing paused",
        summary: "Automatic publishing is paused. Manual Commit & Publish remains available.",
        tone: "warning" as const,
      };
    }
    if (!hasPublishedPr && autoPublishEnabled) {
      return {
        title: "Auto Publish enabled",
        summary: "Auto Publish will run Commit & Publish when the agent finishes.",
        tone: "neutral" as const,
      };
    }
    if (isChangesLoading) {
      return {
        title: "Checking workspace changes",
        summary: "Loading changed files...",
        tone: "neutral" as const,
        busy: true,
      };
    }
    if (isPublishCurrent) {
      return {
        title: "Published to GitHub",
        summary:
          reviewQuery.isSuccess && changes.length > 0
            ? `${changes.length} changed file${changes.length === 1 ? "" : "s"} published for review.`
            : "Workspace is published and current.",
        tone: "success" as const,
      };
    }
    if (reviewQuery.isSuccess && changes.length > 0) {
      return {
        title: "Ready to publish",
        summary: `${changes.length} changed file${changes.length === 1 ? "" : "s"} ready for review.`,
        tone: "warning" as const,
      };
    }
    if (reviewQuery.isSuccess) {
      return {
        title: "No changes to publish",
        summary: "No changed files detected yet.",
        tone: "neutral" as const,
      };
    }
    return {
      title: "Review workspace changes",
      summary: "Review changes before publishing.",
      tone: "neutral" as const,
    };
  })();
  const maintenanceLiveAnnouncement =
    maintenancePresentation && workspace.maintenanceOperation
      ? {
          operationKey: `${workspace.maintenanceOperation.operationId}:${workspace.maintenanceOperation.generation}:${workspace.maintenanceOperation.stage}`,
          title: publishPresentation.title,
          summary: publishPresentation.summary,
        }
      : null;
  const confirmUpdateFromBase = () => {
    void confirm({
      title: "Update from base branch?",
      description: `This will update ${branch} with the latest changes from ${baseActionLabel}. If conflicts are found, RalphX will route this workspace through repair before publishing can continue.`,
      confirmText: "Update branch",
    }).then((confirmed) => {
      if (!confirmed) {
        return;
      }
      if (!conversationId) {
        return;
      }
      runUpdateFromBase({
        conversationId,
        detail: `From ${baseActionLabel}`,
        kind: "update-from-base",
        title: "Updating branch",
        workspace,
      });
    });
  };
  const confirmResolvePrConflicts = () => {
    void confirm({
      title: "Resolve PR conflicts?",
      description: `${terminalPrLabel} is conflicting on GitHub. RalphX will update ${branch} from ${baseActionLabel}; if conflicts are found locally, this workspace will route through repair before publishing can continue.`,
      confirmText: "Resolve conflicts",
    }).then((confirmed) => {
      if (!confirmed || !conversationId) {
        return;
      }
      runUpdateFromBase({
        conversationId,
        detail: `Resolve ${terminalPrLabel} against ${baseActionLabel}`,
        kind: "update-from-base",
        title: "Resolving PR conflicts",
        workspace,
      });
    });
  };
  const rebaseFromSelectedBase = () => {
    if (!selectedRebaseBase) {
      toast.error("Select a base branch before rebasing");
      return;
    }
    setRebaseDialogOpen(false);
    runUpdateFromBase({
      baseSelection: selectedRebaseBase.selection,
      conversationId: workspace.conversationId,
      detail: `From ${selectedRebaseBase.selection.displayName}`,
      kind: "rebase",
      title: "Rebasing branch",
      workspace,
    });
  };
  const confirmClosePr = () => {
    void confirm({
      title: "Close pull request?",
      description: `This will close ${terminalPrLabel} for ${branch}. The workspace files and conversation history will remain available.`,
      confirmText: "Close PR",
      pendingText: "Closing...",
      variant: "destructive",
      onConfirm: () => closePrMutation.mutateAsync(),
    });
  };
  const confirmPublishWorkspace = () => {
    if (!onPublishWorkspace || publishDisabled) {
      return;
    }
    setPublishDialogState({
      conversationId: workspace.conversationId,
      open: true,
      phase: "confirm",
    });
  };
  const confirmCommitLocally = () => {
    if (localCommitDisabled) return;
    void confirm({
      title: "Commit workspace locally?",
      description: `This commits the isolated branch ${branch} only. It will not push, open a pull request, or merge ${base}.`,
      confirmText: "Commit locally",
      pendingText: "Committing...",
      onConfirm: () => commitLocallyMutation.mutateAsync(),
    });
  };
  const handleConfirmPublishWorkspace = () => {
    const publishConversationId = workspace.conversationId;
    setPublishDialogState({
      conversationId: publishConversationId,
      open: true,
      phase: "publishing",
    });
    void Promise.resolve(onPublishWorkspace!(publishConversationId))
      .finally(() => {
        setPublishDialogState((current) =>
          current?.conversationId === publishConversationId ? null : current,
        );
      });
  };
  const handlePublishDialogOpenChange = (open: boolean) => {
    if (!open) {
      const dialogConversationId = workspace.conversationId;
      setPublishDialogState((current) => {
        if (current?.conversationId !== dialogConversationId) {
          return current;
        }
        if (!isPublishingThisWorkspace) {
          return null;
        }
        return {
          ...current,
          open: false,
        };
      });
    }
  };
  const primaryActionClassName = "h-9 gap-2 px-3 text-xs";
  const handleSubTabValueChange = (value: string) => {
    if (
      value !== "changes" &&
      value !== "review" &&
      value !== "checks" &&
      value !== "history" &&
      value !== "automation"
    ) {
      return;
    }
    onSubTabChange(value);
  };
  const changedFileCount = reviewQuery.isSuccess ? changes.length : null;
  const publishChangeFacts =
    terminalPublicationStatus || isRepairPending || blocksGitInspection
      ? null
      : getAgentWorkspaceChangeFacts(
          changeSummaryQuery.data,
          reviewQuery.data,
        );
  const publishAutomationStatus =
    !maintenancePresentation &&
    shouldShowPrSupervisionControls &&
    prSupervisionStatusLabel
      ? {
          label: prSupervisionStatusLabel,
          tone:
            autoMergeGuardSummary?.status === "error" ||
            prSupervisionStatus === "blocked"
              ? ("error" as const)
              : hasPrConflict ||
                  !autoPublishEnabled ||
                  autoMergeGuardSummary?.status === "active"
                ? ("warning" as const)
                : isAutoPublishSaving ||
                    isPrSupervisionSaving ||
                    prSupervisionStatus === "fixing" ||
                    prSupervisionStatus === "waiting_for_checks" ||
                    prAutofixEnabled ||
                    prAutoMergeDesired
                  ? ("accent" as const)
                  : ("neutral" as const),
          live:
            isAutoPublishSaving ||
            isPrSupervisionSaving ||
            prSupervisionStatus === "fixing" ||
            prSupervisionStatus === "waiting_for_checks" ||
            autoMergeGuardSummary?.status === "pending",
        }
      : null;

  return (
    <div className="flex h-full flex-col p-4" data-testid="agents-publish-pane">
      <Tabs
        className="@container flex w-full min-h-0 flex-1 flex-col"
        value={activeSubTab}
        onValueChange={handleSubTabValueChange}
      >
        <section className="sticky top-0 z-20">
          <AgentsPublishActionBar
            presentation={publishPresentation}
            changeFacts={publishChangeFacts}
            automationStatus={publishAutomationStatus}
            liveAnnouncement={maintenanceLiveAnnouncement}
            primaryAction={
              <>
              {maintenancePresentation?.action === "none" ? (
                <Button
                  type="button"
                  variant="ghost"
                  className={`${primaryActionClassName} ${STATUS_ACTION_BUTTON_CLASSNAME}`}
                  style={statusActionButtonStyle(maintenancePresentation.tone)}
                  disabled
                  data-testid="agents-publish-maintenance-active"
                >
                  <Loader2 className="h-3.5 w-3.5 animate-spin" />
                  {maintenancePresentation.title}
                </Button>
              ) : maintenancePresentation?.action === "retry" ? (
                <Button
                  type="button"
                  className={primaryActionClassName}
                  onClick={confirmPublishWorkspace}
                  disabled={!onPublishWorkspace || isManagedByTaskPipeline}
                  data-testid="agents-publish-retry-maintenance"
                >
                  <AlertTriangle className="h-3.5 w-3.5" />
                  Retry repair
                </Button>
              ) : maintenancePresentation?.action === "publish" ? (
                <Button
                  type="button"
                  className={primaryActionClassName}
                  onClick={confirmPublishWorkspace}
                  disabled={!onPublishWorkspace || isManagedByTaskPipeline}
                  data-testid="agents-publish-resume-maintenance"
                >
                  <GitPullRequestArrow className="h-3.5 w-3.5" />
                  Commit & Publish
                </Button>
              ) : isRepairPending ? (
                <Button
                  type="button"
                  variant="ghost"
                  className={`${primaryActionClassName} ${STATUS_ACTION_BUTTON_CLASSNAME}`}
                  style={statusActionButtonStyle("warning")}
                  disabled
                  data-testid="agents-publish-repair-pending"
                >
                  <AlertTriangle className="h-3.5 w-3.5" />
                  Repair pending
                </Button>
              ) : isPublishingThisWorkspace ? (
                <Button
                  type="button"
                  className={primaryActionClassName}
                  disabled
                  data-testid="agents-publish-confirm"
                >
                  <Loader2 className="h-3.5 w-3.5 animate-spin" />
                  {publishButtonLabel}
                </Button>
              ) : hasPrConflict ? (
                <Button
                  type="button"
                  className={primaryActionClassName}
                  onClick={confirmResolvePrConflicts}
                  disabled={
                    effectivePublishing ||
                    isAutomationPreferenceSaving ||
                    workspace.status === "missing"
                  }
                  data-testid="agents-resolve-pr-conflicts"
                >
                  {isUpdatingFromBase ? (
                    <Loader2 className="h-3.5 w-3.5 animate-spin" />
                  ) : (
                    <GitBranch className="h-3.5 w-3.5" />
                  )}
                  Resolve conflicts
                </Button>
              ) : isBranchUpdateNeeded ? (
                <Button
                  type="button"
                  className={primaryActionClassName}
                  onClick={confirmUpdateFromBase}
                  disabled={
                    baseBlocked ||
                    effectivePublishing ||
                    (isRepairPending && !isPipelineOwnedWorkspace)
                  }
                  data-testid="agents-update-from-base"
                >
                  {isUpdatingFromBase ? (
                    <Loader2 className="h-3.5 w-3.5 animate-spin" />
                  ) : (
                    <GitBranch className="h-3.5 w-3.5" />
                  )}
                  Update from {baseActionLabel}
                </Button>
              ) : baseBlocked ? (
                <Button
                  type="button"
                  className={primaryActionClassName}
                  onClick={() => setRebaseDialogOpen(true)}
                  disabled={effectivePublishing}
                  data-testid="agents-rebase-from-base"
                >
                  {isUpdatingFromBase ? (
                    <Loader2 className="h-3.5 w-3.5 animate-spin" />
                  ) : (
                    <GitBranch className="h-3.5 w-3.5" />
                  )}
                  Rebase branch
                </Button>
              ) : repositoryInspectionFailed ? (
                <Button
                  type="button"
                  className={primaryActionClassName}
                  disabled
                  data-testid="agents-publish-unavailable"
                >
                  <AlertTriangle className="h-3.5 w-3.5" />
                  Repository setup required
                </Button>
              ) : reviewBlocksPublish ? (
                <Button
                  type="button"
                  className={primaryActionClassName}
                  onClick={onOpenReview}
                  disabled={!onOpenReview}
                  data-testid={
                    reviewIsRunning
                      ? "agents-publish-reviewing"
                      : "agents-publish-review-required"
                  }
                >
                  {reviewIsRunning ? (
                    <Loader2 className="h-3.5 w-3.5 animate-spin" />
                  ) : (
                    <AlertTriangle className="h-3.5 w-3.5" />
                  )}
                  {publishButtonLabel}
                </Button>
              ) : isLocalCommitPrimary ? (
                <Button
                  type="button"
                  className={primaryActionClassName}
                  onClick={confirmCommitLocally}
                  disabled={localCommitDisabled}
                  data-testid="agents-commit-locally"
                >
                  {commitLocallyMutation.isPending ? (
                    <Loader2 className="h-3.5 w-3.5 animate-spin" />
                  ) : (
                    <GitBranch className="h-3.5 w-3.5" />
                  )}
                  Commit locally
                </Button>
              ) : (
                <Button
                  type="button"
                  variant={publishDisabled ? "ghost" : undefined}
                  className={
                    publishDisabled
                      ? `${primaryActionClassName} ${STATUS_ACTION_BUTTON_CLASSNAME}`
                      : primaryActionClassName
                  }
                  style={
                    publishDisabled
                      ? statusActionButtonStyle(publishPresentation.tone)
                      : undefined
                  }
                  onClick={confirmPublishWorkspace}
                  disabled={publishDisabled}
                  data-testid="agents-publish-confirm"
                >
                  {isPublishingThisWorkspace ? (
                    <Loader2 className="h-3.5 w-3.5 animate-spin" />
                  ) : isPublishCurrent || terminalPublicationStatus ? (
                    <CheckCircle2 className="h-3.5 w-3.5" />
                  ) : (
                    <GitPullRequestArrow className="h-3.5 w-3.5" />
                  )}
                  {baseBlocked
                    ? "Base unavailable"
                    : publishButtonLabel}
                </Button>
              )}
              </>
            }
            overflowAction={
              <>
              {canClosePr && (
                <DropdownMenu>
                  <Tooltip>
                    <TooltipTrigger asChild>
                      <DropdownMenuTrigger asChild>
                        <Button
                          type="button"
                          variant="ghost"
                          className="h-9 w-7 p-0 border-0 bg-transparent hover:bg-[var(--bg-hover)]"
                          disabled={isClosingPr || effectivePublishing}
                          aria-label="Publish actions"
                          data-testid="agents-publish-actions-menu"
                        >
                          {isClosingPr ? (
                            <Loader2 className="h-3.5 w-3.5 animate-spin" />
                          ) : (
                            <MoreVertical className="h-3.5 w-3.5" />
                          )}
                        </Button>
                      </DropdownMenuTrigger>
                    </TooltipTrigger>
                    <TooltipContent>Publish actions</TooltipContent>
                  </Tooltip>
                  <DropdownMenuContent align="end" className="min-w-[160px]">
                    <DropdownMenuItem
                      data-testid="agents-close-pr"
                      onSelect={(event) => {
                        event.preventDefault();
                        confirmClosePr();
                      }}
                      disabled={isClosingPr || effectivePublishing}
                    >
                      <XCircle className="h-3.5 w-3.5" />
                      Close PR
                    </DropdownMenuItem>
                  </DropdownMenuContent>
                </DropdownMenu>
              )}
              </>
            }
          />
          <TabsList
            className="mt-3 flex h-10 w-full min-w-0 justify-start gap-5 overflow-x-auto rounded-none border-y bg-transparent p-0 text-[var(--text-muted)]"
            style={{
              borderColor: "var(--border-subtle)",
              borderStyle: "solid",
              borderWidth: "1px 0",
            }}
            aria-label="Commit and publish sections"
            data-testid="agents-publish-tabs"
          >
            <TabsTrigger
              value="changes"
              className="relative h-full gap-2 rounded-none border-0 bg-transparent px-1 text-xs font-medium text-[var(--text-muted)] shadow-none after:absolute after:inset-x-0 after:bottom-0 after:h-0.5 after:scale-x-0 after:bg-[var(--accent-primary)] after:transition-transform focus-visible:ring-2 focus-visible:ring-[var(--focus-ring)] focus-visible:ring-offset-0 data-[state=active]:bg-transparent data-[state=active]:text-[var(--text-primary)] data-[state=active]:shadow-none data-[state=active]:after:scale-x-100"
              data-testid="agents-publish-tab-changes"
            >
              <Files className="h-3.5 w-3.5" aria-hidden="true" />
              <span>Changes</span>
              {changedFileCount !== null && (
                <span
                  className="rounded-full px-1.5 py-0.5 text-[0.625rem] font-semibold"
                  style={{
                    backgroundColor: "var(--bg-elevated)",
                    color: "var(--text-secondary)",
                  }}
                >
                  {changedFileCount}
                </span>
              )}
            </TabsTrigger>
            {showReviewTab && (
              <TabsTrigger
                value="review"
                className="relative h-full gap-2 rounded-none border-0 bg-transparent px-1 text-xs font-medium text-[var(--text-muted)] shadow-none after:absolute after:inset-x-0 after:bottom-0 after:h-0.5 after:scale-x-0 after:bg-[var(--accent-primary)] after:transition-transform focus-visible:ring-2 focus-visible:ring-[var(--focus-ring)] focus-visible:ring-offset-0 data-[state=active]:bg-transparent data-[state=active]:text-[var(--text-primary)] data-[state=active]:shadow-none data-[state=active]:after:scale-x-100"
                data-testid="agents-publish-tab-review"
              >
                <ShieldCheck
                  className={
                    isReviewTabRunning
                      ? "h-3.5 w-3.5 animate-pulse"
                      : "h-3.5 w-3.5"
                  }
                  style={
                    reviewTabStatusColor
                      ? { color: reviewTabStatusColor }
                      : undefined
                  }
                  aria-hidden="true"
                />
                <span>Review</span>
                {reviewTabStatusLabel && (
                  <span
                    className="rounded-full border px-1.5 py-0.5 text-[0.625rem] font-semibold"
                    style={{
                      backgroundColor: "var(--bg-elevated)",
                      borderColor:
                        reviewTabStatusColor ?? "var(--border-subtle)",
                      borderStyle: "solid",
                      borderWidth: 1,
                      color: reviewTabStatusColor ?? "var(--text-secondary)",
                    }}
                  >
                    {reviewTabStatusLabel}
                  </span>
                )}
              </TabsTrigger>
            )}
            {hasPublishedPr && checksSelector && (
              <TabsTrigger
                value="checks"
                className="relative h-full gap-2 rounded-none border-0 bg-transparent px-1 text-xs font-medium text-[var(--text-muted)] shadow-none after:absolute after:inset-x-0 after:bottom-0 after:h-0.5 after:scale-x-0 after:bg-[var(--accent-primary)] after:transition-transform focus-visible:ring-2 focus-visible:ring-[var(--focus-ring)] focus-visible:ring-offset-0 data-[state=active]:bg-transparent data-[state=active]:text-[var(--text-primary)] data-[state=active]:shadow-none data-[state=active]:after:scale-x-100"
                data-testid="agents-publish-tab-checks"
              >
                <ListChecks className="h-3.5 w-3.5" aria-hidden="true" />
                <span>Checks</span>
                {checksAttentionCount > 0 && (
                  <span
                    aria-label={`${checksSummary.failed} failed and ${checksSummary.pending} pending checks`}
                    className="rounded-full px-1.5 py-0.5 text-[0.625rem] font-semibold"
                    style={{
                      backgroundColor: "var(--status-error-muted)",
                      color: "var(--status-error)",
                    }}
                  >
                    {checksAttentionCount}
                  </span>
                )}
              </TabsTrigger>
            )}
            <TabsTrigger
              value="history"
              className="relative h-full gap-2 rounded-none border-0 bg-transparent px-1 text-xs font-medium text-[var(--text-muted)] shadow-none after:absolute after:inset-x-0 after:bottom-0 after:h-0.5 after:scale-x-0 after:bg-[var(--accent-primary)] after:transition-transform focus-visible:ring-2 focus-visible:ring-[var(--focus-ring)] focus-visible:ring-offset-0 data-[state=active]:bg-transparent data-[state=active]:text-[var(--text-primary)] data-[state=active]:shadow-none data-[state=active]:after:scale-x-100"
              data-testid="agents-publish-tab-history"
            >
              <History className="h-3.5 w-3.5" aria-hidden="true" />
              <span>History</span>
              {publishHistoryCount > 0 ? (
                <span
                  aria-label={`${publishHistoryCount} publication events`}
                  className="rounded-full px-1.5 py-0.5 text-[0.625rem] font-semibold"
                  style={{
                    backgroundColor: "var(--bg-elevated)",
                    color: "var(--text-secondary)",
                  }}
                >
                  {publishHistoryCount}
                </span>
              ) : null}
            </TabsTrigger>
            {shouldShowPrSupervisionControls && (
              <TabsTrigger
                value="automation"
                className="relative ml-auto h-full gap-2 rounded-none border-0 bg-transparent px-1 text-xs font-medium text-[var(--text-muted)] shadow-none after:absolute after:inset-x-0 after:bottom-0 after:h-0.5 after:scale-x-0 after:bg-[var(--accent-primary)] after:transition-transform focus-visible:ring-2 focus-visible:ring-[var(--focus-ring)] focus-visible:ring-offset-0 data-[state=active]:bg-transparent data-[state=active]:text-[var(--text-primary)] data-[state=active]:shadow-none data-[state=active]:after:scale-x-100"
                data-testid="agents-publish-tab-automation"
              >
                <Zap className="h-3.5 w-3.5" aria-hidden="true" />
                <span>Automation</span>
                {(autoPublishEnabled ||
                  prAutofixEnabled ||
                  prAutoMergeDesired) && (
                  <span
                    aria-label="Automation active"
                    className="h-1.5 w-1.5 rounded-full"
                    style={{ backgroundColor: "var(--accent-primary)" }}
                  />
                )}
              </TabsTrigger>
            )}
          </TabsList>
          {shouldShowPublishNotices && hasPrConflict && (
            <div
              className="mt-3 flex items-start gap-2 rounded-md border px-3 py-2 text-xs leading-relaxed"
              style={{
                backgroundColor: "var(--bg-subtle)",
                borderColor: "var(--status-warning-border)",
                borderStyle: "solid",
                borderWidth: "1px",
                color: "var(--text-secondary)",
              }}
              data-testid="agents-pr-conflict"
            >
              <AlertTriangle
                aria-hidden="true"
                className="mt-0.5 h-3.5 w-3.5 shrink-0"
                style={{ color: "var(--status-warning)" }}
              />
              <span>{prConflictSummary}</span>
            </div>
          )}
          {shouldShowPublishNotices && autoMergeGuardSummary && (
            <div
              className="mt-3 flex items-start gap-2 rounded-md border px-3 py-2 text-xs leading-relaxed"
              style={{
                backgroundColor: "var(--bg-subtle)",
                borderColor: autoMergeGuardBorderColor,
                borderStyle: "solid",
                borderWidth: "1px",
                color: "var(--text-secondary)",
              }}
              data-testid="agents-publish-review-auto-merge-guard"
            >
              <AutoMergeGuardIcon
                aria-hidden="true"
                className={`mt-0.5 h-3.5 w-3.5 shrink-0${
                  autoMergeGuardSummary.status === "pending" ? " animate-spin" : ""
                }`}
                style={{ color: autoMergeGuardColor }}
              />
              <span>{autoMergeGuardSummary.detail}</span>
            </div>
          )}
          {shouldShowPublishNotices && isBranchUpdateNeeded && (
            <div
              className="mt-3 flex items-start gap-2 rounded-md border px-3 py-2 text-xs leading-relaxed"
              style={{
                background: "var(--bg-subtle)",
                borderColor: "var(--border-subtle)",
                color: "var(--text-secondary)",
              }}
              data-testid="agents-base-stale"
            >
              <AlertTriangle
                aria-hidden="true"
                className="mt-0.5 h-3.5 w-3.5 shrink-0"
                data-testid="agents-base-stale-icon"
                style={{ color: "var(--status-warning)" }}
              />
              <span>
                Base branch {freshness?.baseRef ?? baseActionLabel} has new commits.
              </span>
            </div>
          )}
          {shouldShowPublishNotices && baseRetargeted && (
            <div
              className="mt-3 flex items-start gap-2 rounded-md border px-3 py-2 text-xs leading-relaxed"
              style={{
                background: "var(--bg-subtle)",
                borderColor: "var(--border-subtle)",
                color: "var(--text-secondary)",
              }}
              data-testid="agents-base-retargeted"
            >
              <GitBranch
                aria-hidden="true"
                className="mt-0.5 h-3.5 w-3.5 shrink-0"
                style={{ color: "var(--accent-primary)" }}
              />
              <span>Base branch retargeted to {base}.</span>
            </div>
          )}
          {shouldShowPublishNotices && baseBlocked && (
            <div
              className="mt-3 flex items-start gap-2 rounded-md border px-3 py-2 text-xs leading-relaxed"
              style={{
                background: "var(--bg-subtle)",
                borderColor: "var(--status-warning-border)",
                color: "var(--text-secondary)",
              }}
              data-testid="agents-base-blocked"
            >
              <AlertTriangle
                aria-hidden="true"
                className="mt-0.5 h-3.5 w-3.5 shrink-0"
                style={{ color: "var(--status-warning)" }}
              />
              <span>
                {freshness?.baseBlockReason ??
                  "This workspace base branch cannot be resolved safely."}
              </span>
            </div>
          )}
        </section>
        {mountedSubTabsForConversation.changes && (
          <TabsContent
            value="changes"
            forceMount
            className="m-0 flex min-h-0 flex-1 flex-col gap-4 pt-4 data-[state=inactive]:hidden"
            data-testid="agents-publish-content-changes"
          >
        {prAnnotationSourcesUnavailable.length > 0 && (
          <div
            className="rounded-md px-2.5 py-1.5 text-[0.6875rem]"
            data-testid="agents-pr-annotations-partial-warning"
            style={{
              backgroundColor: "var(--bg-subtle)",
              borderColor: "var(--status-warning-border)",
              borderStyle: "solid",
              borderWidth: "1px",
              color: "var(--text-secondary)",
            }}
          >
            GitHub annotations partially unavailable
          </div>
        )}
        {shouldShowPublishPipeline && (
          <PublishPipelineSteps
            autoMergeCurrent={prAutoMergeCurrent}
            autoMergeDesired={prAutoMergeDesired}
            className="mt-0"
            prSupervisionStatus={prSupervisionStatus}
            receiptPhase={workspace.publicationMetadataPhase}
            receiptState={workspace.publicationMetadataState}
            targetPullRequestLabel={publishTargetPullRequestLabel}
            status={pipelineStatus}
            isPublishing={effectivePublishing}
          />
        )}

        <GitAuthRepairPanel
          projectId={workspace.projectId}
          surface="publish"
          requiresGhAuth
        />


        {/* Inline diff view — below the action row, all files expanded by default */}
        {isRepairPending && inlineDiffsCandidate ? (
          <AgentsPublishRepairState
            conversationId={workspace.conversationId}
            canHydratePublishFacts={canHydratePublishFacts}
            focusRequest={publishFocusRequest}
          />
        ) : inlineDiffsCandidate && !baseBlocked ? (
          <section
            className="flex min-h-0 flex-1 flex-col overflow-hidden rounded-lg border"
            data-testid="agents-publish-inline-diffs-section"
            style={{
              backgroundColor: "var(--bg-surface)",
              borderColor: "var(--border-subtle)",
            }}
          >
            <AgentsPublishInlineDiffs
              key={`${conversationId ?? "missing"}:${terminalPublicationStatus ?? "active"}`}
              conversationId={conversationId ?? ""}
              review={reviewQuery.data ?? null}
              commits={commits}
              isLoading={Boolean(conversationId) && (!canHydratePublishFacts || reviewQuery.isLoading)}
              annotations={prAnnotations}
              hunkAnnotations={workspaceReviewHunkAnnotations}
              error={reviewQuery.error}
              onOpenInDialog={() => setReviewOpen(true)}
              focusRequest={publishFocusRequest}
              liveSummary={changeSummaryQuery.data ?? null}
              {...(inlineDiffDefaultMode !== undefined && {
                defaultMode: inlineDiffDefaultMode,
              })}
              {...(cumulativeModeLabel !== undefined && {
                cumulativeModeLabel,
              })}
              {...(isPublishCurrent && {
                workspaceChangeLabel: "Published changes",
              })}
            />
          </section>
        ) : null}

          </TabsContent>
        )}
        {showReviewTab && mountedSubTabsForConversation.review && (
          <TabsContent
            value="review"
            forceMount
            className="m-0 min-h-0 flex-1 overflow-y-auto pt-4 data-[state=inactive]:hidden"
            data-testid="agents-publish-content-review"
          >
            {reviewContent}
          </TabsContent>
        )}
        {hasPublishedPr &&
          checksSelector &&
          mountedSubTabsForConversation.checks && (
            <TabsContent
              value="checks"
              forceMount
              className="m-0 min-h-0 flex-1 overflow-y-auto pt-4 data-[state=inactive]:hidden"
              data-testid="agents-publish-content-checks"
            >
              <AgentsPublishChecksTab
                detail={checksDetail}
                isError={checksDetailQuery.isError}
                isLoading={Boolean(
                  canHydrateChecks &&
                    !checksDetail &&
                    (checksDetailQuery.isLoading ||
                      checksDetailQuery.fetchStatus !== "idle"),
                )}
                isReady={canHydrateChecks}
              />
            </TabsContent>
          )}
        {mountedSubTabsForConversation.history && (
          <TabsContent
            value="history"
            forceMount
            className="m-0 min-h-0 flex-1 overflow-y-auto pt-4 data-[state=inactive]:hidden"
            data-testid="agents-publish-content-history"
          >
            <PublishEventLog
              events={publicationEvents}
              isLoading={isPublicationEventsLoading}
              isPublishing={effectivePublishing}
              currentAttemptId={workspace.publicationMetadataAttemptId}
            />
          </TabsContent>
        )}
        {shouldShowPrSupervisionControls &&
          mountedSubTabsForConversation.automation && (
            <TabsContent
              value="automation"
              forceMount
              className="m-0 min-h-0 flex-1 overflow-y-auto pt-4 data-[state=inactive]:hidden"
              data-testid="agents-publish-content-automation"
            >
              <AgentsPublishAutomationTab
                workspace={workspace}
                hasPublishedPr={hasPublishedPr}
                isPipelinePrAutomationWorkspace={
                  isPipelinePrAutomationWorkspace
                }
                canConfigurePrSupervision={canConfigurePrSupervision}
                hasUncommittedChanges={Boolean(
                  freshness?.hasUncommittedChanges,
                )}
                terminalPrLabel={terminalPrLabel}
                onSnapshotChange={setAutomationSnapshot}
              />
            </TabsContent>
          )}
      </Tabs>
      <Dialog open={reviewOpen} onOpenChange={setReviewOpen}>
        <DialogContent
          className="flex h-[95vh] w-[95vw] max-w-[95vw] flex-col gap-0 overflow-hidden p-0"
          style={{
            backgroundColor: "var(--bg-surface)",
            border: "1px solid var(--border-subtle)",
          }}
        >
          <DialogTitle className="sr-only">Review workspace changes</DialogTitle>
          <DialogDescription className="sr-only">
            Inspect changed files and commits before publishing this agent workspace.
          </DialogDescription>
          {reviewOpen && (
            <Suspense fallback={<EmptyArtifactState title="Loading workspace diff..." />}>
              <LazyDiffViewer
                changes={changes}
                commits={commits}
                defaultTab={changes.length === 0 && !changesError ? "history" : "changes"}
                {...(changesError ? {
                  changesEmptyTitle: "Could not load workspace changes",
                  changesEmptySubtitle: changesError instanceof Error ? changesError.message : String(changesError),
                } : {})}
                commitFiles={commitFiles}
                annotations={prAnnotations}
                hunkAnnotations={workspaceReviewHunkAnnotations}
                onFetchDiff={async (filePath, commitSha) => {
                  if (!conversationId) {
                    return null;
                  }
                  const diff = commitSha
                    ? await diffApi.getAgentConversationWorkspaceCommitFileDiff(
                        conversationId,
                        commitSha,
                        filePath,
                      )
                    : await diffApi.getAgentConversationWorkspaceFileDiff(
                        conversationId,
                        filePath,
                      );
                  return {
                    filePath: diff.filePath,
                    hunks: diff.hunks,
                    oldTotalLines: diff.oldTotalLines,
                    newTotalLines: diff.newTotalLines,
                    isBinary: diff.isBinary,
                    language: diff.language,
                  };
                }}
                onFetchCommitFiles={async (commitSha) => {
                  if (!conversationId) {
                    setCommitFiles([]);
                    return;
                  }
                  setIsLoadingCommitFiles(true);
                  setCommitFiles([]);
                  try {
                    setCommitFiles(
                      await diffApi.getAgentConversationWorkspaceCommitFileChanges(
                        conversationId,
                        commitSha,
                      ),
                    );
                  } catch {
                    setCommitFiles([]);
                  } finally {
                    setIsLoadingCommitFiles(false);
                  }
                }}
                isLoadingChanges={reviewQuery.isLoading}
                isLoadingHistory={reviewQuery.isLoading}
                isLoadingCommitFiles={isLoadingCommitFiles}
                changesLabel="Workspace Changes"
                changesEmptyTitle="No workspace changes"
                changesEmptySubtitle="There are no changed files to review for this agent branch."
                {...(conversationId != null && {
                  conversationId,
                  changesRefKind: { kind: "head" as const },
                })}
              />
            </Suspense>
          )}
        </DialogContent>
      </Dialog>
      <Dialog open={rebaseDialogOpen} onOpenChange={setRebaseDialogOpen}>
        <DialogContent
          className="w-[min(460px,calc(100vw-2rem))] p-4"
          style={{
            backgroundColor: "var(--bg-surface)",
            border: "1px solid var(--border-subtle)",
          }}
        >
          <DialogTitle>Rebase branch</DialogTitle>
          <DialogDescription>
            Choose the base branch for {branch}. Project default is selected first.
          </DialogDescription>
          <div className="mt-3 flex flex-col gap-2">
            <BranchBasePicker
              value={resolvedRebaseBaseKey}
              onValueChange={setSelectedRebaseBaseKey}
              options={rebaseBaseOptions}
              placeholder={
                rebaseBaseOptionsQuery.isLoading ? "Loading branches..." : "Base branch"
              }
              disabled={isUpdatingFromBase || rebaseBaseOptions.length === 0}
              testId="agents-rebase-base-select"
              align="start"
              prefixLabel="Rebase from"
              ariaLabel="Rebase from"
              className="w-full max-w-full justify-start rounded-md border border-[var(--border-subtle)] px-3 py-2"
            />
            <p className="text-xs leading-relaxed text-[var(--text-muted)]">
              {selectedRebaseBase?.detail ?? selectedRebaseBase?.selection.ref ?? ""}
            </p>
          </div>
          <div className="mt-4 flex justify-end gap-2">
            <Button
              type="button"
              variant="ghost"
              className="h-9 px-3 text-xs"
              onClick={() => setRebaseDialogOpen(false)}
              disabled={isUpdatingFromBase}
            >
              Cancel
            </Button>
            <Button
              type="button"
              className="h-9 gap-2 px-3 text-xs"
              onClick={rebaseFromSelectedBase}
              disabled={
                isUpdatingFromBase ||
                rebaseBaseOptionsQuery.isLoading ||
                !selectedRebaseBase
              }
            >
              {isUpdatingFromBase ? (
                <Loader2 className="h-3.5 w-3.5 animate-spin" />
              ) : (
                <GitBranch className="h-3.5 w-3.5" />
              )}
              {isUpdatingFromBase ? "Rebasing..." : "Rebase branch"}
            </Button>
          </div>
        </DialogContent>
      </Dialog>
      <PublishWorkspaceDialog
        autoMergeCurrent={prAutoMergeCurrent}
        autoMergeDesired={prAutoMergeDesired}
        open={publishDialogOpen}
        phase={publishDialogPhase}
        branch={branch}
        base={base}
        targetPullRequestLabel={publishTargetPullRequestLabel}
        prSupervisionStatus={prSupervisionStatus}
        status={pipelineStatus}
        isPublishing={isPublishingThisWorkspace}
        confirmDisabled={publishDisabled}
        onConfirm={handleConfirmPublishWorkspace}
        onOpenChange={handlePublishDialogOpenChange}
      />
      <ConfirmationDialog {...confirmationDialogProps} />
    </div>
  );
}
