/**
 * TimelineEntry.tsx - Individual event entry in the execution timeline
 *
 * Displays a single timeline event with:
 * - Timestamp (relative time)
 * - Task reference (if applicable)
 * - Event description
 * - Status color indicator using nodeStyles
 * - Clickable area for node interaction
 *
 * Renders a single task graph timeline event with node-aware styling.
 */

import { memo, useCallback, useMemo } from "react";
import { FileText, GitMerge, CheckCircle2 } from "lucide-react";
import { cn } from "@/lib/utils";
import type { TimelineEvent, TimelineEventType } from "@/api/task-graph.types";
import { getNodeStyle, getStatusCategory } from "../nodes/nodeStyles";

// ============================================================================
// Types
// ============================================================================

export interface TimelineEntryProps {
  /** The timeline event to display */
  event: TimelineEvent;
  /** Callback when clicking on a task-related event (for node highlighting) */
  onTaskClick?: (taskId: string) => void;
  /** Whether this entry's associated task is currently highlighted */
  isHighlighted?: boolean;
}

// ============================================================================
// Helper Functions
// ============================================================================

/**
 * Format timestamp as relative time (e.g., "2m ago", "1h ago", "Yesterday")
 */
function formatRelativeTime(timestamp: string): string {
  const date = new Date(timestamp);
  const now = new Date();
  const diffMs = now.getTime() - date.getTime();
  const diffSec = Math.floor(diffMs / 1000);
  const diffMin = Math.floor(diffSec / 60);
  const diffHour = Math.floor(diffMin / 60);
  const diffDay = Math.floor(diffHour / 24);

  if (diffMin < 1) return "Just now";
  if (diffMin < 60) return `${diffMin}m ago`;
  if (diffHour < 24) return `${diffHour}h ago`;
  if (diffDay === 1) return "Yesterday";
  if (diffDay < 7) return `${diffDay}d ago`;

  // For older events, show the date
  return date.toLocaleDateString("en-US", {
    month: "short",
    day: "numeric",
  });
}

/**
 * Format full timestamp for tooltip
 */
function formatFullTimestamp(timestamp: string): string {
  const date = new Date(timestamp);
  return date.toLocaleString("en-US", {
    month: "short",
    day: "numeric",
    hour: "numeric",
    minute: "2-digit",
  });
}

/**
 * Get the icon element for the event type
 */
function EventIcon({
  eventType,
  style,
}: {
  eventType: TimelineEventType;
  style: { color: string };
}) {
  const className = "w-2 h-2";
  switch (eventType) {
    case "plan_accepted":
      return <FileText className={className} style={style} />;
    case "plan_completed":
      return <CheckCircle2 className={className} style={style} />;
    case "status_change":
    default:
      // For status_change, use a filled circle (no icon, just the dot)
      return null;
  }
}

// ============================================================================
// Component
// ============================================================================

export const TimelineEntry = memo(function TimelineEntry({
  event,
  onTaskClick,
  isHighlighted = false,
}: TimelineEntryProps) {
  // Get the style based on the event's status
  const statusStyle = useMemo(() => {
    // For status_change events, use the "to" status
    if (event.eventType === "status_change" && event.toStatus) {
      return getNodeStyle(event.toStatus);
    }
    // For plan events, use complete style for plan_completed, idle for accepted
    if (event.eventType === "plan_completed") {
      return getNodeStyle("approved");
    }
    return getNodeStyle("ready");
  }, [event.eventType, event.toStatus]);

  // Determine the status category for additional styling
  const statusCategory = useMemo(() => {
    if (event.eventType === "status_change" && event.toStatus) {
      return getStatusCategory(event.toStatus as Parameters<typeof getStatusCategory>[0]);
    }
    if (event.eventType === "plan_completed") {
      return "complete";
    }
    return "idle";
  }, [event.eventType, event.toStatus]);

  // Handle click for task-related events
  const handleClick = useCallback(() => {
    if (event.taskId && onTaskClick) {
      onTaskClick(event.taskId);
    }
  }, [event.taskId, onTaskClick]);

  const isClickable = Boolean(event.taskId && onTaskClick);

  return (
    <div
      className={cn(
        "group relative flex items-start gap-2.5 mx-2 px-2 py-2 rounded-md transition-colors",
        isClickable && "cursor-pointer",
        statusCategory === "executing" && "animate-pulse"
      )}
      style={{
        background: isHighlighted ? "var(--status-info-muted)" : "transparent",
      }}
      onClick={isClickable ? handleClick : undefined}
      role={isClickable ? "button" : undefined}
      tabIndex={isClickable ? 0 : undefined}
      onKeyDown={
        isClickable
          ? (e) => {
              if (e.key === "Enter" || e.key === " ") {
                e.preventDefault();
                handleClick();
              }
            }
          : undefined
      }
      onMouseEnter={(e) => {
        if (isClickable && !isHighlighted) {
          e.currentTarget.style.background = "var(--bg-hover)";
        }
      }}
      onMouseLeave={(e) => {
        if (!isHighlighted) {
          e.currentTarget.style.background = "transparent";
        }
      }}
    >
      {/* Status indicator dot with color */}
      <div
        className="mt-0.5 flex-shrink-0 w-4 h-4 rounded-full flex items-center justify-center transition-shadow"
        style={{
          border: `1.5px solid ${statusStyle.borderColor}`,
          backgroundColor: statusStyle.backgroundColor,
          boxShadow: statusStyle.boxShadow,
        }}
      >
        <EventIcon
          eventType={event.eventType}
          style={{ color: statusStyle.borderColor }}
        />
      </div>

      {/* Event content */}
      <div className="flex-1 min-w-0">
        {/* Description as primary line */}
        <p
          className="truncate"
          style={{
            fontSize: "0.75rem",
            fontWeight: 400,
            color: "var(--text-primary)",
            lineHeight: 1.4,
          }}
        >
          {event.description}
        </p>

        {/* Timestamp and status badge row */}
        <div className="flex items-center gap-2 mt-0.5">
          <span
            title={formatFullTimestamp(event.timestamp)}
            style={{
              fontSize: "0.625rem",
              fontWeight: 500,
              color: "var(--text-muted)",
            }}
          >
            {formatRelativeTime(event.timestamp)}
          </span>

          {/* Status badge (for status_change events) */}
          {event.taskTitle && event.eventType === "status_change" && event.toStatus && (
            <span
              style={{
                display: "inline-flex",
                alignItems: "center",
                padding: "1px 5px",
                borderRadius: "4px",
                fontSize: "0.5625rem",
                fontWeight: 600,
                textTransform: "uppercase",
                letterSpacing: "0.02em",
                backgroundColor: statusStyle.backgroundColor,
                color: statusStyle.borderColor,
              }}
            >
              {event.toStatus.replace(/_/g, " ")}
            </span>
          )}
        </div>

        {/* Plan context (for plan-level events) */}
        {event.sessionTitle && event.eventType !== "status_change" && (
          <div
            className="flex items-center gap-1 mt-0.5"
            style={{
              fontSize: "0.625rem",
              color: "var(--text-muted)",
            }}
          >
            <GitMerge className="w-3 h-3" />
            <span className="truncate">{event.sessionTitle}</span>
          </div>
        )}
      </div>
    </div>
  );
});

export default TimelineEntry;
