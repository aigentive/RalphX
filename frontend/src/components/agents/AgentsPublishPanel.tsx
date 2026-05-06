import {
  AlertTriangle,
  CheckCircle2,
  Code,
  FileText,
  GitPullRequestArrow,
  GitBranch,
  Loader2,
  MoreVertical,
  XCircle,
} from "lucide-react";
import { lazy, Suspense, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { openUrl } from "@tauri-apps/plugin-opener";
import { toast } from "sonner";

import { diffApi } from "@/api/diff";
import {
  chatApi,
  type AgentConversationWorkspace,
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
import { GitAuthRepairPanel } from "@/components/git/GitAuthRepairPanel";
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
import { formatPullRequestUrlLabel } from "./agentPublishFormatting";
import {
  getAgentWorkspaceTerminalPublicationLabel,
  getAgentWorkspaceTerminalPublicationStatus,
  getAgentWorkspaceEffectiveBaseLabel,
  hasPublishedWorkspacePr,
  isPipelineOwnedAgentWorkspace,
  isAgentWorkspacePublishCurrent,
} from "./agentWorkspacePublishState";
import { invalidateWorkspaceQueries } from "./agentWorkspaceQueries";

const LazyDiffViewer = lazy(() =>
  import("@/components/diff").then((module) => ({ default: module.DiffViewer })),
);
export function AgentPublishPanel({
  workspace,
  onPublishWorkspace,
  isPublishingWorkspace,
}: {
  workspace: AgentConversationWorkspace | null;
  onPublishWorkspace: ((conversationId: string) => Promise<void>) | undefined;
  isPublishingWorkspace: boolean;
}) {
  const queryClient = useQueryClient();
  const [reviewOpen, setReviewOpen] = useState(false);
  const [commitFiles, setCommitFiles] = useState<DiffViewerFileChange[]>([]);
  const [isLoadingCommitFiles, setIsLoadingCommitFiles] = useState(false);
  const { confirm, confirmationDialogProps, ConfirmationDialog } = useConfirmation();
  const conversationId = workspace?.conversationId ?? null;
  const canHydratePublishFacts = useDeferredAgentHydration(conversationId);
  const changesQuery = useQuery({
    queryKey: ["agents", "workspace-diff", conversationId],
    queryFn: () => diffApi.getAgentConversationWorkspaceFileChanges(conversationId!),
    enabled: canHydratePublishFacts && !!conversationId,
    staleTime: 2_000,
  });
  const publicationEventsQuery = useQuery({
    queryKey: ["agents", "conversation-workspace-publication-events", conversationId],
    queryFn: () =>
      chatApi.listAgentConversationWorkspacePublicationEvents(conversationId!),
    enabled: canHydratePublishFacts && !!conversationId,
    staleTime: 0,
    refetchInterval: isPublishingWorkspace ? 1_500 : false,
  });
  const commitsQuery = useQuery({
    queryKey: ["agents", "workspace-commits", conversationId],
    queryFn: async (): Promise<DiffViewerCommit[]> => {
      const commits = await diffApi.getAgentConversationWorkspaceCommits(
        conversationId!,
      );
      return commits
        .map((commit) => ({
          sha: commit.sha,
          shortSha: commit.shortSha,
          message: commit.message,
          author: commit.author,
          date: commit.date,
        }))
        .reverse();
    },
    enabled: canHydratePublishFacts && !!conversationId && reviewOpen,
    staleTime: 2_000,
  });
  const terminalPublicationStatus =
    getAgentWorkspaceTerminalPublicationStatus(workspace);
  const terminalPublicationLabel =
    getAgentWorkspaceTerminalPublicationLabel(workspace);
  const isPipelineOwnedWorkspace = isPipelineOwnedAgentWorkspace(workspace);
  const hasPublishedPr = hasPublishedWorkspacePr(workspace);
  const freshnessQuery = useQuery({
    queryKey: ["agents", "conversation-workspace-freshness", conversationId],
    queryFn: () => chatApi.getAgentConversationWorkspaceFreshness(conversationId!),
    enabled:
      canHydratePublishFacts &&
      !!conversationId &&
      (workspace?.mode === "edit" || hasPublishedPr) &&
      !terminalPublicationStatus,
    staleTime: 5_000,
  });
  const updateFromBaseMutation = useMutation({
    mutationFn: () => chatApi.updateAgentConversationWorkspaceFromBase(conversationId!),
    onSuccess: async (result) => {
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
  const changesError = changesQuery.error;
  const changes = changesQuery.data ?? [];
  const commits = commitsQuery.data ?? [];
  const publicationEvents = publicationEventsQuery.data ?? [];
  const isChangesLoading =
    Boolean(conversationId) && (!canHydratePublishFacts || changesQuery.isLoading);
  const isPublicationEventsLoading =
    Boolean(conversationId) &&
    (!canHydratePublishFacts || publicationEventsQuery.isLoading);
  const hasNoDetectedChanges = changesQuery.isSuccess && changes.length === 0;

  if (!workspace) {
    return <EmptyArtifactState title="No workspace selected" />;
  }

  const branch = workspace.branchName;
  const freshness = freshnessQuery.data;
  const base = getAgentWorkspaceEffectiveBaseLabel(workspace, freshness);
  const prLabel = workspace.publicationPrNumber
    ? `PR #${workspace.publicationPrNumber}`
    : workspace.publicationPrUrl
      ? "Published PR"
      : "No PR yet";
  const prUrlLabel = workspace.publicationPrUrl
    ? formatPullRequestUrlLabel(workspace.publicationPrUrl)
    : null;
  const baseStatus = freshness?.baseStatus ?? "valid";
  const baseBlocked = baseStatus === "blocked";
  const baseRetargeted = baseStatus === "retargeted";
  const isBranchUpdateNeeded =
    !baseBlocked && !terminalPublicationStatus && Boolean(freshness?.isBaseAhead);
  const isPublishCurrent = isAgentWorkspacePublishCurrent(workspace, freshness);
  const isUpdatingFromBase = updateFromBaseMutation.isPending;
  const effectivePublishing = isPublishingWorkspace || isUpdatingFromBase;
  const isRepairPending = workspace.publicationPushStatus === "needs_agent";
  const isDescriptionFailed = workspace.publicationPushStatus === "description_failed";
  const pipelineStatus = isUpdatingFromBase
    ? "refreshing"
    : isPublishingWorkspace &&
        ![
          "checking",
          "committing",
          "refreshing",
          "refreshed",
          "describing",
          "description_failed",
          "pushing",
          "pushed",
        ].includes(
          workspace.publicationPushStatus ?? "",
        )
      ? "checking"
      : workspace.publicationPushStatus;
  const baseActionLabel =
    freshness?.effectiveBaseDisplayName ??
    freshness?.effectiveBaseRef ??
    freshness?.baseRef ??
    workspace.baseRef ??
    base;
  const shouldShowPublishPipeline =
    effectivePublishing || workspace.publicationPushStatus === "description_failed";
  const isFreshnessLoading = freshnessQuery.isLoading;
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
            ? changes.length > 0
              ? `${changes.length} changed file${changes.length === 1 ? "" : "s"} published for review.`
              : "Workspace is published and current."
            : changes.length > 0
              ? `${changes.length} changed file${changes.length === 1 ? "" : "s"} ready for review.`
              : "No changed files detected yet.";
  const confirmUpdateFromBase = async () => {
    const confirmed = await confirm({
      title: "Update from base branch?",
      description: `This will update ${branch} with the latest changes from ${baseActionLabel}. If conflicts are found, RalphX will route this workspace through repair before publishing can continue.`,
      confirmText: "Update branch",
    });
    if (!confirmed) {
      return;
    }
    updateFromBaseMutation.mutate();
  };
  const confirmClosePr = async () => {
    const confirmed = await confirm({
      title: "Close pull request?",
      description: `This will close ${terminalPrLabel} for ${branch}. The workspace files and conversation history will remain available.`,
      confirmText: "Close PR",
      variant: "destructive",
    });
    if (!confirmed) {
      return;
    }
    closePrMutation.mutate();
  };
  const confirmPublishWorkspace = async () => {
    if (!onPublishWorkspace) {
      return;
    }
    const confirmed = await confirm({
      title: "Commit and publish workspace?",
      description: `This will commit workspace changes on ${branch} and push them to a pull request against ${base}.`,
      confirmText: "Commit & Publish",
    });
    if (!confirmed) {
      return;
    }
    await onPublishWorkspace(workspace.conversationId);
  };
  const primaryActionClassName = "h-9 gap-2 px-3 text-xs";

  return (
    <div className="min-h-full p-4" data-testid="agents-publish-pane">
      <div className="mx-auto flex max-w-3xl flex-col gap-4">
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

          <div className="mt-4 grid gap-3 sm:grid-cols-2">
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

        <section
          className="rounded-lg border p-4"
          style={{
            background: "var(--bg-surface)",
            borderColor: "var(--border-subtle)",
          }}
        >
          {isBranchUpdateNeeded && (
            <div
              className="mb-3 flex items-start gap-2 rounded-md border px-3 py-2 text-xs leading-relaxed"
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
              className="mb-3 flex items-start gap-2 rounded-md border px-3 py-2 text-xs leading-relaxed"
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
              className="mb-3 flex items-start gap-2 rounded-md border px-3 py-2 text-xs leading-relaxed"
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
          <div className="flex flex-wrap items-center justify-between gap-3">
            <div>
              <div className="text-sm font-semibold text-[var(--text-primary)]">
                {terminalPublicationLabel
                  ? `Pull Request ${terminalPublicationLabel}`
                  : "Commit & Publish"}
              </div>
              <div className="mt-1 text-xs leading-relaxed text-[var(--text-muted)]">
                {publishSummary}
              </div>
            </div>
            <div className="flex items-center gap-2">
              <Button
                type="button"
                variant="ghost"
                className="h-9 gap-2 px-3 text-xs"
                onClick={() => setReviewOpen(true)}
                disabled={baseBlocked || (isChangesLoading && !hasPublishedPr)}
                data-testid="agents-review-changes"
              >
                <Code className="h-3.5 w-3.5" />
                Review Changes
              </Button>
              {isBranchUpdateNeeded ? (
                <Button
                  type="button"
                  className={primaryActionClassName}
                  onClick={() => void confirmUpdateFromBase()}
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
              ) : (
                <Button
                  type="button"
                  className={primaryActionClassName}
                  onClick={() => void confirmPublishWorkspace()}
                  disabled={publishDisabled || isFreshnessLoading}
                  data-testid="agents-publish-confirm"
                >
                  {isPublishingWorkspace || isFreshnessLoading ? (
                    <Loader2 className="h-3.5 w-3.5 animate-spin" />
                  ) : isPublishCurrent || terminalPublicationStatus ? (
                    <CheckCircle2 className="h-3.5 w-3.5" />
                  ) : (
                    <GitPullRequestArrow className="h-3.5 w-3.5" />
                  )}
                  {baseBlocked
                    ? "Base unavailable"
                    : isFreshnessLoading
                      ? "Checking..."
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
                        void confirmClosePr();
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
        </section>
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
                    oldContent: diff.oldContent,
                    newContent: diff.newContent,
                    hunks: [],
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
                isLoadingChanges={changesQuery.isLoading}
                isLoadingHistory={commitsQuery.isLoading}
                isLoadingCommitFiles={isLoadingCommitFiles}
                changesLabel="Workspace Changes"
                changesEmptyTitle="No workspace changes"
                changesEmptySubtitle="There are no changed files to review for this agent branch."
              />
            </Suspense>
          )}
        </DialogContent>
      </Dialog>
      <ConfirmationDialog {...confirmationDialogProps} />
    </div>
  );
}
