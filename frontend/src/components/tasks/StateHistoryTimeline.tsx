/**
 * StateHistoryTimeline - Displays task state transition history with integrated issue tracking
 * Shows a vertical timeline with connected dots, status changes, actors, notes, and issues
 * Premium styling with ring effect on dots and connected lines
 */

import { useState, useMemo } from "react";
import { useQuery } from "@tanstack/react-query";
import { useTaskStateHistory } from "@/hooks/useReviews";
import { reviewIssuesApi } from "@/api/review-issues";
import {
  History,
  Loader2,
  ChevronDown,
  ChevronRight,
  AlertCircle,
} from "lucide-react";
import type { ReviewOutcome, ReviewerType } from "@/types/review";
import type { ReviewIssue, IssueProgressSummary } from "@/types/review-issue";
import { IssueProgressBar, SeverityBadge, StatusBadge } from "@/components/reviews";
import { ReviewFeedbackBody } from "@/components/reviews/ReviewFeedbackBody";
import { getReviewerActorLabel } from "@/lib/review-feedback";

interface StateHistoryTimelineProps {
  taskId: string;
}

const OUTCOME_CONFIG: Record<
  ReviewOutcome,
  { label: string; color: string }
> = {
  approved: { label: "Approved", color: "var(--status-success)" },
  approved_no_changes: { label: "No Changes", color: "var(--status-info)" },
  changes_requested: { label: "Changes Requested", color: "var(--status-warning)" },
  rejected: { label: "Rejected", color: "var(--status-error)" },
};

function mapReviewerToActor(reviewer: ReviewerType): string {
  return getReviewerActorLabel(reviewer);
}

function formatRelativeTime(dateString: string): string {
  const diff = Date.now() - new Date(dateString).getTime();
  const mins = Math.floor(diff / 60000);
  if (mins < 1) return "Just now";
  if (mins < 60) return `${mins} min ago`;
  const hours = Math.floor(mins / 60);
  if (hours < 24) return `${hours}h ago`;
  const days = Math.floor(hours / 24);
  return `${days}d ago`;
}

/**
 * Custom hook to fetch issues for a task
 */
function useTaskIssues(taskId: string) {
  const query = useQuery<ReviewIssue[], Error>({
    queryKey: ["reviewIssues", "byTask", taskId],
    queryFn: () => reviewIssuesApi.getByTaskId(taskId),
    enabled: !!taskId,
    staleTime: 30 * 1000,
  });

  return {
    issues: query.data ?? [],
    isLoading: query.isLoading,
    error: query.error?.message ?? null,
  };
}

/**
 * Custom hook to fetch issue progress summary for a task
 */
function useTaskIssueProgress(taskId: string) {
  const query = useQuery<IssueProgressSummary, Error>({
    queryKey: ["reviewIssues", "progress", taskId],
    queryFn: () => reviewIssuesApi.getProgress(taskId),
    enabled: !!taskId,
    staleTime: 30 * 1000,
  });

  return {
    progress: query.data ?? null,
    isLoading: query.isLoading,
    error: query.error?.message ?? null,
  };
}

/**
 * Compute issue diff between reviews
 * Returns new, resolved, and reopened issues for a review entry
 */
function computeIssueDiff(
  reviewId: string,
  issues: ReviewIssue[],
  previousReviewIds: string[]
): {
  newIssues: ReviewIssue[];
  resolvedIssues: ReviewIssue[];
  verifiedIssues: ReviewIssue[];
} {
  const newIssues: ReviewIssue[] = [];
  const resolvedIssues: ReviewIssue[] = [];
  const verifiedIssues: ReviewIssue[] = [];

  for (const issue of issues) {
    // Issues created by this review are new
    if (issue.reviewNoteId === reviewId) {
      newIssues.push(issue);
    }
    // Issues verified by this review
    if (issue.verifiedByReviewId === reviewId) {
      verifiedIssues.push(issue);
    }
  }

  // Issues that were open in previous reviews but are now resolved count as resolved
  // This is a simplification - we're showing issues that were addressed/verified
  // since the previous review
  for (const issue of issues) {
    if (
      previousReviewIds.includes(issue.reviewNoteId) &&
      (issue.status === "addressed" || issue.status === "verified")
    ) {
      resolvedIssues.push(issue);
    }
  }

  return { newIssues, resolvedIssues, verifiedIssues };
}

interface ReviewEntryIssuesProps {
  reviewId: string;
  issues: ReviewIssue[];
  previousReviewIds: string[];
  isExpanded: boolean;
  onToggle: () => void;
}

function ReviewEntryIssues({
  reviewId,
  issues,
  previousReviewIds,
  isExpanded,
  onToggle,
}: ReviewEntryIssuesProps) {
  const { newIssues, verifiedIssues } = computeIssueDiff(
    reviewId,
    issues,
    previousReviewIds
  );

  // Issues created by this review
  const reviewIssues = issues.filter((i) => i.reviewNoteId === reviewId);
  const hasIssues = reviewIssues.length > 0 || verifiedIssues.length > 0;

  if (!hasIssues) {
    return null;
  }

  const ChevronIcon = isExpanded ? ChevronDown : ChevronRight;

  return (
    <div className="mt-2">
      <button
        onClick={onToggle}
        className="flex items-center gap-1.5 text-[0.6875rem] w-full text-left"
        style={{ color: "var(--text-muted)" }}
      >
        <ChevronIcon className="w-3 h-3" />
        <AlertCircle className="w-3 h-3" />
        <span>
          {newIssues.length > 0 && (
            <span style={{ color: "var(--status-info)" }}>
              {newIssues.length} new
            </span>
          )}
          {newIssues.length > 0 && verifiedIssues.length > 0 && (
            <span> · </span>
          )}
          {verifiedIssues.length > 0 && (
            <span style={{ color: "var(--status-success)" }}>
              {verifiedIssues.length} verified
            </span>
          )}
        </span>
      </button>

      {isExpanded && (
        <div className="mt-2 pl-4 space-y-2">
          {/* New issues from this review */}
          {newIssues.length > 0 && (
            <div>
              <div
                className="text-[0.625rem] uppercase tracking-wider mb-1.5"
                style={{ color: "var(--text-muted)" }}
              >
                Issues Found
              </div>
              <div className="space-y-1.5">
                {newIssues.map((issue) => (
                  <CompactIssueCard key={issue.id} issue={issue} />
                ))}
              </div>
            </div>
          )}

          {/* Verified issues in this review */}
          {verifiedIssues.length > 0 && (
            <div className="mt-2">
              <div
                className="text-[0.625rem] uppercase tracking-wider mb-1.5"
                style={{ color: "var(--status-success)" }}
              >
                Verified
              </div>
              <div className="space-y-1.5">
                {verifiedIssues.map((issue) => (
                  <CompactIssueCard key={issue.id} issue={issue} />
                ))}
              </div>
            </div>
          )}
        </div>
      )}
    </div>
  );
}

interface CompactIssueCardProps {
  issue: ReviewIssue;
}

function CompactIssueCard({ issue }: CompactIssueCardProps) {
  return (
    <div
      className="px-2 py-1.5 rounded"
      style={{ backgroundColor: "var(--bg-surface)" }}
    >
      <div className="flex items-center gap-1.5">
        <SeverityBadge severity={issue.severity} compact />
        <StatusBadge status={issue.status} compact />
        <span
          className="text-[0.6875rem] truncate flex-1"
          style={{ color: "var(--text-secondary)" }}
        >
          {issue.title}
        </span>
      </div>
      {issue.filePath && (
        <div
          className="text-[0.625rem] font-mono mt-0.5"
          style={{ color: "var(--status-info)" }}
        >
          {issue.filePath}
          {issue.lineNumber && `:${issue.lineNumber}`}
        </div>
      )}
    </div>
  );
}

interface IssueSummaryHeaderProps {
  progress: IssueProgressSummary;
}

function IssueSummaryHeader({ progress }: IssueSummaryHeaderProps) {
  if (progress.total === 0) {
    return null;
  }

  return (
    <div
      className="p-3 rounded-lg mb-3"
      style={{ backgroundColor: "var(--bg-surface)" }}
    >
      <div className="flex items-center justify-between mb-2">
        <span
          className="text-[0.6875rem] uppercase tracking-wider font-medium"
          style={{ color: "var(--text-muted)" }}
        >
          Issue Progress
        </span>
        <span
          className="text-[0.75rem] font-medium tabular-nums"
          style={{ color: "var(--text-secondary)" }}
        >
          {progress.verified + progress.addressed + progress.wontfix}/{progress.total} resolved
        </span>
      </div>
      <IssueProgressBar progress={progress} showSeverityBreakdown />
    </div>
  );
}

export function StateHistoryTimeline({ taskId }: StateHistoryTimelineProps) {
  const { data, isLoading, isEmpty } = useTaskStateHistory(taskId);
  const { issues } = useTaskIssues(taskId);
  const { progress } = useTaskIssueProgress(taskId);
  const [expandedReviews, setExpandedReviews] = useState<Set<string>>(new Set());

  // Build list of review IDs in chronological order (oldest first)
  const reviewIds = useMemo(() => {
    return [...data]
      .sort((a, b) => new Date(a.created_at).getTime() - new Date(b.created_at).getTime())
      .map((entry) => entry.id);
  }, [data]);

  const toggleExpanded = (reviewId: string) => {
    setExpandedReviews((prev) => {
      const next = new Set(prev);
      if (next.has(reviewId)) {
        next.delete(reviewId);
      } else {
        next.add(reviewId);
      }
      return next;
    });
  };

  if (isLoading) {
    return (
      <div data-testid="timeline-loading" className="flex justify-center py-8">
        <Loader2
          className="w-6 h-6 animate-spin"
          style={{ color: "var(--text-muted)" }}
        />
      </div>
    );
  }

  if (isEmpty) {
    return (
      <div
        data-testid="timeline-empty"
        className="flex flex-col items-center justify-center py-8 text-center"
      >
        <History
          className="w-8 h-8 mb-2"
          style={{ color: "var(--text-muted)", opacity: 0.5 }}
        />
        <p className="text-sm" style={{ color: "var(--text-muted)" }}>
          No history yet
        </p>
        <p className="text-xs mt-1" style={{ color: "var(--text-muted)" }}>
          Status changes will appear here
        </p>
      </div>
    );
  }

  return (
    <div
      data-testid="timeline-container"
      className="p-4 rounded-lg"
      style={{ backgroundColor: "var(--bg-surface)" }}
    >
      {/* Issue Progress Summary */}
      {progress && progress.total > 0 && (
        <IssueSummaryHeader progress={progress} />
      )}

      {/* Timeline entries */}
      <div className="relative">
        {data.map((entry, index) => {
          const config = OUTCOME_CONFIG[entry.outcome];
          const relativeTime = formatRelativeTime(entry.created_at);
          const isLatest = index === 0;
          const isLast = index === data.length - 1;

          // Get previous review IDs (reviews before this one in chronological order)
          const entryIndex = reviewIds.indexOf(entry.id);
          const previousReviewIds = reviewIds.slice(0, entryIndex);

          return (
            <div
              key={entry.id}
              data-testid={`timeline-entry-${entry.id}`}
              data-timestamp={entry.created_at}
              className="relative pl-6"
              style={{ paddingBottom: isLast ? 0 : "16px" }}
            >
              {/* Vertical connector line */}
              {!isLast && (
                <div
                  className="absolute w-0.5"
                  style={{
                    left: "5px",
                    top: isLatest ? "20px" : "12px",
                    bottom: 0,
                    backgroundColor: "var(--border-subtle)",
                  }}
                />
              )}

              {/* Status dot with ring effect */}
              <div
                data-testid={`timeline-dot-${entry.id}`}
                className="absolute rounded-full"
                style={{
                  left: isLatest ? "-2px" : 0,
                  top: isLatest ? "2px" : "4px",
                  width: isLatest ? "16px" : "12px",
                  height: isLatest ? "16px" : "12px",
                  backgroundColor: config.color,
                  border: isLatest ? "none" : "2px solid var(--bg-elevated)",
                  boxShadow: isLatest
                    ? `0 0 0 4px ${config.color}33`
                    : undefined,
                }}
              />

              {/* Content */}
              <div className="flex items-center justify-between gap-2">
                <span
                  className="font-medium text-sm"
                  style={{ color: "var(--text-primary)" }}
                >
                  {config.label}
                </span>
                <span
                  className="text-xs"
                  style={{ color: "var(--text-muted)" }}
                >
                  {relativeTime}
                </span>
              </div>
              <div
                className="text-xs mt-0.5"
                style={{ color: "var(--text-secondary)" }}
              >
                by: {mapReviewerToActor(entry.reviewer)}
              </div>
              {(entry.summary || entry.notes) && (
                <div
                  className="text-xs mt-1"
                  style={{ color: "var(--text-secondary)" }}
                >
                  <ReviewFeedbackBody
                    summary={entry.summary ?? null}
                    notes={entry.notes ?? null}
                    dialogTitle="Full review feedback"
                    dialogDescription="Full review feedback in a scrollable view."
                    fullButtonLabel="View full feedback"
                    previewClassName="text-xs text-text-secondary"
                  />
                </div>
              )}

              {/* Issues section for this review entry */}
              <ReviewEntryIssues
                reviewId={entry.id}
                issues={issues}
                previousReviewIds={previousReviewIds}
                isExpanded={expandedReviews.has(entry.id)}
                onToggle={() => toggleExpanded(entry.id)}
              />
            </div>
          );
        })}
      </div>
    </div>
  );
}
