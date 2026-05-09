/**
 * ReviewTimeline - macOS Tahoe-inspired review history timeline
 *
 * Shows a vertical timeline of review events with collapsible markdown content.
 */

import { Fragment } from "react";
import { CheckCircle2, RotateCcw, Bot, User, Settings, ExternalLink } from "lucide-react";
import type { ReviewNoteResponse } from "@/lib/tauri";
import type { StateTransition } from "@/api/tasks";
import { navigateToIdeationSession } from "@/lib/navigation";
import { withAlpha } from "@/lib/theme-colors";
import { ReviewFeedbackBody } from "@/components/reviews/ReviewFeedbackBody";
import { getReviewerTypeLabel } from "@/lib/review-feedback";

// ============================================================================
// Staleness Detection
// ============================================================================

/**
 * Statuses that indicate the task progressed past a "changes_requested" review.
 * A review is stale if any transition to these statuses happened after it.
 */
const STALE_TRIGGER_STATUSES = new Set<string>([
  "re_executing",
  "approved",
  "pending_merge",
  "merging",
  "merged",
]);

/**
 * Milestone statuses shown in the resolution trail (in display order).
 */
const TRAIL_MILESTONES: { status: string; label: string }[] = [
  { status: "re_executing", label: "Re-execution" },
  { status: "approved", label: "Approved" },
  { status: "merged", label: "Merged" },
];

/**
 * Returns true if the given "changes_requested" review has been superseded
 * by a subsequent state transition (e.g. re_executing, approved, merged).
 */
function isReviewStale(
  review: ReviewNoteResponse,
  stateTransitions: StateTransition[]
): boolean {
  if (review.outcome !== "changes_requested") return false;
  const reviewDate = new Date(review.created_at);
  return stateTransitions.some(
    (t) =>
      STALE_TRIGGER_STATUSES.has(t.toStatus) &&
      new Date(t.timestamp) > reviewDate
  );
}

/**
 * Returns the ordered milestone transitions that occurred after the given review.
 * Used to render the resolution trail below a stale review.
 */
function getResolutionTrail(
  review: ReviewNoteResponse,
  stateTransitions: StateTransition[]
): { label: string; timestamp: string }[] {
  const reviewDate = new Date(review.created_at);
  const result: { label: string; timestamp: string }[] = [];

  for (const { status, label } of TRAIL_MILESTONES) {
    // Find the first transition to this milestone status after the review
    const transition = stateTransitions
      .filter(
        (t) => t.toStatus === status && new Date(t.timestamp) > reviewDate
      )
      .sort((a, b) => new Date(a.timestamp).getTime() - new Date(b.timestamp).getTime())[0];

    if (transition) {
      result.push({ label, timestamp: transition.timestamp });
    }
  }

  return result;
}

// ============================================================================
// Helpers
// ============================================================================

function formatRelativeTime(date: Date | string | undefined): string {
  if (!date) return "Unknown";

  const now = new Date();
  const then = typeof date === "string" ? new Date(date) : date;
  const diffMs = now.getTime() - then.getTime();
  const diffMins = Math.floor(diffMs / 60000);
  const diffHours = Math.floor(diffMins / 60);
  const diffDays = Math.floor(diffHours / 24);

  if (diffMins < 1) return "Just now";
  if (diffMins < 60) return `${diffMins}m ago`;
  if (diffHours < 24) return `${diffHours}h ago`;
  return `${diffDays}d ago`;
}

// ============================================================================
// TimelineItem
// ============================================================================

interface TimelineItemProps {
  entry: ReviewNoteResponse;
  isLast: boolean;
  entryContext?: string | null;
  attemptNumber?: number;
  isStale?: boolean;
  resolutionTrail?: { label: string; timestamp: string }[];
}

function TimelineItem({
  entry,
  isLast,
  entryContext = null,
  attemptNumber,
  isStale = false,
  resolutionTrail = [],
}: TimelineItemProps) {
  const isApproved = entry.outcome === "approved" || entry.outcome === "approved_no_changes";
  const isNoChanges = entry.outcome === "approved_no_changes";
  const isChangesRequested = entry.outcome === "changes_requested";
  const isHuman = entry.reviewer === "human";
  const isSystem = entry.reviewer === "system";

  const hasSummary = !!entry.summary;
  const hasNotes = !!entry.notes;
  const hasContent = hasSummary || hasNotes;

  const getConfig = () => {
    if (isChangesRequested) {
      return {
        Icon: RotateCcw,
        color: "color-mix(in srgb, var(--status-warning) 76%, var(--text-muted))",
        bgColor: "var(--overlay-moderate)",
        lineColor: withAlpha("var(--text-muted)", 20),
      };
    }
    return {
      Icon: CheckCircle2,
      color: isApproved
        ? "color-mix(in srgb, var(--status-success) 76%, var(--text-muted))"
        : "var(--text-muted)",
      bgColor: "var(--overlay-moderate)",
      lineColor: withAlpha("var(--text-muted)", 20),
    };
  };

  const config = getConfig();
  const ReviewerIcon = isHuman ? User : isSystem ? Settings : Bot;
  const reviewerLabel = getReviewerTypeLabel(entry.reviewer).replace(" Review", "");
  const previewClassName = [
    "text-[0.8125rem] text-text-primary leading-relaxed",
    "[&_.prose]:!text-text-primary [&_.prose]:text-[0.8125rem] [&_.prose]:leading-relaxed",
    "[&_p]:!text-text-primary [&_p]:text-[0.8125rem] [&_p]:leading-relaxed",
    "[&_li]:!text-text-primary [&_ul]:!text-text-primary [&_ol]:!text-text-primary",
    "[&_li]:text-[0.8125rem] [&_ul]:text-[0.8125rem] [&_ol]:text-[0.8125rem]",
    "[&_code]:!text-text-primary [&_code]:text-[0.75rem]",
  ].join(" ");

  const getLabel = () => {
    if (attemptNumber !== undefined && isChangesRequested) {
      return `Attempt #${attemptNumber}: Changes requested`;
    }
    if (isNoChanges) {
      return `${reviewerLabel} approved (no changes)`;
    }
    if (isApproved) {
      return `${reviewerLabel} approved`;
    }
    if (isChangesRequested) {
      return `${reviewerLabel} requested changes`;
    }
    return `${reviewerLabel} reviewed`;
  };

  return (
    <div
      className="flex gap-3 py-3 first:pt-0 last:pb-0"
      style={
        !isLast
          ? { borderBottom: "1px solid var(--border-subtle)" }
          : undefined
      }
    >
      {/* Timeline connector — dimmed for stale reviews */}
      <div
        className="flex flex-col items-center"
        style={isStale ? { opacity: 0.6 } : undefined}
      >
        {/* Icon circle */}
        <div
          className="flex items-center justify-center w-7 h-7 rounded-xl shrink-0"
          style={{ backgroundColor: config.bgColor }}
        >
          <config.Icon className="w-4 h-4" style={{ color: config.color }} />
        </div>
        {/* Vertical line */}
        {(!isLast || (isStale && resolutionTrail.length > 0)) && (
          <div
            className="w-0.5 flex-1 min-h-[20px] mt-1"
            style={{ backgroundColor: config.lineColor }}
          />
        )}
      </div>

      {/* Content */}
      <div className="flex-1">
        {/* Main content — dimmed for stale reviews */}
        <div style={isStale ? { opacity: 0.6 } : undefined}>
          <div className="flex items-center gap-2">
            <ReviewerIcon
              className="w-3.5 h-3.5"
              style={{
                color: isHuman
                  ? "color-mix(in srgb, var(--status-success) 70%, var(--text-muted))"
                  : isSystem
                    ? "color-mix(in srgb, var(--status-warning) 70%, var(--text-muted))"
                    : "color-mix(in srgb, var(--status-info) 70%, var(--text-muted))",
              }}
            />
            <span className="text-[0.75rem] font-semibold text-text-primary/70">
              {getLabel()}
            </span>
            {entryContext && (
              <span
                className="text-[0.625rem] text-text-primary/40 truncate min-w-0 flex-1"
                title={entryContext}
              >
                {entryContext}
              </span>
            )}
            {isStale && (
              <span
                className="text-[0.5625rem] font-semibold uppercase tracking-wider px-1.5 py-0.5 rounded"
                style={{
                  backgroundColor: "var(--overlay-weak)",
                  color: "var(--text-muted)",
                }}
              >
                Superseded
              </span>
            )}
            <span className="text-[0.625rem] text-text-primary/40 ml-auto">
              {formatRelativeTime(entry.created_at)}
            </span>
          </div>
          {hasContent && (
            <div className="mt-1.5 pl-5">
              <ReviewFeedbackBody
                summary={entry.summary ?? null}
                notes={entry.notes ?? null}
                dialogTitle="Full review feedback"
                dialogDescription="Full review feedback in a scrollable view."
                fullButtonLabel="View full feedback"
                fullButtonClassName="text-[0.75rem]"
                previewClassName={previewClassName}
              />

              {entry.followup_session_id && (
                <div className="mt-3 flex items-center justify-between gap-2 rounded-lg px-2.5 py-2"
                  style={{
                    backgroundColor: "var(--overlay-weak)",
                    border: "1px solid var(--border-subtle)",
                  }}
                >
                  <div className="min-w-0">
                    <div className="text-[0.6875rem] font-medium text-text-primary/65">
                      Follow-up ideation session
                    </div>
                    <div className="mt-0.5 text-[0.6875rem] text-text-primary/45 break-all">
                      {entry.followup_session_id}
                    </div>
                  </div>
                  <button
                    type="button"
                    onClick={() => navigateToIdeationSession(entry.followup_session_id!)}
                    className="shrink-0 inline-flex items-center gap-1 rounded-md px-2 py-1 text-[0.6875rem] font-medium transition-opacity hover:opacity-80"
                    style={{
                      color: "var(--accent-primary)",
                      backgroundColor: "var(--overlay-faint)",
                    }}
                  >
                    <ExternalLink className="w-3 h-3" />
                    Open
                  </button>
                </div>
              )}
            </div>
          )}
        </div>

        {/* Resolution trail — rendered at full opacity below the stale review */}
        {isStale && resolutionTrail.length > 0 && (
          <div className="pl-5 mt-1.5 flex items-center gap-1 flex-wrap">
            {resolutionTrail.map((item, i) => (
              <Fragment key={`${item.label}-${i}`}>
                <span className="text-[0.625rem]" style={{ color: "var(--text-muted)" }}>
                  {item.label}{" "}
                  <span style={{ color: "var(--text-muted)" }}>
                    ({formatRelativeTime(item.timestamp)})
                  </span>
                </span>
                {i < resolutionTrail.length - 1 && (
                  <span
                    className="text-[0.625rem]"
                    style={{ color: "var(--text-muted)" }}
                  >
                    →
                  </span>
                )}
              </Fragment>
            ))}
          </div>
        )}
      </div>
    </div>
  );
}

// ============================================================================
// ReviewTimeline
// ============================================================================

export interface ReviewTimelineProps {
  history: ReviewNoteResponse[];
  filter?: (entry: ReviewNoteResponse) => boolean;
  emptyMessage?: string;
  getEntryContext?: (entry: ReviewNoteResponse) => string | null | undefined;
  showAttemptNumbers?: boolean;
  /** State transitions used to detect stale "changes_requested" reviews */
  stateTransitions?: StateTransition[];
}

export function ReviewTimeline({
  history,
  filter,
  emptyMessage = "No review history available",
  getEntryContext,
  showAttemptNumbers = false,
  stateTransitions = [],
}: ReviewTimelineProps) {
  const displayedHistory = filter ? history.filter(filter) : history;

  if (displayedHistory.length === 0) {
    return (
      <p className="text-[0.75rem] text-text-primary/35 italic py-2">
        {emptyMessage}
      </p>
    );
  }

  return (
    <div data-testid="review-history-timeline">
      {displayedHistory.map((entry, index) => {
        const stale =
          stateTransitions.length > 0 && isReviewStale(entry, stateTransitions);
        const trail = stale
          ? getResolutionTrail(entry, stateTransitions)
          : [];

        return (
          <TimelineItem
            key={entry.id}
            entry={entry}
            isLast={index === displayedHistory.length - 1}
            entryContext={getEntryContext?.(entry) ?? null}
            isStale={stale}
            resolutionTrail={trail}
            {...(showAttemptNumbers && {
              // Array is newest-first, so invert: oldest = #1, newest = #N
              attemptNumber: displayedHistory.length - index,
            })}
          />
        );
      })}
    </div>
  );
}
