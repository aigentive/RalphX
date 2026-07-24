import type { LucideIcon } from "lucide-react";
import { AlertCircle, CheckCircle2, CircleDot, Clock, XCircle } from "lucide-react";

import type { PullRequestCheck, PullRequestReviewSummary } from "@/api/github";
import {
  StatusPill,
  type StatusPillTone,
} from "@/components/ui/status-pill";
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip";

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

const COMPACT_TONE: Record<ChipTone, StatusPillTone> = {
  success: "success",
  error: "error",
  warning: "warning",
  muted: "neutral",
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

function CompactStatusChip({
  tone,
  icon: Icon,
  label,
  visibleLabel,
}: {
  tone: ChipTone;
  icon: LucideIcon;
  label: string;
  visibleLabel?: string | undefined;
}) {
  return (
    <StatusPill
      tone={COMPACT_TONE[tone]}
      ariaLabel={label}
      icon={<Icon className="h-3 w-3" aria-hidden="true" />}
      label={visibleLabel ?? <span aria-hidden="true" />}
      className={
        visibleLabel
          ? "gap-1 px-1.5 font-medium"
          : "gap-0 px-1.5 font-medium"
      }
    />
  );
}

function SkeletonChip() {
  return (
    <span
      data-testid="pr-status-skeleton-chip"
      className="inline-block h-5 w-20 animate-pulse rounded-full"
      style={{ backgroundColor: "var(--bg-hover)" }}
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
  variant = "default",
}: {
  reviewSummary: PullRequestReviewSummary | null | undefined;
  checks: PullRequestCheck[];
  checksUnavailable?: boolean | undefined;
  loading?: boolean | undefined;
  variant?: "default" | "compact" | undefined;
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

  if (variant === "compact") {
    const labels = [
      ...(badge && review ? [badge.label] : []),
      ...(summary.passed > 0 ? [`${summary.passed} passed`] : []),
      ...(summary.failed > 0 ? [`${summary.failed} failed`] : []),
      ...(summary.pending > 0 ? [`${summary.pending} pending`] : []),
      ...(showCiUnavailable ? ["CI unavailable"] : []),
    ];

    return (
      <Tooltip>
        <TooltipTrigger asChild>
          <div
            className="flex flex-wrap items-center gap-1.5"
            data-testid="pr-status-strip"
            role="group"
            aria-label="Pull request status"
            tabIndex={0}
          >
            {badge && review ? (
              <CompactStatusChip
                tone={review.tone}
                icon={review.icon}
                label={badge.label}
              />
            ) : null}
            {summary.passed > 0 ? (
              <CompactStatusChip
                tone="success"
                icon={CheckCircle2}
                label={`${summary.passed} passed`}
                visibleLabel={`${summary.passed}`}
              />
            ) : null}
            {summary.failed > 0 ? (
              <CompactStatusChip
                tone="error"
                icon={XCircle}
                label={`${summary.failed} failed`}
                visibleLabel={`${summary.failed}`}
              />
            ) : null}
            {summary.pending > 0 ? (
              <CompactStatusChip
                tone="warning"
                icon={Clock}
                label={`${summary.pending} pending`}
                visibleLabel={`${summary.pending}`}
              />
            ) : null}
            {showCiUnavailable ? (
              <CompactStatusChip
                tone="muted"
                icon={AlertCircle}
                label="CI unavailable"
              />
            ) : null}
          </div>
        </TooltipTrigger>
        <TooltipContent
          side="bottom"
          className="flex flex-col items-start gap-1 leading-tight"
        >
          {labels.map((label) => (
            <span key={label}>{label}</span>
          ))}
        </TooltipContent>
      </Tooltip>
    );
  }

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
