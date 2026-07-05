import type { LucideIcon } from "lucide-react";
import { AlertCircle, CheckCircle2, CircleDot, Clock, XCircle } from "lucide-react";

import type { PullRequestCheck, PullRequestReviewSummary } from "@/api/github";

import { summarizeChecks } from "./pullRequestChecksSummary";
import { reviewDecisionBadge, type ReviewDecisionTone } from "./pullRequestReviewDecision";

type ChipTone = "success" | "error" | "warning" | "muted";

const TONE: Record<ChipTone, { color: string; bg: string; border: string }> = {
  success: {
    color: "var(--status-success)",
    bg: "var(--status-success-muted)",
    border: "var(--status-success-border)",
  },
  error: {
    color: "var(--status-error)",
    bg: "var(--status-error-muted)",
    border: "var(--status-error-border)",
  },
  warning: {
    color: "var(--status-warning)",
    bg: "var(--status-warning-muted)",
    border: "var(--status-warning-border)",
  },
  muted: {
    color: "var(--text-muted)",
    bg: "var(--bg-surface)",
    border: "var(--border-subtle)",
  },
};

const REVIEW_TONE: Record<ReviewDecisionTone, { tone: ChipTone; icon: LucideIcon }> = {
  approved: { tone: "success", icon: CheckCircle2 },
  changesRequested: { tone: "error", icon: AlertCircle },
  pending: { tone: "warning", icon: CircleDot },
};

function StatusChip({
  tone,
  icon: Icon,
  label,
}: {
  tone: ChipTone;
  icon: LucideIcon;
  label: string;
}) {
  const colors = TONE[tone];
  return (
    <span
      className="inline-flex items-center gap-1 rounded-full px-2 py-0.5 text-[0.6875rem] font-medium"
      style={{
        backgroundColor: colors.bg,
        color: colors.color,
        borderColor: colors.border,
        borderStyle: "solid",
        borderWidth: "1px",
      }}
    >
      <Icon className="h-3 w-3" aria-hidden="true" />
      {label}
    </span>
  );
}

function SkeletonChip() {
  return (
    <span
      className="inline-block h-5 w-20 animate-pulse rounded-full"
      style={{ backgroundColor: "var(--bg-surface)" }}
    />
  );
}

/**
 * At-a-glance PR health band: the GitHub review decision plus a CI passed /
 * failed / pending count. Renders nothing when there is neither a review
 * decision nor any check data, so it never adds an empty row.
 */
export function PullRequestStatusStrip({
  reviewSummary,
  checks,
  checksUnavailable = false,
  loading = false,
}: {
  reviewSummary: PullRequestReviewSummary | null | undefined;
  checks: PullRequestCheck[];
  checksUnavailable?: boolean | undefined;
  loading?: boolean | undefined;
}) {
  const badge = reviewDecisionBadge(reviewSummary);
  const summary = summarizeChecks(checks);
  const hasCi = summary.total > 0;
  const showCiUnavailable = !hasCi && checksUnavailable;

  if (loading) {
    return (
      <div
        className="flex flex-wrap items-center gap-2"
        data-testid="pr-status-strip-skeleton"
        role="status"
        aria-label="Loading pull request status"
      >
        <SkeletonChip />
        <SkeletonChip />
      </div>
    );
  }

  if (!badge && !hasCi && !showCiUnavailable) {
    return null;
  }

  const review = badge ? REVIEW_TONE[badge.tone] : null;

  return (
    <div
      className="flex flex-wrap items-center gap-2"
      data-testid="pr-status-strip"
      role="group"
      aria-label="Pull request status"
    >
      {badge && review ? (
        <StatusChip tone={review.tone} icon={review.icon} label={badge.label} />
      ) : null}
      {summary.passed > 0 ? (
        <StatusChip tone="success" icon={CheckCircle2} label={`${summary.passed} passed`} />
      ) : null}
      {summary.failed > 0 ? (
        <StatusChip tone="error" icon={XCircle} label={`${summary.failed} failed`} />
      ) : null}
      {summary.pending > 0 ? (
        <StatusChip tone="warning" icon={Clock} label={`${summary.pending} pending`} />
      ) : null}
      {showCiUnavailable ? (
        <StatusChip tone="muted" icon={AlertCircle} label="CI unavailable" />
      ) : null}
    </div>
  );
}
