import {
  AlertTriangle,
  CheckCircle2,
  FileText,
  GitPullRequestArrow,
  GitBranch,
  Loader2,
  Maximize2,
  MoreVertical,
  XCircle,
} from "lucide-react";
import { lazy, Suspense, useEffect, useMemo, useRef, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { openUrl } from "@tauri-apps/plugin-opener";
import { toast } from "sonner";

import { diffApi } from "@/api/diff";
import {
  chatApi,
  type AgentConversationBaseSelection,
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
import { Tooltip, TooltipContent, TooltipTrigger, TooltipProvider } from "@/components/ui/tooltip";
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
import { PublishFact } from "./AgentsPublishFact";
import { PublishPipelineSteps } from "./AgentsPublishPipelineSteps";
import {
  PublishWorkspaceDialog,
  type PublishWorkspaceDialogPhase,
} from "./AgentsPublishWorkspaceDialog";
import { AgentsPublishInlineDiffs } from "./AgentsPublishInlineDiffs";
import { formatPullRequestUrlLabel } from "./agentPublishFormatting";
import {
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
  projectBaseBranch,
  onPublishWorkspace,
  isPublishingWorkspace,
  publishFocusRequest,
}: {
  workspace: AgentConversationWorkspace | null;
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
  const updateFromBaseMutation = useMutation({
    mutationFn: (base?: AgentConversationBaseSelection | null) =>
      base
        ? chatApi.updateAgentConversationWorkspaceFromBase(conversationId!, base)
        : chatApi.updateAgentConversationWorkspaceFromBase(conversationId!),
    onSuccess: async (result) => {
      setRebaseDialogOpen(false);
      queryClient.setQueryData(
        ["agents", "conversation-workspace", result.workspace.conversationId],
        result.workspace,
      );
      await invalidateWorkspaceQueries(queryClient, result.workspace.conversationId);
      toast.success(
        result.updated
          ? `Updated from ${result.targetRef}`
          : `Already current with ${result.targetRef}`,
      );
    },
    onError: (error) => {
      toast.error(
        error instanceof Error ? error.message : "Failed to update from base",
      );
      if (conversationId) {
        void invalidateWorkspaceQueries(queryClient, conversationId);
      }
    },
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
  const isUpdatingFromBase = updateFromBaseMutation.isPending;
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
  const workspacePipelineStatus =
    isPublishingThisWorkspace &&
    !PUBLISH_PIPELINE_EVENT_STEPS.has(workspace.publicationPushStatus ?? "")
      ? "checking"
      : workspace.publicationPushStatus;
  const pipelineStatus = isUpdatingFromBase
    ? "refreshing"
    : eventPipelineStatus ?? workspacePipelineStatus;
  const baseActionLabel =
    freshness?.effectiveBaseDisplayName ??
    freshness?.effectiveBaseRef ??
    freshness?.baseRef ??
    workspace.baseRef ??
    base;
  const shouldShowPublishPipeline =
    effectivePublishing || workspace.publicationPushStatus === "description_failed";
  const publishDisabled =
    !onPublishWorkspace ||
    isPipelineOwnedWorkspace ||
    effectivePublishing ||
    baseBlocked ||
    (isRepairPending && !isPipelineOwnedWorkspace) ||
    isPublishCurrent ||
    Boolean(terminalPublicationStatus) ||
    hasNoDetectedChanges ||
    workspace.status === "missing";
  const publishButtonLabel =
    terminalPublicationLabel ??
    (isPipelineOwnedWorkspace
      ? "Managed by Tasks"
      : isPublishCurrent
        ? "PR is up to date"
        : "Commit & Publish");
  const canClosePr =
    hasPublishedPr &&
    !terminalPublicationStatus;
  const isClosingPr = closePrMutation.isPending;
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
      pendingText: "Updating...",
      onConfirm: () => updateFromBaseMutation.mutateAsync(undefined),
    });
  };
  const rebaseFromSelectedBase = () => {
    if (!selectedRebaseBase) {
      toast.error("Select a base branch before rebasing");
      return;
    }
    updateFromBaseMutation.mutate(selectedRebaseBase.selection);
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
    if (!onPublishWorkspace) {
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
        toast.error(
          error instanceof Error ? error.message : "Failed to publish branch",
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
              <TooltipProvider>
                <Tooltip>
                  <TooltipTrigger asChild>
                    <Button
                      type="button"
                      variant="ghost"
                      size="icon"
                      className="h-9 w-9"
                      onClick={() => setReviewOpen(true)}
                      disabled={baseBlocked}
                      data-testid="agents-review-changes"
                      aria-label="Open changes in full diff dialog"
                    >
                      <Maximize2 className="h-3.5 w-3.5" aria-hidden="true" />
                    </Button>
                  </TooltipTrigger>
                  <TooltipContent side="top">
                    <p>Open in full dialog</p>
                  </TooltipContent>
                </Tooltip>
              </TooltipProvider>
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
          className="rounded-lg border p-4"
          style={{
            background: "var(--bg-surface)",
            borderColor: "var(--border-subtle)",
          }}
        >
          <div className="flex flex-wrap items-start justify-between gap-3">
            <div>
              <div className="text-sm font-semibold text-[var(--text-primary)]">
                Review Changes
              </div>
              <div className="mt-1 text-xs text-[var(--text-muted)]">
                {isPipelineOwnedWorkspace
                  ? "Review this ideation workspace's execution branch and pull request."
                  : "Review this agent workspace before publishing its draft PR."}
              </div>
            </div>
            <span
              className="rounded-full border px-2.5 py-1 text-xs capitalize"
              data-testid="agents-publish-status-pill"
              style={{
                borderColor: "var(--overlay-weak)",
                color: "var(--text-secondary)",
              }}
            >
              {terminalPublicationLabel ??
                (isBranchUpdateNeeded
                  ? "Behind base"
                  : workspace.publicationPushStatus ??
                    workspace.status)}
            </span>
          </div>

          <div className="mt-4 grid gap-3 @md:grid-cols-2">
            <PublishFact icon={GitBranch} label="Branch" value={branch} />
            <PublishFact
              icon={FileText}
              label="Base"
              value={base}
              description={freshness?.baseBlockReason ?? null}
            />
            <PublishFact
              icon={GitPullRequestArrow}
              label="Pull Request"
              value={prLabel}
              description={prUrlLabel}
              descriptionAction={
                workspace.publicationPrUrl
                  ? {
                      label: `Open ${prUrlLabel}`,
                      testId: "agents-open-pr-url",
                      onClick: async () => {
                        await openUrl(workspace.publicationPrUrl!);
                      },
                    }
                  : undefined
              }
              action={
                workspace.publicationPrUrl
                  ? {
                      label: "Open pull request",
                      testId: "agents-open-pr",
                      onClick: async () => {
                        await openUrl(workspace.publicationPrUrl!);
                      },
                    }
                  : undefined
              }
            />
            <PublishFact
              icon={CheckCircle2}
              label="Mode"
              value={
                workspace.mode === "edit"
                  ? "Edit agent"
                  : isPipelineOwnedWorkspace
                    ? "Ideation plan"
                    : workspace.mode
              }
            />
          </div>
          {prAnnotationSummary && (
            <div
              className="mt-3 rounded-md border px-3 py-2 text-xs"
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
              isLoading={Boolean(conversationId) && reviewQuery.isLoading}
              annotations={prAnnotations}
              error={reviewQuery.error}
              onOpenInDialog={() => setReviewOpen(true)}
              focusRequest={publishFocusRequest}
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
        open={publishDialogOpen}
        phase={publishDialogPhase}
        branch={branch}
        base={base}
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
