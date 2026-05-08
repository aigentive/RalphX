/**
 * ReviewWidget — Specialized renderer for complete_review and get_review_notes tool calls.
 *
 * complete_review: outcome-colored card showing decision, feedback, and issues.
 * get_review_notes: compact list of review notes with timestamps and outcomes.
 */

import React, { useState, useMemo } from "react";
import {
  CheckCircle,
  AlertTriangle,
  AlertCircle,
  ChevronDown,
  ChevronRight,
  FileText,
  Clock,
  ExternalLink,
} from "lucide-react";
import type { ToolCallWidgetProps } from "./shared";
import { parseMcpToolResult, getArray } from "./shared.constants";
import { navigateToIdeationSession } from "@/lib/navigation";

// ============================================================================
// Types
// ============================================================================

interface ReviewIssue {
  severity: string;
  file?: string;
  line?: number;
  description: string;
}

interface CompleteReviewArgs {
  task_id?: string;
  decision?: string;
  feedback?: string;
  summary?: string;
  issues?: ReviewIssue[];
}

interface CompleteReviewResult {
  success?: boolean;
  message?: string;
  new_status?: string;
  fix_task_id?: string;
  followup_session_id?: string;
}

interface ReviewNoteEntry {
  id: string;
  reviewer: string;
  outcome: string;
  summary?: string;
  notes?: string;
  issues?: ReviewIssue[];
  created_at: string;
}

// ============================================================================
// Helpers
// ============================================================================

const OUTCOME_STYLES = {
  approved: {
    bg: "var(--status-success-muted)",
    border: "var(--status-success-border)",
    accent: "var(--status-success)",
    text: "var(--status-success)",
    label: "Approved",
    Icon: CheckCircle,
  },
  approved_no_changes: {
    bg: "var(--status-success-muted)",
    border: "var(--status-success-border)",
    accent: "var(--status-success)",
    text: "var(--status-success)",
    label: "No Changes",
    Icon: CheckCircle,
  },
  needs_changes: {
    bg: "var(--accent-muted)",
    border: "var(--accent-border)",
    accent: "var(--accent-primary)",
    text: "var(--accent-primary)",
    label: "Changes Requested",
    Icon: AlertTriangle,
  },
  changes_requested: {
    bg: "var(--accent-muted)",
    border: "var(--accent-border)",
    accent: "var(--accent-primary)",
    text: "var(--accent-primary)",
    label: "Changes Requested",
    Icon: AlertTriangle,
  },
  escalate: {
    bg: "var(--status-info-muted)",
    border: "var(--status-info-border)",
    accent: "var(--status-info)",
    text: "var(--status-info)",
    label: "Escalated",
    Icon: AlertCircle,
  },
  rejected: {
    bg: "var(--status-info-muted)",
    border: "var(--status-info-border)",
    accent: "var(--status-info)",
    text: "var(--status-info)",
    label: "Escalated",
    Icon: AlertCircle,
  },
} as const;

type OutcomeKey = keyof typeof OUTCOME_STYLES;

const DEFAULT_STYLE = {
  bg: "var(--bg-elevated)",
  border: "var(--border-subtle)",
  accent: "var(--text-secondary)",
  text: "var(--text-secondary)",
  label: "Review",
  Icon: FileText,
};

function getOutcomeStyle(outcome: string | undefined) {
  if (!outcome) return DEFAULT_STYLE;
  return OUTCOME_STYLES[outcome as OutcomeKey] ?? DEFAULT_STYLE;
}

function getSeverityColor(severity: string): string {
  switch (severity) {
    case "critical":
      return "var(--status-error)";
    case "major":
      return "var(--accent-primary)";
    case "minor":
      return "var(--status-warning)";
    case "suggestion":
      return "var(--status-info)";
    default:
      return "var(--text-secondary)";
  }
}

function getSeverityBg(severity: string): string {
  switch (severity) {
    case "critical":
      return "var(--status-error-muted)";
    case "major":
      return "var(--accent-muted)";
    case "minor":
      return "var(--status-warning-muted)";
    case "suggestion":
      return "var(--status-info-muted)";
    default:
      return "var(--bg-hover)";
  }
}

function formatTimestamp(ts: string): string {
  try {
    const date = new Date(ts);
    return date.toLocaleTimeString(undefined, {
      hour: "2-digit",
      minute: "2-digit",
    });
  } catch {
    return "";
  }
}

// ============================================================================
// CompleteReview Renderer
// ============================================================================

function CompleteReviewCard({ toolCall, className = "", compact = false }: ToolCallWidgetProps) {
  const [isExpanded, setIsExpanded] = useState(false);
  const args = useMemo((): CompleteReviewArgs => {
    const raw = typeof toolCall.arguments === "string"
      ? (() => { try { return JSON.parse(toolCall.arguments as string); } catch { return {}; } })()
      : (toolCall.arguments ?? {});
    return (raw as CompleteReviewArgs);
  }, [toolCall.arguments]);
  const result = useMemo((): CompleteReviewResult | null => {
    if (!toolCall.result) return null;
    const parsed = parseMcpToolResult(toolCall.result);
    return Object.keys(parsed).length > 0 ? (parsed as unknown as CompleteReviewResult) : null;
  }, [toolCall.result]);
  const hasError = Boolean(toolCall.error);

  const style = getOutcomeStyle(args.decision);
  const { Icon } = style;
  const issues = args.issues ?? [];
  const followupSessionId = result?.followup_session_id;
  const hasBody = Boolean(args.feedback) || issues.length > 0 || Boolean(followupSessionId);
  const iconSize = compact ? 12 : 14;

  return (
    <div
      data-testid="review-widget-complete"
      className={`${compact ? "rounded-md" : "rounded-lg"} overflow-hidden ${compact ? "mb-1" : ""} ${className}`}
      style={{
        backgroundColor: hasError ? "var(--status-error-muted)" : style.bg,
        borderLeft: `3px solid ${hasError ? "var(--status-error)" : style.accent}`,
      }}
    >
      {/* Header */}
      <div
        onClick={() => hasBody && setIsExpanded(!isExpanded)}
        onKeyDown={(event) => {
          if (!hasBody) {
            return;
          }
          if (event.key === "Enter" || event.key === " ") {
            event.preventDefault();
            setIsExpanded((prev) => !prev);
          }
        }}
        className={`w-full flex items-center flex-wrap gap-2 ${compact ? "px-2 py-1.5" : "px-3 py-2"} text-left ${hasBody ? "hover:opacity-80 cursor-pointer" : "cursor-default"} transition-opacity`}
        aria-expanded={hasBody ? isExpanded : undefined}
        role={hasBody ? "button" : undefined}
        tabIndex={hasBody ? 0 : undefined}
      >
        {/* Expand/collapse chevron */}
        {hasBody ? (
          isExpanded ? (
            <ChevronDown size={iconSize} className="flex-shrink-0" style={{ color: "var(--text-muted)" }} />
          ) : (
            <ChevronRight size={iconSize} className="flex-shrink-0" style={{ color: "var(--text-muted)" }} />
          )
        ) : null}

        {/* Outcome icon */}
        <Icon size={iconSize} className="flex-shrink-0" style={{ color: style.accent }} />

        {/* Outcome badge */}
        <span
          className={`${compact ? "text-[0.5625rem]" : "text-[0.625rem]"} px-1.5 py-0.5 rounded font-medium flex-shrink-0`}
          style={{ backgroundColor: style.bg, color: style.text, border: `1px solid ${style.border}` }}
        >
          {style.label}
        </span>

        {/* Issue count or summary */}
        <span
          className={`${compact ? "text-[0.6875rem]" : "text-xs"} truncate flex-1 min-w-[80px]`}
          style={{ color: hasError ? "var(--status-error)" : "var(--text-secondary)" }}
        >
          {issues.length > 0
            ? `${issues.length} issue${issues.length !== 1 ? "s" : ""} found`
            : args.summary || (result?.new_status ? `→ ${result.new_status.replace(/_/g, " ")}` : "")}
        </span>

        {/* Error indicator */}
        {hasError && (
          <span
            className={`${compact ? "text-[0.5625rem]" : "text-[0.625rem]"} font-medium px-1.5 py-0.5 rounded`}
            style={{ backgroundColor: "var(--status-error-muted)", color: "var(--status-error)" }}
          >
            Failed
          </span>
        )}

        {followupSessionId && (
          <button
            type="button"
            onClick={(event) => {
              event.stopPropagation();
              navigateToIdeationSession(followupSessionId);
            }}
            className={`${compact ? "text-[0.5625rem]" : "text-[0.625rem]"} flex items-center gap-1 px-1.5 py-0.5 rounded transition-opacity hover:opacity-80 flex-shrink-0`}
            style={{
              backgroundColor: "var(--accent-muted)",
              color: "var(--accent-primary)",
              border: "1px solid var(--accent-border)",
            }}
          >
            <ExternalLink size={compact ? 9 : 10} />
            Open Follow-up
          </button>
        )}
      </div>

      {/* Expanded body */}
      {isExpanded && hasBody && (
        <div
          className={`${compact ? "px-2 pb-2" : "px-3 pb-3"} space-y-2 pt-1`}
          style={{ borderTop: "1px solid var(--overlay-faint)" }}
        >
          {/* Feedback text */}
          {args.feedback && (
            <div
              className={`${compact ? "text-[0.625rem]" : "text-[0.6875rem]"} px-2 py-1.5 rounded`}
              style={{
                backgroundColor: "var(--bg-surface)",
                color: "var(--text-secondary)",
                whiteSpace: "pre-wrap",
                lineHeight: 1.5,
              }}
            >
              {args.feedback}
            </div>
          )}

          {/* Issues list */}
          {issues.length > 0 && (
            <div className="space-y-1">
              <div
                className={`${compact ? "text-[0.5625rem]" : "text-[0.625rem]"} font-medium uppercase tracking-wide`}
                style={{ color: "var(--text-muted)" }}
              >
                Issues
              </div>
              {issues.map((issue, idx) => (
                <div
                  key={idx}
                  className={`${compact ? "text-[0.625rem]" : "text-[0.6875rem]"} px-2 py-1.5 rounded flex items-start gap-2`}
                  style={{ backgroundColor: "var(--bg-surface)" }}
                >
                  {/* Severity badge */}
                  <span
                    className="text-[0.5625rem] px-1 py-0.5 rounded flex-shrink-0 font-medium mt-0.5"
                    style={{ backgroundColor: getSeverityBg(issue.severity), color: getSeverityColor(issue.severity) }}
                  >
                    {issue.severity}
                  </span>
                  <div className="flex-1 min-w-0">
                    <div style={{ color: "var(--text-primary)" }}>{issue.description}</div>
                    {issue.file && (
                      <div className="mt-0.5 font-mono text-[0.5625rem]" style={{ color: "var(--text-muted)" }}>
                        {issue.file}{issue.line != null ? `:${issue.line}` : ""}
                      </div>
                    )}
                  </div>
                </div>
              ))}
            </div>
          )}

          {followupSessionId && (
            <div className="space-y-1">
              <div
                className={`${compact ? "text-[0.5625rem]" : "text-[0.625rem]"} font-medium uppercase tracking-wide`}
                style={{ color: "var(--text-muted)" }}
              >
                Follow-up Session
              </div>
              <div
                className={`${compact ? "text-[0.625rem]" : "text-[0.6875rem]"} px-2 py-1.5 rounded flex items-center justify-between gap-2`}
                style={{ backgroundColor: "var(--bg-surface)" }}
              >
                <span style={{ color: "var(--text-secondary)" }}>{followupSessionId}</span>
                <button
                  type="button"
                  onClick={() => navigateToIdeationSession(followupSessionId)}
                  className="flex items-center gap-1 px-2 py-1 rounded transition-opacity hover:opacity-80"
                  style={{
                    backgroundColor: "var(--accent-muted)",
                    color: "var(--accent-primary)",
                    border: "1px solid var(--accent-border)",
                  }}
                >
                  <ExternalLink size={compact ? 10 : 11} />
                  Open
                </button>
              </div>
            </div>
          )}
        </div>
      )}
    </div>
  );
}

// ============================================================================
// GetReviewNotes Renderer
// ============================================================================

function GetReviewNotesCard({ toolCall, className = "", compact = false }: ToolCallWidgetProps) {
  const [isExpanded, setIsExpanded] = useState(false);
  const { reviews, revisionCount, maxRevisions, severityCounts } = useMemo(() => {
    const parsed = parseMcpToolResult(toolCall.result);
    const reviewList = (getArray(parsed, "reviews") as ReviewNoteEntry[] | undefined) ?? [];
    const counts: Record<string, number> = {};
    for (const review of reviewList) {
      for (const issue of review.issues ?? []) {
        counts[issue.severity] = (counts[issue.severity] ?? 0) + 1;
      }
    }
    return {
      reviews: reviewList,
      revisionCount: typeof parsed.revision_count === "number" ? parsed.revision_count : 0,
      maxRevisions: typeof parsed.max_revisions === "number" ? parsed.max_revisions : undefined,
      severityCounts: counts,
    };
  }, [toolCall.result]);
  const hasError = Boolean(toolCall.error);

  const hasBody = reviews.length > 0;
  const iconSize = compact ? 12 : 14;

  // Approval status from latest review
  const latestOutcome = reviews.length > 0 ? reviews[reviews.length - 1]?.outcome : undefined;
  const latestStyle = latestOutcome ? getOutcomeStyle(latestOutcome) : null;

  // Empty state
  if (!hasError && reviews.length === 0 && toolCall.result != null) {
    return (
      <div
        data-testid="review-widget-notes-empty"
        className={`${compact ? "rounded-md" : "rounded-lg"} overflow-hidden ${compact ? "mb-1" : ""} ${className}`}
        style={{ backgroundColor: "var(--bg-elevated)" }}
      >
        <div className={`flex items-center gap-2 ${compact ? "px-2 py-1.5" : "px-3 py-2"}`}>
          <FileText size={iconSize} className="flex-shrink-0" style={{ color: "var(--text-muted)" }} />
          <span className={`${compact ? "text-[0.6875rem]" : "text-xs"}`} style={{ color: "var(--text-muted)" }}>
            No review notes
          </span>
        </div>
      </div>
    );
  }

  return (
    <div
      data-testid="review-widget-notes"
      className={`${compact ? "rounded-md" : "rounded-lg"} overflow-hidden ${compact ? "mb-1" : ""} ${className}`}
      style={{
        backgroundColor: hasError ? "var(--status-error-muted)" : "var(--bg-elevated)",
        border: "none",
      }}
    >
      {/* Header */}
      <button
        onClick={() => hasBody && setIsExpanded(!isExpanded)}
        className={`w-full flex items-center flex-wrap gap-2 ${compact ? "px-2 py-1.5" : "px-3 py-2"} text-left ${hasBody ? "hover:opacity-80 cursor-pointer" : "cursor-default"} transition-opacity`}
        aria-expanded={hasBody ? isExpanded : undefined}
      >
        {hasBody ? (
          isExpanded ? (
            <ChevronDown size={iconSize} className="flex-shrink-0" style={{ color: "var(--text-muted)" }} />
          ) : (
            <ChevronRight size={iconSize} className="flex-shrink-0" style={{ color: "var(--text-muted)" }} />
          )
        ) : null}

        <FileText size={iconSize} className="flex-shrink-0" style={{ color: "var(--accent-primary)" }} />

        {/* Approval status badge from latest review */}
        {latestStyle && (
          <span
            className={`${compact ? "text-[0.5625rem]" : "text-[0.625rem]"} px-1.5 py-0.5 rounded font-medium flex-shrink-0`}
            style={{ backgroundColor: latestStyle.bg, color: latestStyle.text, border: `1px solid ${latestStyle.border}` }}
          >
            {latestStyle.label}
          </span>
        )}

        <span
          className={`${compact ? "text-[0.6875rem]" : "text-xs"} flex-1 min-w-[80px] break-words`}
          style={{ color: "var(--text-secondary)" }}
        >
          {reviews.length} review note{reviews.length !== 1 ? "s" : ""}
        </span>

        {/* Revision counter badge */}
        {revisionCount > 0 && (
          <span
            className={`${compact ? "text-[0.5625rem]" : "text-[0.625rem]"} px-1.5 py-0.5 rounded flex-shrink-0`}
            style={{
              backgroundColor: "var(--accent-muted)",
              color: "var(--accent-primary)",
            }}
          >
            {revisionCount}{maxRevisions != null ? `/${maxRevisions}` : ""} revision{revisionCount !== 1 ? "s" : ""}
          </span>
        )}

        {hasError && (
          <span
            className={`${compact ? "text-[0.5625rem]" : "text-[0.625rem]"} font-medium px-1.5 py-0.5 rounded`}
            style={{ backgroundColor: "var(--status-error-muted)", color: "var(--status-error)" }}
          >
            Failed
          </span>
        )}

        {/* Severity breakdown badges (wrapped second line) */}
        {Object.entries(severityCounts).map(([severity, count]) =>
          count > 0 ? (
            <span
              key={severity}
              className="text-[0.5625rem] px-1 py-0.5 rounded font-medium flex-shrink-0"
              style={{ backgroundColor: getSeverityBg(severity), color: getSeverityColor(severity) }}
            >
              {severity}: {count}
            </span>
          ) : null
        )}
      </button>

      {/* Expanded: note list */}
      {isExpanded && hasBody && (
        <div
          className={`${compact ? "px-2 pb-2" : "px-3 pb-3"} space-y-1.5 pt-1`}
          style={{ borderTop: "1px solid var(--overlay-faint)" }}
        >
          {reviews.map((note) => {
            const noteStyle = getOutcomeStyle(note.outcome);
            const NoteIcon = noteStyle.Icon;
            return (
              <div
                key={note.id}
                className={`${compact ? "text-[0.625rem]" : "text-[0.6875rem]"} px-2 py-1.5 rounded`}
                style={{ backgroundColor: "var(--bg-surface)" }}
              >
                <div className="flex items-center gap-1.5 mb-1">
                  <NoteIcon size={10} style={{ color: noteStyle.accent }} />
                  <span className="font-medium" style={{ color: noteStyle.text }}>
                    {noteStyle.label}
                  </span>
                  <span className="text-[0.5625rem]" style={{ color: "var(--text-muted)" }}>
                    {note.reviewer}
                  </span>
                  {note.created_at && (
                    <span className="flex items-center gap-0.5 text-[0.5625rem] ml-auto" style={{ color: "var(--text-muted)" }}>
                      <Clock size={8} />
                      {formatTimestamp(note.created_at)}
                    </span>
                  )}
                </div>
                {note.notes && (
                  <div style={{ color: "var(--text-secondary)", lineHeight: 1.4, whiteSpace: "pre-wrap" }}>
                    {note.notes.length > 200 ? note.notes.slice(0, 200) + "..." : note.notes}
                  </div>
                )}
                {note.issues && note.issues.length > 0 && (
                  <div className="mt-1 flex items-center gap-1">
                    <span style={{ color: "var(--text-muted)" }}>
                      {note.issues.length} issue{note.issues.length !== 1 ? "s" : ""}
                    </span>
                  </div>
                )}
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
}

// ============================================================================
// Main Widget (dispatches based on tool name)
// ============================================================================

export const ReviewWidget = React.memo(function ReviewWidget(props: ToolCallWidgetProps) {
  const toolName = props.toolCall.name.toLowerCase();
  if (toolName.includes("complete_review")) {
    return <CompleteReviewCard {...props} />;
  }
  return <GetReviewNotesCard {...props} />;
});
