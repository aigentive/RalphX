import {
  AlertTriangle,
  CheckCircle2,
  ExternalLink,
  GitPullRequestArrow,
  GitBranch,
  Info,
  Loader2,
  MoreVertical,
  Settings2,
  XCircle,
} from "lucide-react";
import {
  type ReactNode,
  lazy,
  Suspense,
  useEffect,
  useMemo,
  useRef,
  useState,
} from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { openUrl } from "@tauri-apps/plugin-opener";
import { toast } from "sonner";

import { diffApi } from "@/api/diff";
import {
  chatApi,
  type AgentConversationWorkspace,
  type AgentConversationWorkspacePublicationEvent,
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
import { Switch } from "@/components/ui/switch";
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import { useUiStore } from "@/stores/uiStore";
import type { SettingsSectionId } from "@/components/settings/settings-registry";
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
import { useDeferredAgentHydration } from "./useDeferredAgentHydration";
import { EmptyArtifactState } from "./AgentsArtifactEmptyState";
import { PublishEventLog } from "./AgentsPublishEventLog";
import { PublishPipelineSteps } from "./AgentsPublishPipelineSteps";
import { AgentsPublishProgressToast } from "./AgentsPublishProgressToast";
import {
  PublishWorkspaceDialog,
  type PublishWorkspaceDialogPhase,
} from "./AgentsPublishWorkspaceDialog";
import { AgentsPublishInlineDiffs } from "./AgentsPublishInlineDiffs";
import { formatPullRequestUrlLabel } from "./agentPublishFormatting";
import {
  isAgentWorkspaceAutoMergeDeferred,
  isAgentWorkspaceAutoMergeRequestPending,
  getAgentWorkspaceTerminalPublicationLabel,
  getAgentWorkspaceTerminalPublicationStatus,
  getAgentWorkspaceEffectiveBaseLabel,
  hasPublishedWorkspacePr,
  isPipelineOwnedAgentWorkspace,
  isAgentWorkspacePublishCurrent,
} from "./agentWorkspacePublishState";
import {
  AGENT_WORKSPACE_FRESHNESS_STALE_MS,
  agentWorkspaceKeys,
  invalidateWorkspaceQueries,
} from "./agentWorkspaceQueries";
import type { AgentPublishFocusRequest } from "./agentPublishFocus";
import { mapReviewCommitsToDiffViewerCommits } from "./useAgentWorkspaceChangeSummary";
import {
  AGENT_WORKSPACE_OPERATION_ERROR_DURATION_MS,
  agentWorkspaceOperationErrorDetail,
  agentWorkspaceOperationToastId,
  agentWorkspaceOperationToastDescription,
  markAgentWorkspaceOperationToastSettled,
} from "./agentWorkspaceOperationToast";
import { useAgentWorkspaceBaseUpdate } from "./useAgentWorkspaceBaseUpdate";

const LazyDiffViewer = lazy(() =>
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

function PublishSwitchInfoTooltip({
  label,
  children,
  settingsSection,
}: {
  label: string;
  children: ReactNode;
  settingsSection?: SettingsSectionId;
}) {
  const openModal = useUiStore((s) => s.openModal);
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <button
          type="button"
          aria-label={label}
          className="inline-flex h-5 w-5 shrink-0 cursor-help items-center justify-center rounded-full border-0 bg-transparent p-0 text-[var(--text-muted)] transition-colors hover:text-[var(--text-secondary)] focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-[var(--focus-ring)]"
        >
          <Info className="h-3.5 w-3.5" aria-hidden="true" />
        </button>
      </TooltipTrigger>
      <TooltipContent
        side="top"
        align="center"
        className="max-w-[300px] text-xs leading-relaxed"
      >
        <div className="space-y-2">
          <div>{children}</div>
          {settingsSection && (
            <button
              type="button"
              className="inline-flex items-center gap-1.5 rounded-[6px] px-1.5 py-1 text-[11px] font-medium text-[var(--accent-primary)] hover:bg-[var(--bg-hover)] focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-[var(--focus-ring)]"
              onClick={() => openModal("settings", { section: settingsSection })}
              data-testid={`agents-tooltip-settings-${settingsSection}`}
            >
              <Settings2 className="h-3 w-3" aria-hidden="true" />
              Change defaults in Settings
            </button>
          )}
        </div>
      </TooltipContent>
    </Tooltip>
  );
}

function latestPublicationEventForActivePublish(
  events: AgentConversationWorkspacePublicationEvent[],
  publishStartedAtMs: number | null,
): AgentConversationWorkspacePublicationEvent | null {
  const candidates =
    publishStartedAtMs === null
      ? events
      : events.filter((event) => {
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

export function AgentPublishPanel({
  workspace,
  conversationTitle,
  projectBaseBranch,
  onPublishWorkspace,
  isPublishingWorkspace,
  publishFocusRequest,
}: {
  workspace: AgentConversationWorkspace | null;
  conversationTitle?: string | null;
  projectBaseBranch?: string | null;
  onPublishWorkspace: ((conversationId: string) => Promise<void>) | undefined;
  isPublishingWorkspace: boolean;
  publishFocusRequest?: AgentPublishFocusRequest | null;
}) {
  const queryClient = useQueryClient();
  const [reviewOpen, setReviewOpen] = useState(false);
  const [commitFiles, setCommitFiles] = useState<DiffViewerFileChange[]>([]);
  const [isLoadingCommitFiles, setIsLoadingCommitFiles] = useState(false);
  const [rebaseDialogOpen, setRebaseDialogOpen] = useState(false);
  const [publishDialogOpen, setPublishDialogOpen] = useState(false);
  const [publishDialogPhase, setPublishDialogPhase] =
    useState<PublishWorkspaceDialogPhase>("confirm");
  const [localPublishInFlight, setLocalPublishInFlight] = useState(false);
  const [localPublishStartedAtMs, setLocalPublishStartedAtMs] = useState<number | null>(
    null,
  );
  const prDescriptionPrecomputeKeysRef = useRef<Set<string>>(new Set());
  const [selectedRebaseBaseKey, setSelectedRebaseBaseKey] = useState("");
  const { confirm, confirmationDialogProps, ConfirmationDialog } = useConfirmation();
  const conversationId = workspace?.conversationId ?? null;
  const toastConversationTitle = conversationTitle?.trim() || null;
  const { isUpdatingFromBase, runUpdateFromBase } = useAgentWorkspaceBaseUpdate({
    conversationTitle,
  });
  const canHydratePublishFacts = useDeferredAgentHydration(conversationId);
  // Workspace-only flag computed early so reviewQuery can decide whether the
  // inline diff view will be visible.
  const inlineDiffsCandidate = workspace?.mode === "edit" && workspace.status !== "missing";
  const hasPublishedPr = hasPublishedWorkspacePr(workspace);
  const reviewQuery = useQuery({
    queryKey: agentWorkspaceKeys.review(conversationId),
    queryFn: () => diffApi.getAgentConversationWorkspaceReview(conversationId!),
    enabled:
      canHydratePublishFacts && !!conversationId && (reviewOpen || inlineDiffsCandidate),
    staleTime: 2_000,
  });
  const publicationEventsQuery = useQuery({
    queryKey: ["agents", "conversation-workspace-publication-events", conversationId],
    queryFn: () =>
      chatApi.listAgentConversationWorkspacePublicationEvents(conversationId!),
    enabled: canHydratePublishFacts && !!conversationId,
    staleTime: 0,
    refetchInterval: isPublishingWorkspace || localPublishInFlight ? 1_500 : false,
  });
  const prAnnotationsQuery = useQuery({
    queryKey: agentWorkspaceKeys.prAnnotations(conversationId),
    queryFn: () => diffApi.getAgentConversationWorkspacePrAnnotations(conversationId!),
    enabled: canHydratePublishFacts && !!conversationId && hasPublishedPr,
    staleTime: 30_000,
    refetchInterval: isPublishingWorkspace || localPublishInFlight ? 5_000 : false,
  });
  const terminalPublicationStatus =
    getAgentWorkspaceTerminalPublicationStatus(workspace);
  const terminalPublicationLabel =
    getAgentWorkspaceTerminalPublicationLabel(workspace);
  const isPipelineOwnedWorkspace = isPipelineOwnedAgentWorkspace(workspace);
  const isPipelinePrAutomationWorkspace =
    workspace?.mode === "ideation" && isPipelineOwnedWorkspace && hasPublishedPr;
  const freshnessQuery = useQuery({
    queryKey: agentWorkspaceKeys.scopedFreshness(conversationId, "full"),
    queryFn: () =>
      chatApi.getAgentConversationWorkspaceFreshness(conversationId!, {
        scope: "full",
      }),
    enabled:
      canHydratePublishFacts &&
      !!conversationId &&
      (workspace?.mode === "edit" || hasPublishedPr) &&
      !terminalPublicationStatus,
    staleTime: AGENT_WORKSPACE_FRESHNESS_STALE_MS,
  });
  const freshness = freshnessQuery.data;
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
  const autoPublishMutation = useMutation<
    AgentConversationWorkspace,
    Error,
    { autoPublishEnabled: boolean }
  >({
    mutationFn: (input) =>
      chatApi.setAgentConversationWorkspaceAutoPublish(conversationId!, input),
    onSuccess: (updatedWorkspace) => {
      queryClient.setQueryData(
        agentWorkspaceKeys.workspace(updatedWorkspace.conversationId),
        updatedWorkspace,
      );
      void queryClient.invalidateQueries({
        queryKey: agentWorkspaceKeys.publicationEvents(updatedWorkspace.conversationId),
      });
    },
    onError: (error) => {
      toast.error(
        error instanceof Error ? error.message : "Unable to update Auto Publish",
      );
    },
  });
  const prSupervisionMutation = useMutation<
    AgentConversationWorkspace,
    Error,
    { autoFixEnabled: boolean; autoMergeDesired: boolean }
  >({
    mutationFn: (input) =>
      chatApi.setAgentConversationWorkspacePrSupervision(conversationId!, {
        autoFixEnabled: input.autoFixEnabled,
        autoMergeDesired: input.autoMergeDesired,
        autoMergeMethod: workspace?.prAutoMergeMethod ?? "squash",
      }),
    onSuccess: (updatedWorkspace) => {
      queryClient.setQueryData(
        agentWorkspaceKeys.workspace(updatedWorkspace.conversationId),
        updatedWorkspace,
      );
      void queryClient.invalidateQueries({
        queryKey: agentWorkspaceKeys.publicationEvents(updatedWorkspace.conversationId),
      });
    },
    onError: (error) => {
      toast.error(
        error instanceof Error ? error.message : "Unable to update PR supervision",
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
  const prAnnotationSourcesUnavailable =
    prAnnotationsQuery.data?.sourcesUnavailable ?? [];
  const prAnnotationSummary =
    prAnnotations.length > 0
      ? `${prAnnotations.length} GitHub annotation${prAnnotations.length === 1 ? "" : "s"} synced`
      : prAnnotationSourcesUnavailable.length > 0
        ? "GitHub annotations partially unavailable"
        : prAnnotationsQuery.isLoading && hasPublishedPr
          ? "Checking GitHub annotations..."
          : null;
  const isChangesLoading =
    Boolean(conversationId) && reviewOpen && (!canHydratePublishFacts || reviewQuery.isLoading);
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
  const prLabel = workspace.publicationPrNumber
    ? `PR #${workspace.publicationPrNumber}`
    : workspace.publicationPrUrl
      ? "Published PR"
      : "No PR yet";
  const prUrlLabel = workspace.publicationPrUrl
    ? formatPullRequestUrlLabel(workspace.publicationPrUrl)
    : null;
  const baseRetargeted = baseStatus === "retargeted";
  const isBranchUpdateNeeded =
    !baseBlocked && !terminalPublicationStatus && Boolean(freshness?.isBaseAhead);
  const isPublishCurrent = isAgentWorkspacePublishCurrent(workspace, freshness);
  const isPublishingThisWorkspace = isPublishingWorkspace || localPublishInFlight;
  const effectivePublishing = isPublishingThisWorkspace || isUpdatingFromBase;
  const isRepairPending = workspace.publicationPushStatus === "needs_agent";
  const isDescriptionFailed = workspace.publicationPushStatus === "description_failed";
  const latestActivePublishEvent = latestPublicationEventForActivePublish(
    publicationEvents,
    localPublishStartedAtMs,
  );
  const eventPipelineStatus = isPublishingThisWorkspace
    ? pipelineStatusFromPublicationEvent(latestActivePublishEvent)
    : null;
  const localPublishFallbackStatus =
    localPublishStartedAtMs !== null && !eventPipelineStatus ? "checking" : null;
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
  const pendingAutoPublish = autoPublishMutation.isPending
    ? autoPublishMutation.variables
    : null;
  const storedAutoPublishEnabled = workspace.autoPublishEnabled ?? true;
  const initialAutoPublishEnabled = workspace.autoPublishInitialPrEnabled ?? false;
  const autoPublishEnabled =
    pendingAutoPublish?.autoPublishEnabled ??
    (hasPublishedPr ? storedAutoPublishEnabled : initialAutoPublishEnabled);
  const pendingPrSupervision = prSupervisionMutation.isPending
    ? prSupervisionMutation.variables
    : null;
  const isAutoPublishSaving = autoPublishMutation.isPending;
  const isPrSupervisionSaving =
    prSupervisionMutation.isPending || autoPublishMutation.isPending;
  const prAutofixEnabled =
    pendingPrSupervision?.autoFixEnabled ?? workspace.prAutofixEnabled ?? false;
  const prAutoMergeDesired =
    pendingPrSupervision?.autoMergeDesired ?? workspace.prAutoMergeDesired ?? false;
  const prAutoMergeCurrent = workspace.prAutoMergeCurrent ?? null;
  const prSupervisionStatus = workspace.prSupervisionStatus ?? null;
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
    effectivePublishing ||
    workspace.publicationPushStatus === "description_failed" ||
    shouldShowAutoMergeProgress ||
    shouldShowAutoMergeDeferred;
  const publishDisabled =
    !onPublishWorkspace ||
    isManagedByTaskPipeline ||
    effectivePublishing ||
    isPrSupervisionSaving ||
    baseBlocked ||
    (isRepairPending && !isPipelineOwnedWorkspace) ||
    isPublishCurrent ||
    Boolean(terminalPublicationStatus) ||
    (hasNoDetectedChanges && !isPipelinePrAutomationWorkspace) ||
    workspace.status === "missing";
  const publishButtonLabel =
    terminalPublicationLabel ??
    (isManagedByTaskPipeline
      ? "Managed by Tasks"
      : isPublishCurrent
        ? "PR is up to date"
        : "Commit & Publish");
  const canClosePr =
    hasPublishedPr &&
    !terminalPublicationStatus;
  const isClosingPr = closePrMutation.isPending;
  const shouldShowPrSupervisionControls =
    workspace.mode === "edit" || isPipelinePrAutomationWorkspace;
  const canConfigurePrSupervision =
    shouldShowPrSupervisionControls &&
    workspace.status !== "missing" &&
    !terminalPublicationStatus;
  const canConfigureAutoPublish = canConfigurePrSupervision;
  const prSupervisionStatusLabel = (() => {
    if (terminalPublicationStatus) return null;
    if (isAutoPublishSaving) return "Saving Auto Publish";
    if (isPrSupervisionSaving) return "Saving PR supervision";
    if (!hasPublishedPr && autoPublishEnabled) return "Auto Publish armed";
    if (!autoPublishEnabled && hasPublishedPr) return "Auto Publish paused";
    if (prSupervisionStatus === "fixing") return "Fixing PR";
    if (prSupervisionStatus === "waiting_for_checks") return "Waiting for checks";
    if (prSupervisionStatus === "blocked") return "PR supervision blocked";
    if (prAutofixEnabled || prAutoMergeDesired) return "Monitoring PR";
    return null;
  })();
  const updatePrSupervisionPreferences = (next: {
    autoFixEnabled: boolean;
    autoMergeDesired: boolean;
  }) => {
    if (
      !hasPublishedPr ||
      !canConfigurePrSupervision ||
      !autoPublishEnabled ||
      isPrSupervisionSaving
    ) {
      return;
    }
    prSupervisionMutation.mutate(next);
  };
  const terminalPrLabel =
    workspace.publicationPrNumber != null
      ? `PR #${workspace.publicationPrNumber}`
      : "This pull request";
  const publishSummary =
    terminalPublicationStatus === "merged"
      ? `${terminalPrLabel} has been merged. By continuing this conversation, a new workspace branch will be created automatically.`
      : terminalPublicationStatus === "closed"
        ? `${terminalPrLabel} is closed. By continuing this conversation, a new workspace branch will be created automatically.`
        : baseBlocked
          ? "Publishing is blocked until the workspace base branch is resolved."
        : isPipelineOwnedWorkspace
          ? workspace.publicationPrNumber || workspace.publicationPrUrl
            ? `${terminalPrLabel} is managed by this ideation plan's task pipeline.`
            : "Publishing is managed by this ideation plan's task pipeline."
        : isDescriptionFailed
          ? "RalphX could not draft a PR description. No pull request was opened; retry Commit & Publish after reviewing the latest publish event."
        : hasPublishedPr && !autoPublishEnabled
          ? "Automatic publishing is paused. Manual Commit & Publish remains available."
        : !hasPublishedPr && autoPublishEnabled
          ? "Auto Publish will run Commit & Publish when the agent finishes."
        : isChangesLoading
          ? "Loading changed files..."
          : isPublishCurrent
            ? reviewQuery.isSuccess && changes.length > 0
              ? `${changes.length} changed file${changes.length === 1 ? "" : "s"} published for review.`
              : "Workspace is published and current."
            : reviewQuery.isSuccess && changes.length > 0
              ? `${changes.length} changed file${changes.length === 1 ? "" : "s"} ready for review.`
              : reviewQuery.isSuccess
                ? "No changed files detected yet."
                : "Review changes before publishing.";
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
  const confirmAutoPublishChange = (nextEnabled: boolean) => {
    if (!canConfigureAutoPublish || autoPublishMutation.isPending) {
      return;
    }
    const enablingDirtyWarning =
      nextEnabled && freshness?.hasUncommittedChanges
        ? " The next automatic trigger may commit current local workspace changes."
        : "";
    const isInitialPublishToggle = !hasPublishedPr;
    void confirm({
      title: isInitialPublishToggle
        ? nextEnabled
          ? "Enable Auto Publish?"
          : "Disable Auto Publish?"
        : nextEnabled
          ? "Resume Auto Publish?"
          : "Pause Auto Publish?",
      description: isInitialPublishToggle
        ? nextEnabled
          ? `When the agent finishes, RalphX will run Commit & Publish for this workspace and open a draft pull request.${enablingDirtyWarning}`
          : "RalphX will wait for manual Commit & Publish before opening the first pull request."
        : nextEnabled
          ? `Background publish, PR autofix, and auto-merge automation will resume for ${terminalPrLabel}.${enablingDirtyWarning}`
          : `Background publish, stale-base publish scans, PR autofix publishing, and auto-merge automation will pause for ${terminalPrLabel}. Manual Commit & Publish remains available.`,
      confirmText: isInitialPublishToggle
        ? nextEnabled
          ? "Enable Auto Publish"
          : "Disable Auto Publish"
        : nextEnabled
          ? "Resume Auto Publish"
          : "Pause Auto Publish",
      pendingText: "Saving...",
      onConfirm: () =>
        autoPublishMutation.mutateAsync({ autoPublishEnabled: nextEnabled }),
    });
  };
  const confirmPublishWorkspace = () => {
    if (!onPublishWorkspace || publishDisabled) {
      return;
    }
    setPublishDialogPhase("confirm");
    setPublishDialogOpen(true);
  };
  const handleConfirmPublishWorkspace = () => {
    setPublishDialogPhase("publishing");
    setLocalPublishStartedAtMs(Date.now());
    setLocalPublishInFlight(true);
    void Promise.resolve(onPublishWorkspace!(workspace.conversationId))
      .catch((error) => {
        const publishToastId = agentWorkspaceOperationToastId(
          workspace.conversationId,
          "publish",
        );
        const description = agentWorkspaceOperationToastDescription(
          toastConversationTitle,
          agentWorkspaceOperationErrorDetail(error, "Failed to publish branch"),
        );
        markAgentWorkspaceOperationToastSettled(publishToastId);
        toast.error(
          "Failed to publish branch",
          {
            closeButton: true,
            ...(description ? { description } : {}),
            dismissible: true,
            duration: AGENT_WORKSPACE_OPERATION_ERROR_DURATION_MS,
            id: publishToastId,
          },
        );
      })
      .finally(() => {
        setLocalPublishInFlight(false);
        setLocalPublishStartedAtMs(null);
        setPublishDialogOpen(false);
        setPublishDialogPhase("confirm");
      });
  };
  const handlePublishDialogOpenChange = (open: boolean) => {
    setPublishDialogOpen(open);
    if (!open && !isPublishingThisWorkspace) {
      setPublishDialogPhase("confirm");
    }
  };
  const primaryActionClassName = "h-9 gap-2 px-3 text-xs";

  return (
    <div className="flex h-full flex-col p-4" data-testid="agents-publish-pane">
      <AgentsPublishProgressToast
        active={isPublishingThisWorkspace}
        conversationTitle={toastConversationTitle}
        conversationId={conversationId}
        startedAtMs={localPublishStartedAtMs}
        status={pipelineStatus}
      />
      <div className="@container flex w-full min-h-0 flex-1 flex-col gap-4">
        <section
          className="sticky top-0 z-20 -mx-4 border-b px-4 py-4"
          data-testid="agents-publish-actionbar"
          style={{
            backgroundColor: "var(--bg-surface)",
            borderColor: "var(--border-subtle)",
          }}
        >
          <div className="flex flex-wrap items-center justify-between gap-3">
            <div>
              {terminalPublicationLabel && (
                <div className="text-sm font-semibold text-[var(--text-primary)]">
                  Pull Request {terminalPublicationLabel}
                </div>
              )}
              <div
                className={`text-xs leading-relaxed text-[var(--text-muted)]${
                  terminalPublicationLabel ? " mt-1" : ""
                }`}
              >
                {publishSummary}
              </div>
            </div>
            <div className="flex items-center gap-2">
              {isBranchUpdateNeeded ? (
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
              ) : (
                <Button
                  type="button"
                  className={primaryActionClassName}
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
              {canClosePr && (
                <DropdownMenu>
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
            </div>
          </div>
          {shouldShowPrSupervisionControls && (
            <div
              className="mt-3 flex flex-wrap items-center gap-x-4 gap-y-2 text-xs"
              data-testid="agents-pr-supervision-controls"
            >
              <div className="flex min-h-8 items-center gap-1.5 text-[var(--text-secondary)]">
                <label className="flex min-h-8 items-center gap-2">
                  <Switch
                    checked={autoPublishEnabled}
                    disabled={!canConfigureAutoPublish || isPrSupervisionSaving}
                    onCheckedChange={confirmAutoPublishChange}
                    aria-label="Auto Publish"
                    data-testid="agents-auto-publish-switch"
                  />
                  <span>Auto Publish</span>
                </label>
                <PublishSwitchInfoTooltip label="About Auto Publish">
                  {isPipelinePrAutomationWorkspace
                    ? "Controls PR autofix publishing and auto-merge automation for this task-managed PR."
                    : hasPublishedPr
                      ? "Controls background publishing for this PR, including publish-after-turn, stale-base scans, PR autofix publishing, and auto-merge automation."
                      : "Runs Commit & Publish automatically when the agent finishes before a pull request exists."}
                </PublishSwitchInfoTooltip>
              </div>
              {hasPublishedPr && (
                <>
                  <div className="flex min-h-8 items-center gap-1.5 text-[var(--text-secondary)]">
                    <label className="flex min-h-8 items-center gap-2">
                      <Switch
                        checked={prAutofixEnabled}
                        disabled={
                          !canConfigurePrSupervision ||
                          !autoPublishEnabled ||
                          isPrSupervisionSaving
                        }
                        onCheckedChange={(checked) =>
                          updatePrSupervisionPreferences({
                            autoFixEnabled: checked,
                            autoMergeDesired: prAutoMergeDesired,
                          })
                        }
                        aria-label="Autofix CI & Reviews"
                        data-testid="agents-pr-autofix-switch"
                      />
                      <span>Autofix CI &amp; Reviews</span>
                    </label>
                    <PublishSwitchInfoTooltip
                      label="About Autofix CI and Reviews"
                      settingsSection="execution"
                    >
                      RalphX monitors this PR for failing checks and review feedback, then
                      publishes follow-up fixes from the workspace automatically.
                    </PublishSwitchInfoTooltip>
                  </div>
                  <div className="flex min-h-8 items-center gap-1.5 text-[var(--text-secondary)]">
                    <label className="flex min-h-8 items-center gap-2">
                      <Switch
                        checked={prAutoMergeDesired}
                        disabled={
                          !canConfigurePrSupervision ||
                          !autoPublishEnabled ||
                          isPrSupervisionSaving
                        }
                        onCheckedChange={(checked) =>
                          updatePrSupervisionPreferences({
                            autoFixEnabled: prAutofixEnabled,
                            autoMergeDesired: checked,
                          })
                        }
                        aria-label="GitHub auto-merge"
                        data-testid="agents-pr-auto-merge-switch"
                      />
                      <span>GitHub auto-merge</span>
                    </label>
                    <PublishSwitchInfoTooltip
                      label="About GitHub auto-merge"
                      settingsSection="execution"
                    >
                      RalphX asks GitHub to merge the PR after required checks and review
                      requirements pass.
                    </PublishSwitchInfoTooltip>
                  </div>
                </>
              )}
              {prSupervisionStatusLabel && (
                <span
                  className="rounded-full border px-2 py-1 text-[11px] font-medium"
                  style={{
                    backgroundColor: "var(--bg-elevated)",
                    borderColor: "var(--border-subtle)",
                    borderStyle: "solid",
                    borderWidth: "1px",
                    color: "var(--text-muted)",
                  }}
                  data-testid="agents-pr-supervision-status"
                >
                  {prSupervisionStatusLabel}
                </span>
              )}
            </div>
          )}
          {isBranchUpdateNeeded && (
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
          {baseRetargeted && (
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
          {baseBlocked && (
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
        <section
          className="rounded-lg border px-3 py-2"
          style={{
            backgroundColor: "var(--bg-surface)",
            borderColor: "var(--border-subtle)",
            borderStyle: "solid",
            borderWidth: "1px",
          }}
          data-testid="agents-publish-metadata-strip"
        >
          <div className="flex flex-wrap items-center gap-x-3 gap-y-1.5 text-xs">
            <span className="inline-flex min-w-0 items-center gap-1.5" style={{ color: "var(--text-secondary)" }}>
              <GitBranch className="h-3 w-3 shrink-0" style={{ color: "var(--text-muted)" }} aria-hidden="true" />
              <span className="truncate font-medium" style={{ color: "var(--text-primary)" }}>{branch}</span>
              <span style={{ color: "var(--text-muted)" }} aria-hidden="true">&rarr;</span>
              <span className="truncate" style={{ color: "var(--text-muted)" }}>{base}</span>
            </span>

            <span className="hidden @xs:inline-block" style={{ color: "var(--border-subtle)" }} aria-hidden="true">|</span>

            <span className="inline-flex min-w-0 items-center gap-1.5" style={{ color: "var(--text-secondary)" }}>
              <GitPullRequestArrow className="h-3 w-3 shrink-0" style={{ color: "var(--text-muted)" }} aria-hidden="true" />
              {workspace.publicationPrUrl ? (
                <button
                  type="button"
                  className="inline-flex min-w-0 items-center gap-1 truncate bg-transparent p-0 text-left font-medium transition-colors hover:text-[var(--accent-primary)]"
                  style={{ color: "var(--text-primary)" }}
                  onClick={() => void openUrl(workspace.publicationPrUrl!)}
                  data-testid="agents-open-pr-url"
                  aria-label={`Open ${prUrlLabel ?? "pull request"}`}
                >
                  {prLabel}
                  <ExternalLink className="h-2.5 w-2.5 shrink-0" style={{ color: "var(--text-muted)" }} aria-hidden="true" />
                </button>
              ) : (
                <span className="truncate font-medium" style={{ color: "var(--text-primary)" }}>{prLabel}</span>
              )}
            </span>

            <span className="hidden @xs:inline-block" style={{ color: "var(--border-subtle)" }} aria-hidden="true">|</span>

            <span
              className="rounded-full border px-2 py-0.5 text-[0.6875rem] capitalize"
              data-testid="agents-publish-status-pill"
              style={{
                borderColor: "var(--overlay-weak)",
                color: "var(--text-secondary)",
              }}
            >
              {workspace.mode === "edit"
                ? "Edit"
                : isPipelineOwnedWorkspace
                  ? "Plan"
                  : workspace.mode}
            </span>

            <span
              className="rounded-full border px-2 py-0.5 text-[0.6875rem] capitalize"
              data-testid="agents-publish-push-status-pill"
              style={{
                borderColor: "var(--overlay-weak)",
                color: "var(--text-muted)",
              }}
            >
              {terminalPublicationLabel ??
                (isBranchUpdateNeeded
                  ? "Behind base"
                  : workspace.publicationPushStatus ??
                    workspace.status)}
            </span>
          </div>

          {prAnnotationSummary && (
            <div
              className="mt-2 rounded-md border px-2.5 py-1.5 text-[0.6875rem]"
              data-testid="agents-pr-annotations-summary"
              style={{
                backgroundColor: "var(--bg-subtle)",
                borderColor:
                  prAnnotations.length > 0
                    ? "var(--status-warning-border)"
                    : "var(--border-subtle)",
                color: "var(--text-secondary)",
              }}
            >
              {prAnnotationSummary}
            </div>
          )}
          {shouldShowPublishPipeline && (
            <PublishPipelineSteps
              autoMergeCurrent={prAutoMergeCurrent}
              autoMergeDesired={prAutoMergeDesired}
              prSupervisionStatus={prSupervisionStatus}
              status={pipelineStatus}
              isPublishing={effectivePublishing}
            />
          )}
        </section>

        <GitAuthRepairPanel
          projectId={workspace.projectId}
          surface="publish"
          requiresGhAuth
        />


        {/* Inline diff view — below the action row, all files expanded by default */}
        {inlineDiffsCandidate && !baseBlocked && (
          <section
            className="flex min-h-0 flex-1 flex-col overflow-hidden rounded-lg border"
            data-testid="agents-publish-inline-diffs-section"
            style={{
              backgroundColor: "var(--bg-surface)",
              borderColor: "var(--border-subtle)",
            }}
          >
            <AgentsPublishInlineDiffs
              conversationId={conversationId ?? ""}
              review={reviewQuery.data ?? null}
              commits={commits}
              isLoading={Boolean(conversationId) && (!canHydratePublishFacts || reviewQuery.isLoading)}
              annotations={prAnnotations}
              error={reviewQuery.error}
              onOpenInDialog={() => setReviewOpen(true)}
              focusRequest={publishFocusRequest}
              {...(isPublishCurrent && { workspaceChangeLabel: "Published changes" })}
            />
          </section>
        )}

        <PublishEventLog
          events={publicationEvents}
          isLoading={isPublicationEventsLoading}
          isPublishing={effectivePublishing}
        />
      </div>
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
