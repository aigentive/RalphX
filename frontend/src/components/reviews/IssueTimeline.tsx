/**
 * IssueTimeline - Shows issue lifecycle across attempts
 *
 * Displays the timeline of events for a single issue:
 * - Created in Review 1
 * - Addressed in Attempt 2
 * - Verified in Review 2
 *
 * Following macOS Tahoe design:
 * - Flat, solid background colors
 * - Blue-gray palette
 * - Small typography (11-13px)
 * - No shadows or gradients
 */

import {
  AlertCircle,
  CheckCircle2,
  Clock,
  RotateCcw,
  XCircle,
  FileCode,
} from "lucide-react";
import type { ReviewIssue, IssueStatus } from "@/types/review-issue";
import { withAlpha } from "@/lib/theme-colors";

// ============================================================================
// Types
// ============================================================================

interface TimelineEvent {
  type: "created" | "in_progress" | "addressed" | "verified" | "reopened" | "wontfix";
  timestamp: string;
  details?: string | undefined;
}

export interface IssueTimelineProps {
  issue: ReviewIssue;
  showFileLink?: boolean;
}

// ============================================================================
// Helpers
// ============================================================================

const EVENT_CONFIG: Record<
  TimelineEvent["type"],
  { icon: typeof AlertCircle; color: string; bgColor: string; label: string }
> = {
  created: {
    icon: AlertCircle,
    color: "var(--status-info)",
    bgColor: withAlpha("var(--status-info)", 15),
    label: "Issue created",
  },
  in_progress: {
    icon: Clock,
    color: "var(--status-warning)",
    bgColor: withAlpha("var(--status-warning)", 15),
    label: "Work started",
  },
  addressed: {
    icon: CheckCircle2,
    color: "var(--status-success)",
    bgColor: withAlpha("var(--status-success)", 15),
    label: "Addressed",
  },
  verified: {
    icon: CheckCircle2,
    color: "var(--status-success)",
    bgColor: withAlpha("var(--status-success)", 20),
    label: "Verified",
  },
  reopened: {
    icon: RotateCcw,
    color: "var(--status-warning)",
    bgColor: withAlpha("var(--status-warning)", 15),
    label: "Reopened",
  },
  wontfix: {
    icon: XCircle,
    color: "var(--text-muted)",
    bgColor: withAlpha("var(--text-muted)", 15),
    label: "Won't fix",
  },
};

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

/**
 * Build timeline events from issue data
 * We infer events from the issue's current state
 */
function buildTimelineEvents(issue: ReviewIssue): TimelineEvent[] {
  const events: TimelineEvent[] = [];

  // Always has a created event
  events.push({
    type: "created",
    timestamp: issue.createdAt,
    details: `Severity: ${issue.severity}`,
  });

  // If status is beyond open, infer intermediate events
  const statusOrder: IssueStatus[] = [
    "open",
    "in_progress",
    "addressed",
    "verified",
  ];
  const currentIndex = statusOrder.indexOf(issue.status);

  // If issue is in_progress or beyond, add in_progress event
  if (currentIndex >= 1 || issue.status === "in_progress") {
    events.push({
      type: "in_progress",
      timestamp: issue.updatedAt, // Best approximation
    });
  }

  // If issue is addressed or beyond
  if (currentIndex >= 2 || issue.status === "addressed") {
    events.push({
      type: "addressed",
      timestamp: issue.updatedAt,
      details: issue.addressedInAttempt
        ? `In attempt #${issue.addressedInAttempt}`
        : undefined,
    });
  }

  // If issue is verified
  if (issue.status === "verified") {
    events.push({
      type: "verified",
      timestamp: issue.updatedAt,
    });
  }

  // If issue is wontfix
  if (issue.status === "wontfix") {
    events.push({
      type: "wontfix",
      timestamp: issue.updatedAt,
      details: issue.resolutionNotes ?? undefined,
    });
  }

  return events;
}

// ============================================================================
// Sub-components
// ============================================================================

interface TimelineEventItemProps {
  event: TimelineEvent;
  isLast: boolean;
}

function TimelineEventItem({ event, isLast }: TimelineEventItemProps) {
  const config = EVENT_CONFIG[event.type];
  const Icon = config.icon;

  return (
    <div className="flex gap-3">
      {/* Timeline connector */}
      <div className="flex flex-col items-center">
        {/* Icon circle */}
        <div
          className="flex items-center justify-center w-6 h-6 rounded-lg shrink-0"
          style={{ backgroundColor: config.bgColor }}
        >
          <Icon className="w-3.5 h-3.5" style={{ color: config.color }} />
        </div>
        {/* Vertical line */}
        {!isLast && (
          <div
            className="w-0.5 flex-1 min-h-[16px] mt-1"
            style={{ backgroundColor: "var(--bg-hover)" }}
          />
        )}
      </div>

      {/* Content */}
      <div className="flex-1 pb-3">
        <div className="flex items-center gap-2">
          <span
            className="text-[0.75rem] font-medium"
            style={{ color: "var(--text-primary)" }}
          >
            {config.label}
          </span>
          <span
            className="text-[0.6875rem] ml-auto"
            style={{ color: "var(--text-muted)" }}
          >
            {formatRelativeTime(event.timestamp)}
          </span>
        </div>
        {event.details && (
          <p
            className="text-[0.6875rem] mt-0.5"
            style={{ color: "var(--text-muted)" }}
          >
            {event.details}
          </p>
        )}
      </div>
    </div>
  );
}

// ============================================================================
// IssueTimeline Main Component
// ============================================================================

export function IssueTimeline({ issue, showFileLink = true }: IssueTimelineProps) {
  const events = buildTimelineEvents(issue);
  const hasFileLink = issue.filePath && issue.lineNumber;

  return (
    <div
      className="rounded-lg p-3"
      style={{ backgroundColor: "var(--bg-surface)" }}
    >
      {/* Issue header */}
      <div className="mb-3 pb-3" style={{ borderBottom: "1px solid var(--border-subtle)" }}>
        <h4
          className="text-[0.8125rem] font-medium"
          style={{ color: "var(--text-primary)" }}
        >
          {issue.title}
        </h4>
        {issue.description && (
          <p
            className="text-[0.75rem] mt-1"
            style={{ color: "var(--text-secondary)" }}
          >
            {issue.description}
          </p>
        )}
        {showFileLink && hasFileLink && (
          <div className="flex items-center gap-1.5 mt-2">
            <FileCode className="w-3 h-3" style={{ color: "var(--text-muted)" }} />
            <span
              className="text-[0.6875rem] font-mono"
              style={{ color: "var(--status-info)" }}
            >
              {issue.filePath}:{issue.lineNumber}
            </span>
          </div>
        )}
      </div>

      {/* Timeline */}
      <div>
        {events.map((event, index) => (
          <TimelineEventItem
            key={`${event.type}-${index}`}
            event={event}
            isLast={index === events.length - 1}
          />
        ))}
      </div>

      {/* Resolution notes */}
      {issue.resolutionNotes && issue.status !== "wontfix" && (
        <div
          className="mt-2 pt-2"
          style={{ borderTop: "1px solid var(--border-subtle)" }}
        >
          <span
            className="text-[0.625rem] uppercase tracking-wider"
            style={{ color: "var(--text-muted)" }}
          >
            Resolution Notes
          </span>
          <p
            className="text-[0.6875rem] mt-1"
            style={{ color: "var(--text-secondary)" }}
          >
            {issue.resolutionNotes}
          </p>
        </div>
      )}
    </div>
  );
}

// ============================================================================
// Compact variant for inline display
// ============================================================================

export interface IssueTimelineCompactProps {
  issue: ReviewIssue;
}

/**
 * Compact timeline showing just status progression as dots
 */
export function IssueTimelineCompact({ issue }: IssueTimelineCompactProps) {
  const events = buildTimelineEvents(issue);

  return (
    <div className="flex items-center gap-1">
      {events.map((event, index) => {
        const config = EVENT_CONFIG[event.type];
        return (
          <div key={`${event.type}-${index}`} className="flex items-center">
            <div
              className="w-2 h-2 rounded-full"
              style={{ backgroundColor: config.color }}
              title={`${config.label} - ${formatRelativeTime(event.timestamp)}`}
            />
            {index < events.length - 1 && (
              <div
                className="w-3 h-0.5 mx-0.5"
                style={{ backgroundColor: "var(--bg-hover)" }}
              />
            )}
          </div>
        );
      })}
    </div>
  );
}
