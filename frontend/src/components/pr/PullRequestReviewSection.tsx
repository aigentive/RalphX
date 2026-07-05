import type { LucideIcon } from "lucide-react";
import { AlertCircle, CheckCircle2, CircleDot } from "lucide-react";

import type {
  PullRequestReviewSummary,
  PullRequestReviewThreadComment,
} from "@/api/github";

import {
  DetailSkeleton,
  PrCommentCard,
  PrMarkdown,
  PrSection,
} from "./PullRequestDetailPrimitives";
import { reviewDecisionBadge, type ReviewDecisionTone } from "./pullRequestReviewDecision";
import { formatPrDate } from "./PullRequestDetailUtils";

const DECISION_STYLE: Record<ReviewDecisionTone, { color: string; icon: LucideIcon }> = {
  approved: { color: "var(--status-success)", icon: CheckCircle2 },
  changesRequested: { color: "var(--status-error)", icon: AlertCircle },
  pending: { color: "var(--status-warning)", icon: CircleDot },
};

function threadLocation(comment: PullRequestReviewThreadComment): string {
  return [
    comment.path,
    comment.line != null ? `L${comment.line}` : null,
    comment.isOutdated ? "outdated" : null,
  ]
    .filter(Boolean)
    .join(" · ");
}

/**
 * The "Review" section: GitHub review decision + the latest changes-requested
 * feedback (body and inline comments) followed by the live inline review
 * thread. Consolidates what used to be a bare "Review Thread" list so the
 * actual review verdict is visible, not just scattered inline notes.
 */
export function PullRequestReviewSection({
  reviewSummary,
  reviewThread,
  loading,
}: {
  reviewSummary: PullRequestReviewSummary | null | undefined;
  reviewThread: PullRequestReviewThreadComment[];
  loading: boolean;
}) {
  const badge = reviewDecisionBadge(reviewSummary);
  const changesBody = reviewSummary?.latestChangesRequestedBody?.trim() || null;
  const changesAuthor = reviewSummary?.latestChangesRequestedAuthor ?? null;
  const changesAt = reviewSummary?.latestChangesRequestedSubmittedAt ?? null;
  const feedback = reviewSummary?.latestChangesRequestedComments ?? [];

  const hasSummary = Boolean(badge || changesBody || feedback.length > 0);
  const hasThread = reviewThread.length > 0;
  const count = feedback.length + reviewThread.length;

  const decision = badge ? DECISION_STYLE[badge.tone] : null;
  const DecisionIcon = decision?.icon;

  return (
    <PrSection title="Review" count={count}>
      {hasSummary || hasThread ? (
        <div className="space-y-3">
          {badge && decision && DecisionIcon ? (
            <div
              className="flex items-center gap-1.5 text-sm font-medium"
              style={{ color: decision.color }}
            >
              <DecisionIcon className="h-4 w-4 shrink-0" aria-hidden="true" />
              {badge.label}
            </div>
          ) : null}

          {changesBody || feedback.length > 0 ? (
            <div
              className="space-y-2 rounded-md p-3"
              style={{
                backgroundColor: "var(--bg-surface)",
                borderColor: "var(--border-subtle)",
                borderStyle: "solid",
                borderWidth: "1px",
              }}
            >
              {changesAuthor || changesAt ? (
                <p className="text-xs text-[var(--text-muted)]">
                  {changesAuthor ?? "Reviewer"} requested changes
                  {changesAt ? ` · ${formatPrDate(changesAt)}` : ""}
                </p>
              ) : null}
              {changesBody ? <PrMarkdown content={changesBody} /> : null}
              {feedback.map((comment) => (
                <PrCommentCard
                  key={comment.id}
                  author={comment.author}
                  createdAt={null}
                  body={comment.body}
                  meta={
                    [comment.path, comment.line != null ? `L${comment.line}` : null]
                      .filter(Boolean)
                      .join(" · ") || undefined
                  }
                />
              ))}
            </div>
          ) : null}

          {hasThread ? (
            <div className="space-y-2">
              {reviewThread.map((comment) => (
                <PrCommentCard
                  key={comment.id}
                  author={comment.author}
                  createdAt={comment.createdAt}
                  body={comment.body}
                  meta={threadLocation(comment) || undefined}
                />
              ))}
            </div>
          ) : null}
        </div>
      ) : loading ? (
        <DetailSkeleton lines={2} />
      ) : (
        <p className="text-sm text-[var(--text-secondary)]">No review yet.</p>
      )}
    </PrSection>
  );
}
