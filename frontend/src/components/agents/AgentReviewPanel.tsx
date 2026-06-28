import { AlertCircle, CheckCircle2, Loader2, RefreshCw } from "lucide-react";
import { lazy, Suspense, useEffect, useMemo, useState, type ElementType } from "react";

import type {
  AgentWorkspaceReviewContext,
  StartAgentWorkspaceReviewResult,
} from "@/api/chat";
import { Button } from "@/components/ui/button";
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
  force: boolean;
};

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
  onStartReview: (force: boolean) => void;
}

function reviewTargetLabel(context: ReviewDisplayContext | null): string | null {
  const target = context?.target;
  if (!target) return null;
  if (target.sourcePullRequestNumber) {
    return `PR #${target.sourcePullRequestNumber} source changes`;
  }
  return target.scope === "workspace_delta" ? "Workspace changes" : "Selected source changes";
}

function reviewErrorMessage(
  context: ReviewDisplayContext | null,
  reviewStartError: Error | null,
): string | null {
  if (reviewStartError) {
    return reviewStartError.message || "Failed to start review.";
  }
  if (context?.monitor.status === "blocked") {
    return context.monitor.lastError ?? "Review could not complete.";
  }
  return null;
}

function reviewActionForState({
  context,
  hasArtifact,
  isBlocked,
  isRunning,
}: {
  context: ReviewDisplayContext | null;
  hasArtifact: boolean;
  isBlocked: boolean;
  isRunning: boolean;
}): ReviewAction | null {
  if (!context?.target || isRunning) return null;
  if (isBlocked) return { label: "Retry review", force: true };
  if (!hasArtifact) return { label: "Run review", force: false };
  if (context.isOutdated) return { label: "Update review", force: true };
  if (context.isCurrent) return { label: "Run again", force: true };
  return { label: "Run review", force: true };
}

function reviewStatusForState({
  context,
  hasArtifact,
  isBlocked,
  isRunning,
}: {
  context: ReviewDisplayContext | null;
  hasArtifact: boolean;
  isBlocked: boolean;
  isRunning: boolean;
}): ReviewStatus {
  if (isRunning) {
    return {
      label: "Review running",
      detail: "The reviewer is checking the current changes. The Review will appear here when it finishes.",
      color: "var(--accent-primary)",
      icon: Loader2,
      iconClassName: "animate-spin",
    };
  }
  if (isBlocked) {
    return {
      label: "Review blocked",
      detail: "The last review attempt did not complete.",
      color: "var(--status-error)",
      icon: AlertCircle,
    };
  }
  if (context?.isOutdated) {
    return {
      label: "Review is outdated",
      detail: "This Review was generated for earlier changes. Update it when you want a fresh reviewer pass.",
      color: "var(--status-warning)",
      icon: AlertCircle,
    };
  }
  if (context?.isCurrent) {
    return {
      label: "Review is current",
      detail: "This Review matches the current review target.",
      color: "var(--status-success)",
      icon: CheckCircle2,
    };
  }
  if (context?.target && !hasArtifact) {
    return {
      label: "Review not run",
      detail: "Reviewable changes are available. Run review when you want a reviewer pass.",
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
  onStartReview,
}: AgentReviewPanelProps) {
  const [isReviewExpanded, setIsReviewExpanded] = useState(true);

  useEffect(() => {
    setIsReviewExpanded(true);
  }, [reviewArtifact?.id, reviewArtifact?.metadata.version]);

  const displayContext = (
    isReviewActionPending
      ? reviewStartResult ?? reviewContext
      : reviewContext ?? reviewStartResult
  ) as ReviewDisplayContext | null;
  const isRunning =
    isReviewActionPending || displayContext?.monitor.status === "reviewing";
  const errorMessage = reviewErrorMessage(displayContext, reviewStartError);
  const isBlocked = Boolean(errorMessage) && !isRunning;
  const status = reviewStatusForState({
    context: displayContext,
    hasArtifact: Boolean(reviewArtifact),
    isBlocked,
    isRunning,
  });
  const action = reviewActionForState({
    context: displayContext,
    hasArtifact: Boolean(reviewArtifact),
    isBlocked,
    isRunning,
  });
  const targetLabel = reviewTargetLabel(displayContext);
  const skippedReason = reviewStartResult?.skippedReason ?? null;
  const versionLabel = displayContext?.monitor.reviewArtifactVersion
    ? `v${displayContext.monitor.reviewArtifactVersion}`
    : null;
  const StatusIcon = status.icon;
  const actionIconClassName = isReviewActionPending ? "animate-spin" : "";
  const ActionIcon = isReviewActionPending ? Loader2 : RefreshCw;
  const reviewUpdatedAt = displayContext?.monitor.reviewArtifactUpdatedAt
    ? new Date(displayContext.monitor.reviewArtifactUpdatedAt).toLocaleString()
    : null;
  const emptyArtifactTitle = isRunning
    ? "Review pending"
    : displayContext?.target
      ? "No Review yet"
      : status.label;
  const emptyArtifactDetail = isRunning
    ? "A Review will appear here when the reviewer finishes."
    : displayContext?.target
      ? isBlocked
        ? "Resolve the issue above or retry when the workspace is ready."
        : "Run review when you want a reviewer pass."
      : status.detail;

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
    if (!action) return null;
    return (
      <Button
        type="button"
        size="sm"
        onClick={() => onStartReview(action.force)}
        disabled={isReviewActionPending}
        className="h-8 gap-1.5 bg-[var(--accent-primary)] text-white hover:bg-[var(--accent-hover)]"
      >
        <ActionIcon className={`h-4 w-4 ${actionIconClassName}`} />
        {action.label}
      </Button>
    );
  }, [
    ActionIcon,
    action,
    actionIconClassName,
    isReviewActionPending,
    isRunning,
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
              <p className="mt-1 text-xs" style={{ color: "var(--text-muted)" }}>
                {errorMessage ?? status.detail}
              </p>
              {(targetLabel || reviewUpdatedAt) && (
                <p className="mt-2 text-[0.6875rem]" style={{ color: "var(--text-subtle)" }}>
                  {[targetLabel, reviewUpdatedAt].filter(Boolean).join(" · ")}
                </p>
              )}
            </div>
          </div>
          <div className="shrink-0">{actionButton}</div>
        </div>

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
          Outdated for current changes. The Review below is still available for reference.
        </div>
      )}

      {reviewArtifact ? (
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
      ) : (
        <div
          className="rounded-md px-6 py-8 text-center"
          style={{
            backgroundColor: "var(--bg-sunken)",
            borderColor: "var(--border-subtle)",
            borderWidth: 1,
            borderStyle: "solid",
          }}
        >
          <p className="text-sm font-medium" style={{ color: "var(--text-primary)" }}>
            {emptyArtifactTitle}
          </p>
          <p className="mx-auto mt-1 max-w-[28rem] text-xs" style={{ color: "var(--text-muted)" }}>
            {emptyArtifactDetail}
          </p>
        </div>
      )}
    </div>
  );
}
