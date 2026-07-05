import type { PullRequestReviewSummary } from "@/api/github";

export type ReviewDecisionTone = "approved" | "changesRequested" | "pending";

export interface ReviewDecisionBadge {
  label: string;
  tone: ReviewDecisionTone;
}

/**
 * Map a GitHub review decision to a display badge. Returns `null` when there is
 * no decision to show (no review summary, or an empty/unknown decision), so the
 * caller can omit the chip entirely rather than render a meaningless one.
 *
 * GitHub emits `APPROVED`, `CHANGES_REQUESTED`, or `REVIEW_REQUIRED`.
 */
export function reviewDecisionBadge(
  reviewSummary: PullRequestReviewSummary | null | undefined,
): ReviewDecisionBadge | null {
  const decision = (reviewSummary?.reviewDecision ?? "").trim().toUpperCase();
  switch (decision) {
    case "APPROVED":
      return { label: "Approved", tone: "approved" };
    case "CHANGES_REQUESTED":
      return { label: "Changes requested", tone: "changesRequested" };
    case "REVIEW_REQUIRED":
      return { label: "Review required", tone: "pending" };
    default:
      return null;
  }
}
