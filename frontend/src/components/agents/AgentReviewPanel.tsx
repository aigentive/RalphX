import {
  AlertCircle,
  CheckCircle2,
  GitPullRequestArrow,
  Loader2,
  MoreVertical,
  RefreshCw,
  Wrench,
} from "lucide-react";
import {
  lazy,
  Suspense,
  useEffect,
  useMemo,
  useState,
  type ElementType,
} from "react";

import type {
  AgentWorkspaceReviewContext,
  StartAgentWorkspaceReviewResult,
} from "@/api/chat";
import { Button } from "@/components/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import { withAlpha } from "@/lib/theme-colors";
import type { Artifact } from "@/types/artifact";

import { EmptyArtifactState } from "./AgentsArtifactEmptyState";

const LazyPlanDisplay = lazy(() =>
  import("@/components/Ideation/PlanDisplay").then((module) => ({
    default: module.PlanDisplay,
  })),
);

type ReviewDisplayContext = Pick<
  AgentWorkspaceReviewContext,
  "target" | "monitor" | "isCurrent" | "isOutdated" | "shouldShowTab"
>;

type ReviewAction = {
  label: string;
} & (
  | {
      kind: "review";
      force: boolean;
    }
  | {
      kind: "fix";
    }
);

type ReviewStatus = {
  label: string;
  detail: string;
  color: string;
  icon: ElementType;
  iconClassName?: string;
};

interface AgentReviewPanelProps {
  reviewArtifact: Artifact | null;
  reviewContext: AgentWorkspaceReviewContext | null;
  reviewStartResult: StartAgentWorkspaceReviewResult | null;
  reviewStartError: Error | null;
  isReviewLoading: boolean;
  isReviewActionPending: boolean;
  isFixIssuesActionPending?: boolean;
  isWorkspaceRuntimeGenerating?: boolean;
  isPublishingWorkspace?: boolean;
  onOpenPublish?: () => void;
  onStartReview: (force: boolean) => void;
  onFixIssues: () => void;
}

function reviewTargetLabel(
  context: ReviewDisplayContext | null,
): string | null {
  const target = context?.target;
  if (!target) return null;
  if (target.sourcePullRequestNumber) {
    return `PR #${target.sourcePullRequestNumber} source changes`;
  }
  return target.scope === "workspace_delta"
    ? "Workspace changes"
    : "Selected source changes";
}

function reviewErrorMessage(
  context: ReviewDisplayContext | null,
  reviewStartError: Error | null,
): string | null {
  if (reviewStartError) {
    return reviewStartError.message || "Failed to start review.";
  }
  if (
    context?.monitor.status === "blocked" ||
    context?.monitor.reviewGateStatus === "failed" ||
    context?.monitor.reviewOutcome === "run_failed"
  ) {
    return context.monitor.lastError ?? "Review could not complete.";
  }
  return null;
}

function hasPassedWorkspaceReview(
  context: ReviewDisplayContext | null,
): boolean {
  const gateStatus = context?.monitor.reviewGateStatus ?? null;
  if (gateStatus) {
    return gateStatus === "passed";
  }
  return Boolean(
    context?.isCurrent && context.monitor.reviewOutcome === "passed",
  );
}

function isWorkspaceReviewFixerActive(
  status: string | null | undefined,
): boolean {
  return status === "routing" || status === "queued" || status === "running";
}

function canFixBlockingReview(
  context: ReviewDisplayContext | null,
  isRunning: boolean,
): boolean {
  if (
    !context?.target ||
    isRunning ||
    !context.isCurrent ||
    context.isOutdated
  ) {
    return false;
  }
  return (
    context.monitor.reviewGateStatus === "blocking" ||
    context.monitor.reviewOutcome === "blocking"
  );
}

function reviewActionForState({
  context,
  hasArtifact,
  isRunFailed,
  isRunning,
  isFixerActive,
}: {
  context: ReviewDisplayContext | null;
  hasArtifact: boolean;
  isRunFailed: boolean;
  isRunning: boolean;
  isFixerActive: boolean;
}): ReviewAction | null {
  if (!context?.target || isRunning) return null;
  if (canFixBlockingReview(context, isRunning)) {
    if (isFixerActive) return null;
    return { label: "Fix Issues", kind: "fix" };
  }
  if (isRunFailed)
    return { label: "Retry review", kind: "review", force: true };
  if (!hasArtifact)
    return { label: "Run review", kind: "review", force: false };
  if (context.isOutdated)
    return { label: "Update review", kind: "review", force: true };
  if (context.isCurrent)
    return { label: "Run again", kind: "review", force: true };
  return { label: "Run review", kind: "review", force: true };
}

function reviewActionDisabledReason({
  isReviewActionPending,
  isFixIssuesActionPending,
  isWorkspaceRuntimeGenerating,
  isPublishingWorkspace,
}: {
  isReviewActionPending: boolean;
  isFixIssuesActionPending: boolean;
  isWorkspaceRuntimeGenerating: boolean;
  isPublishingWorkspace: boolean;
}): string | null {
  if (isReviewActionPending) {
    return "Review is starting. Wait for this request to finish.";
  }
  if (isFixIssuesActionPending) {
    return "Fixer is starting. Wait for this request to finish.";
  }
  if (isWorkspaceRuntimeGenerating) {
    return "Review is available after the current agent run finishes.";
  }
  if (isPublishingWorkspace) {
    return "Review actions are unavailable while Commit & Publish is running.";
  }
  return null;
}

function reviewStatusForState({
  context,
  hasArtifact,
  isRunFailed,
  isRunning,
}: {
  context: ReviewDisplayContext | null;
  hasArtifact: boolean;
  isRunFailed: boolean;
  isRunning: boolean;
}): ReviewStatus {
  const gateStatus = context?.monitor.reviewGateStatus ?? null;
  if (isRunning) {
    return {
      label: "Reviewing",
      detail:
        "The reviewer is checking the current changes. The Review will appear here when it finishes.",
      color: "var(--accent-primary)",
      icon: Loader2,
      iconClassName: "animate-spin",
    };
  }
  if (isRunFailed) {
    return {
      label: "Review failed",
      detail: "The last review attempt did not complete.",
      color: "var(--status-error)",
      icon: AlertCircle,
    };
  }
  if (gateStatus === "blocking") {
    return {
      label: "Review blocking",
      detail:
        context?.monitor.reviewBlockingSummary ??
        "The reviewer found blocking issues in the current changes.",
      color: "var(--status-error)",
      icon: AlertCircle,
    };
  }
  if (context?.isOutdated) {
    return {
      label: "Review is outdated",
      detail:
        "This Review was generated for earlier changes. Update it when you want a fresh reviewer pass.",
      color: "var(--status-warning)",
      icon: AlertCircle,
    };
  }
  if (hasPassedWorkspaceReview(context)) {
    return {
      label: "Review passed",
      detail: "This Review passed for the current review target.",
      color: "var(--status-success)",
      icon: CheckCircle2,
    };
  }
  if (context?.target && !hasArtifact) {
    return {
      label: "Review not run",
      detail:
        "Reviewable changes are available. Run review when you want a reviewer pass.",
      color: "var(--text-muted)",
      icon: AlertCircle,
    };
  }
  if (context?.target) {
    return {
      label: "Review pending",
      detail: "Reviewable changes are available.",
      color: "var(--text-muted)",
      icon: AlertCircle,
    };
  }
  return {
    label: hasArtifact ? "Review available" : "No reviewable changes",
    detail: hasArtifact
      ? "The latest Review is available below."
      : "No reviewable changes were found for this workspace.",
    color: "var(--text-muted)",
    icon: AlertCircle,
  };
}

export function AgentReviewPanel({
  reviewArtifact,
  reviewContext,
  reviewStartResult,
  reviewStartError,
  isReviewLoading,
  isReviewActionPending,
  isFixIssuesActionPending = false,
  isWorkspaceRuntimeGenerating = false,
  isPublishingWorkspace = false,
  onOpenPublish,
  onStartReview,
  onFixIssues,
}: AgentReviewPanelProps) {
  const [isReviewExpanded, setIsReviewExpanded] = useState(true);

  useEffect(() => {
    setIsReviewExpanded(true);
  }, [reviewArtifact?.id, reviewArtifact?.metadata.version]);

  const displayContext = (
    isReviewActionPending
      ? (reviewStartResult ?? reviewContext)
      : (reviewContext ?? reviewStartResult)
  ) as ReviewDisplayContext | null;
  const isRunning =
    isReviewActionPending || displayContext?.monitor.status === "reviewing";
  const isFixerActive =
    isFixIssuesActionPending ||
    isWorkspaceReviewFixerActive(displayContext?.monitor.reviewFixerStatus);
  const errorMessage = reviewErrorMessage(displayContext, reviewStartError);
  const isRunFailed = Boolean(errorMessage) && !isRunning;
  const status = reviewStatusForState({
    context: displayContext,
    hasArtifact: Boolean(reviewArtifact),
    isRunFailed,
    isRunning,
  });
  const action = reviewActionForState({
    context: displayContext,
    hasArtifact: Boolean(reviewArtifact),
    isRunFailed,
    isRunning,
    isFixerActive,
  });
  const targetLabel = reviewTargetLabel(displayContext);
  const skippedReason = reviewStartResult?.skippedReason ?? null;
  const versionLabel = displayContext?.monitor.reviewArtifactVersion
    ? `v${displayContext.monitor.reviewArtifactVersion}`
    : null;
  const StatusIcon = status.icon;
  const isAnyActionPending = isReviewActionPending || isFixIssuesActionPending;
  const actionIconClassName = isAnyActionPending ? "animate-spin" : "";
  const ActionIcon = isAnyActionPending
    ? Loader2
    : action?.kind === "fix"
      ? Wrench
      : RefreshCw;
  const reviewUpdatedAt = displayContext?.monitor.reviewArtifactUpdatedAt
    ? new Date(displayContext.monitor.reviewArtifactUpdatedAt).toLocaleString()
    : null;
  const actionDisabledReason = action
    ? reviewActionDisabledReason({
        isReviewActionPending,
        isFixIssuesActionPending,
        isWorkspaceRuntimeGenerating,
        isPublishingWorkspace,
      })
    : null;
  const actionDisabledReasonId = actionDisabledReason
    ? "agents-review-action-disabled-reason"
    : undefined;
  const actionButton = useMemo(() => {
    if (isRunning && !action) {
      return (
        <Button
          type="button"
          variant="ghost"
          size="sm"
          disabled
          className="h-8 gap-1.5"
        >
          <Loader2 className="h-4 w-4 animate-spin" />
          Running
        </Button>
      );
    }
    if (isFixerActive && !action) {
      return (
        <Button
          type="button"
          variant="ghost"
          size="sm"
          disabled
          className="h-8 gap-1.5"
          data-testid="agents-review-fixing"
        >
          <Loader2 className="h-4 w-4 animate-spin" />
          Fixing...
        </Button>
      );
    }
    if (!action) return null;
    const isActionDisabled = actionDisabledReason !== null;
    const shouldPromotePublish =
      action.label === "Run again" &&
      Boolean(onOpenPublish) &&
      Boolean(displayContext?.isCurrent) &&
      !displayContext?.isOutdated &&
      hasPassedWorkspaceReview(displayContext);
    if (shouldPromotePublish) {
      return (
        <div className="flex items-center gap-1.5">
          <Button
            type="button"
            size="sm"
            onClick={() => onOpenPublish?.()}
            disabled={isPublishingWorkspace}
            className="h-8 gap-1.5 bg-[var(--accent-primary)] text-white hover:bg-[var(--accent-hover)]"
            data-testid="agents-review-open-publish"
          >
            {isPublishingWorkspace ? (
              <Loader2 className="h-4 w-4 animate-spin" />
            ) : (
              <GitPullRequestArrow className="h-4 w-4" />
            )}
            Commit &amp; Publish
          </Button>
          <DropdownMenu>
            <Tooltip>
              <TooltipTrigger asChild>
                <span className="inline-flex">
                  <DropdownMenuTrigger asChild>
                    <Button
                      type="button"
                      variant="ghost"
                      size="sm"
                      className="h-8 w-7 border-0 bg-transparent p-0 hover:bg-[var(--bg-hover)]"
                      disabled={isActionDisabled}
                      {...(actionDisabledReasonId !== undefined && {
                        "aria-describedby": actionDisabledReasonId,
                      })}
                      aria-label="Review actions"
                      data-testid="agents-review-actions-menu"
                    >
                      {isReviewActionPending ? (
                        <Loader2 className="h-3.5 w-3.5 animate-spin" />
                      ) : (
                        <MoreVertical className="h-3.5 w-3.5" />
                      )}
                    </Button>
                  </DropdownMenuTrigger>
                </span>
              </TooltipTrigger>
              <TooltipContent side="top">
                {actionDisabledReason ?? "Review actions"}
              </TooltipContent>
            </Tooltip>
            <DropdownMenuContent align="end" className="min-w-[160px]">
              <DropdownMenuItem
                data-testid="agents-review-rerun"
                onSelect={(event) => {
                  event.preventDefault();
                  if (action.kind === "review") {
                    onStartReview(action.force);
                  }
                }}
                disabled={isActionDisabled}
                {...(actionDisabledReasonId !== undefined && {
                  "aria-describedby": actionDisabledReasonId,
                })}
              >
                <RefreshCw className="h-3.5 w-3.5" />
                Run again
              </DropdownMenuItem>
            </DropdownMenuContent>
          </DropdownMenu>
        </div>
      );
    }
    const button = (
      <Button
        type="button"
        size="sm"
        onClick={() =>
          action.kind === "fix" ? onFixIssues() : onStartReview(action.force)
        }
        disabled={isActionDisabled}
        {...(actionDisabledReasonId !== undefined && {
          "aria-describedby": actionDisabledReasonId,
        })}
        className="h-8 gap-1.5 bg-[var(--accent-primary)] text-white hover:bg-[var(--accent-hover)]"
      >
        <ActionIcon className={`h-4 w-4 ${actionIconClassName}`} />
        {action.label}
      </Button>
    );
    if (!actionDisabledReason) {
      return button;
    }
    return (
      <Tooltip>
        <TooltipTrigger asChild>
          <span className="inline-flex">{button}</span>
        </TooltipTrigger>
        <TooltipContent side="top">{actionDisabledReason}</TooltipContent>
      </Tooltip>
    );
  }, [
    ActionIcon,
    action,
    actionDisabledReason,
    actionDisabledReasonId,
    actionIconClassName,
    displayContext,
    isFixerActive,
    isReviewActionPending,
    isPublishingWorkspace,
    isRunning,
    onOpenPublish,
    onFixIssues,
    onStartReview,
  ]);

  if (isReviewLoading) {
    return <EmptyArtifactState title="Loading review..." />;
  }

  if (!displayContext && reviewArtifact) {
    return (
      <div className="min-h-full px-4 pb-4 pt-4">
        <Suspense fallback={<EmptyArtifactState title="Loading review..." />}>
          <LazyPlanDisplay
            plan={reviewArtifact}
            artifactLabel="Review"
            linkedProposalsCount={0}
            isExpanded={isReviewExpanded}
            onExpandedChange={setIsReviewExpanded}
            chromeless
          />
        </Suspense>
      </div>
    );
  }

  return (
    <div className="min-h-full px-4 pb-4 pt-4">
      <div
        className="mb-4 rounded-md p-4"
        style={{
          backgroundColor: "var(--bg-elevated)",
          borderColor: "var(--border-subtle)",
          borderWidth: 1,
          borderStyle: "solid",
        }}
      >
        <div className="flex items-start justify-between gap-3">
          <div className="flex min-w-0 gap-3">
            <div
              className="mt-0.5 flex h-8 w-8 shrink-0 items-center justify-center rounded-md"
              style={{
                backgroundColor: withAlpha(status.color, 12),
                borderColor: withAlpha(status.color, 24),
                borderWidth: 1,
                borderStyle: "solid",
                color: status.color,
              }}
            >
              <StatusIcon className={`h-4 w-4 ${status.iconClassName ?? ""}`} />
            </div>
            <div className="min-w-0">
              <div className="flex flex-wrap items-center gap-2">
                <p
                  className="text-sm font-semibold"
                  style={{ color: "var(--text-primary)" }}
                >
                  {status.label}
                </p>
                {versionLabel && (
                  <span
                    className="rounded-sm px-1.5 py-0.5 text-[0.6875rem] font-medium"
                    style={{
                      backgroundColor: "var(--bg-sunken)",
                      color: "var(--text-muted)",
                    }}
                  >
                    {versionLabel}
                  </span>
                )}
              </div>
              <p
                className="mt-1 text-xs"
                style={{ color: "var(--text-muted)" }}
              >
                {errorMessage ?? status.detail}
              </p>
              {(targetLabel || reviewUpdatedAt) && (
                <p
                  className="mt-2 text-[0.6875rem]"
                  style={{ color: "var(--text-subtle)" }}
                >
                  {[targetLabel, reviewUpdatedAt].filter(Boolean).join(" · ")}
                </p>
              )}
            </div>
          </div>
          <div className="shrink-0">{actionButton}</div>
        </div>

        {actionDisabledReason && (
          <div
            id={actionDisabledReasonId}
            className="mt-3 rounded-md px-3 py-2 text-xs"
            data-testid="agents-review-action-disabled-reason"
            role="status"
            style={{
              backgroundColor: "var(--bg-sunken)",
              borderColor: "var(--border-subtle)",
              borderWidth: 1,
              borderStyle: "solid",
              color: "var(--text-secondary)",
            }}
          >
            {actionDisabledReason}
          </div>
        )}

        {skippedReason === "conversation_active" && (
          <div
            className="mt-3 rounded-md px-3 py-2 text-xs"
            role="status"
            style={{
              backgroundColor: "var(--bg-sunken)",
              borderColor: "var(--border-subtle)",
              borderWidth: 1,
              borderStyle: "solid",
              color: "var(--text-secondary)",
            }}
          >
            Review will be available after the current agent run.
          </div>
        )}
      </div>

      {reviewArtifact && displayContext?.isOutdated && (
        <div
          className="mb-4 rounded-md px-3 py-2 text-xs"
          style={{
            backgroundColor: withAlpha("var(--status-warning)", 8),
            borderColor: withAlpha("var(--status-warning)", 24),
            borderWidth: 1,
            borderStyle: "solid",
            color: "var(--text-secondary)",
          }}
        >
          Outdated for current changes. The Review below is still available for
          reference.
        </div>
      )}

      {reviewArtifact && (
        <Suspense fallback={<EmptyArtifactState title="Loading review..." />}>
          <LazyPlanDisplay
            plan={reviewArtifact}
            artifactLabel="Review"
            linkedProposalsCount={0}
            isExpanded={isReviewExpanded}
            onExpandedChange={setIsReviewExpanded}
            chromeless
          />
        </Suspense>
      )}
    </div>
  );
}
