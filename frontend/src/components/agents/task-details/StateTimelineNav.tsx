/**
 * StateTimelineNav - macOS Tahoe-inspired timeline navigation
 *
 * A beautiful horizontal timeline showing task state history.
 * Features:
 * - Vibrancy material background
 * - Smooth hover transitions
 * - Connected state dots with animated connectors
 * - Premium badge styling per status
 */

import { useMemo } from "react";
import { useTaskStateTransitions } from "@/hooks/useTaskStateTransitions";
import { Loader2, History, ChevronRight } from "lucide-react";
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import type { InternalStatus } from "@/types/task";
import { isTerminalStatus } from "@/types/status";
import {
  STATUS_TOKEN_REFS,
  statusTint,
  withAlpha,
  type StatusTokenKey,
} from "@/lib/theme-colors";

type TimelineStatusKey = StatusTokenKey | "muted";

// macOS Tahoe system colors (dark mode)
const STATUS_CONFIG: Record<
  InternalStatus,
  { label: string; color: TimelineStatusKey }
> = {
  backlog: { label: "Backlog", color: "muted" },
  ready: { label: "Ready", color: "info" },
  blocked: { label: "Blocked", color: "warning" },
  executing: { label: "Executing", color: "accent" },
  qa_refining: { label: "QA Refining", color: "accent" },
  qa_testing: { label: "QA Testing", color: "accent" },
  qa_passed: { label: "QA Passed", color: "success" },
  qa_failed: { label: "QA Failed", color: "error" },
  pending_review: { label: "Pending Review", color: "muted" },
  revision_needed: { label: "Revision Needed", color: "warning" },
  approved: { label: "Approved", color: "success" },
  failed: { label: "Failed", color: "error" },
  cancelled: { label: "Cancelled", color: "muted" },
  reviewing: { label: "Reviewing", color: "info" },
  review_passed: { label: "Review Passed", color: "success" },
  escalated: { label: "Escalated", color: "warning" },
  re_executing: { label: "Re-executing", color: "warning" },
  pending_merge: { label: "Pending Merge", color: "accent" },
  merging: { label: "Merging", color: "accent" },
  waiting_on_pr: { label: "Waiting on PR", color: "info" },
  merge_incomplete: { label: "Merge Incomplete", color: "warning" },
  merge_conflict: { label: "Merge Conflict", color: "warning" },
  merged: { label: "Merged", color: "success" },
  paused: { label: "Paused", color: "warning" },
  stopped: { label: "Stopped", color: "error" },
};

/**
 * Resolve a timeline status key to its CSS var reference. The "muted" key
 * returns `var(--text-muted)` since the Okabe palette does not carry a
 * dedicated neutral status tone.
 */
function resolveTimelineColor(color: TimelineStatusKey): string {
  return color === "muted" ? "var(--text-muted)" : STATUS_TOKEN_REFS[color];
}

/**
 * Resolve a timeline status key + alpha-percent into a translucent color
 * expression. Uses statusTint for known status keys and withAlpha-equivalent
 * color-mix for the muted neutral.
 */
function resolveTimelineTint(color: TimelineStatusKey, alpha: number): string {
  if (color === "muted") {
    return `color-mix(in srgb, var(--text-muted) ${alpha}%, transparent)`;
  }
  return statusTint(color, alpha);
}

interface TimelineEntry {
  status: InternalStatus;
  timestamp: string;
  isCurrent: boolean;
  /** Conversation ID from the state transition metadata (for states that spawn conversations) */
  conversationId?: string | undefined;
  /** Agent run ID from the state transition metadata */
  agentRunId?: string | undefined;
}

function formatRelativeTime(dateString: string): string {
  const diff = Date.now() - new Date(dateString).getTime();
  const mins = Math.floor(diff / 60000);
  if (mins < 1) return "Just now";
  if (mins < 60) return `${mins}m ago`;
  const hours = Math.floor(mins / 60);
  if (hours < 24) return `${hours}h ago`;
  const days = Math.floor(hours / 24);
  return `${days}d ago`;
}

// ============================================================================
// Sub-components
// ============================================================================

interface TimelineBadgeProps {
  entry: TimelineEntry;
  isSelected: boolean;
  onClick: () => void;
}

function TimelineBadge({ entry, isSelected, onClick }: TimelineBadgeProps) {
  const config = STATUS_CONFIG[entry.status];
  const isActive = isSelected || entry.isCurrent;
  const colorRef = resolveTimelineColor(config.color);
  const bgRef = resolveTimelineTint(config.color, 15);
  const glowInnerRef = resolveTimelineTint(config.color, 30);
  const glowOuterRef = resolveTimelineTint(config.color, 20);
  const dotGlowRef = resolveTimelineTint(config.color, 40);

  return (
    <Tooltip delayDuration={200}>
      <TooltipTrigger asChild>
        <button
          type="button"
          onClick={onClick}
          data-testid={`timeline-badge-${entry.status}`}
          data-status={entry.status}
          data-current={entry.isCurrent}
          data-selected={isSelected}
          className="group relative flex items-center gap-2 px-3 py-1.5 rounded-full transition-all duration-200"
          style={{
            backgroundColor: isActive ? bgRef : "transparent",
            boxShadow: isSelected
              ? `0 0 0 2px ${glowInnerRef}, 0 2px 8px ${glowOuterRef}`
              : undefined,
          }}
        >
          {/* Status dot */}
          <div
            className="relative w-2 h-2 rounded-full shrink-0 transition-transform duration-200 group-hover:scale-125"
            style={{
              backgroundColor: colorRef,
              boxShadow: isActive ? `0 0 8px ${dotGlowRef}` : undefined,
            }}
          >
            {/* Pulse ring for current */}
            {entry.isCurrent && (
              <div
                className="absolute inset-0 rounded-full animate-ping"
                style={{
                  backgroundColor: colorRef,
                  opacity: 0.4,
                }}
              />
            )}
          </div>

          {/* Label */}
          <span
            className="text-[0.6875rem] font-semibold tracking-tight transition-colors duration-200"
            style={{
              color: isActive ? colorRef : withAlpha("var(--text-primary)", 45),
            }}
          >
            {config.label}
          </span>
        </button>
      </TooltipTrigger>
      <TooltipContent
        side="bottom"
        sideOffset={8}
        className="px-3 py-1.5 text-[0.6875rem] font-medium rounded-lg"
        style={{
          backgroundColor: "var(--bg-elevated)",
          backdropFilter: "blur(20px)",
          WebkitBackdropFilter: "blur(20px)",
          border: "0.5px solid var(--overlay-moderate)",
          color: withAlpha("var(--text-primary)", 70),
          boxShadow: "0 4px 16px var(--overlay-scrim)",
        }}
      >
        <div className="flex items-center gap-2">
          <div
            className="w-1.5 h-1.5 rounded-full"
            style={{ backgroundColor: colorRef }}
          />
          <span>{formatRelativeTime(entry.timestamp)}</span>
          {entry.isCurrent && (
            <span
              className="px-1.5 py-0.5 rounded text-[0.5625rem] font-bold uppercase"
              style={{
                backgroundColor: bgRef,
                color: colorRef,
              }}
            >
              Current
            </span>
          )}
        </div>
      </TooltipContent>
    </Tooltip>
  );
}

interface TimelineConnectorProps {
  isActive: boolean;
  color: string;
}

function TimelineConnector({ isActive, color }: TimelineConnectorProps) {
  return (
    <div className="flex items-center px-0.5">
      <ChevronRight
        className="w-3.5 h-3.5 transition-colors duration-200"
        style={{
          color: isActive ? color : withAlpha("var(--text-primary)", 15),
        }}
      />
    </div>
  );
}

// ============================================================================
// Main Component
// ============================================================================

export interface StateTimelineNavProps {
  taskId: string;
  currentStatus: InternalStatus;
  onStateSelect: (state: {
    status: InternalStatus;
    timestamp: string;
    conversationId?: string | undefined;
    agentRunId?: string | undefined;
  } | null) => void;
  selectedState?: {
    status: InternalStatus;
    timestamp: string;
    conversationId?: string | undefined;
    agentRunId?: string | undefined;
  } | null;
}

export function StateTimelineNav({
  taskId,
  currentStatus,
  onStateSelect,
  selectedState,
}: StateTimelineNavProps) {
  const { data: transitions, isLoading, error } = useTaskStateTransitions(taskId);

  // Derive unique timeline entries from transitions (latest cycle per status)
  const timelineEntries = useMemo((): TimelineEntry[] => {
    // Transient states to skip in the timeline
    // - ready: brief transition between draft and executing
    // - pending_review: brief wait for AI reviewer
    // - pending_merge: brief programmatic merge attempt (1-3s)
    const transientStatuses: InternalStatus[] = [
      "ready",
      "pending_review",
      "pending_merge",
    ];

    // Intermediate retry/failure states that add noise once a task reaches
    // a terminal status (merged, failed, cancelled, stopped, approved).
    // These are still shown when they ARE the current status.
    const intermediateRetryStatuses: InternalStatus[] = [
      "merge_incomplete",
      "merge_conflict",
      "revision_needed",
      "qa_failed",
      "blocked",
      "paused",
    ];

    if (!transitions || transitions.length === 0) {
      // Don't show timeline for transient states with no history
      if (transientStatuses.includes(currentStatus)) {
        return [];
      }
      return [
        {
          status: currentStatus,
          timestamp: new Date().toISOString(),
          isCurrent: true,
        },
      ];
    }

    const entries: TimelineEntry[] = [];
    const seenStatuses = new Set<InternalStatus>();

    // Walk from newest to oldest so we keep the latest occurrence of each status.
    for (const transition of [...transitions].reverse()) {
      // Skip transient states - they're brief transitions not worth showing
      // (but keep current status visible)
      if (transientStatuses.includes(transition.toStatus) && transition.toStatus !== currentStatus) {
        continue;
      }
      // Skip intermediate retry/failure states once the task has reached a
      // terminal status — they add noise to the timeline. Still show them
      // when they ARE the current status (task is currently in that state).
      if (
        isTerminalStatus(currentStatus) &&
        intermediateRetryStatuses.includes(transition.toStatus) &&
        transition.toStatus !== currentStatus
      ) {
        continue;
      }
      if (seenStatuses.has(transition.toStatus)) {
        continue;
      }
      seenStatuses.add(transition.toStatus);

      entries.push({
        status: transition.toStatus,
        timestamp: transition.timestamp,
        isCurrent: transition.toStatus === currentStatus,
        conversationId: transition.conversationId,
        agentRunId: transition.agentRunId,
      });
    }

    entries.reverse();

    const lastEntry = entries[entries.length - 1];
    if (lastEntry && lastEntry.status !== currentStatus) {
      const existingEntry = entries.find((e) => e.status === currentStatus);
      if (existingEntry) {
        existingEntry.isCurrent = true;
      }
    }

    return entries;
  }, [transitions, currentStatus]);

  // Handle badge click
  const handleBadgeClick = (entry: TimelineEntry) => {
    if (entry.isCurrent) {
      onStateSelect(null);
    } else {
      onStateSelect({
        status: entry.status,
        timestamp: entry.timestamp,
        conversationId: entry.conversationId,
        agentRunId: entry.agentRunId,
      });
    }
  };

  // Loading state
  if (isLoading) {
    return (
      <div
        data-testid="timeline-loading"
        className="flex items-center gap-2.5 px-4 py-3 text-text-primary/40"
      >
        <Loader2 className="w-4 h-4 animate-spin" />
        <span className="text-[0.6875rem] font-medium">Loading history...</span>
      </div>
    );
  }

  // Error state
  if (error) {
    return (
      <div
        data-testid="timeline-error"
        className="flex items-center gap-2 px-4 py-3 text-[0.6875rem] font-medium"
        style={{ color: "var(--status-error)" }}
      >
        <History className="w-4 h-4" />
        <span>Failed to load history</span>
      </div>
    );
  }

  // Hide if single state
  if (timelineEntries.length <= 1) {
    return null;
  }

  // Get the color of the last active entry for connectors
  const selectedIndex = selectedState
    ? timelineEntries.findIndex(
        (e) =>
          e.status === selectedState.status &&
          e.timestamp === selectedState.timestamp
      )
    : -1;

  return (
    <TooltipProvider delayDuration={200}>
      <div
        data-testid="state-timeline-nav"
        className="flex items-center gap-1 px-4 py-3 overflow-x-auto"
        style={{
          backgroundColor: withAlpha("var(--bg-base)", 60),
          backdropFilter: "blur(40px) saturate(150%)",
          WebkitBackdropFilter: "blur(40px) saturate(150%)",
          borderBottom: "0.5px solid var(--overlay-weak)",
        }}
      >
        {/* History icon */}
        <div
          className="flex items-center gap-2 mr-2 pr-3"
          style={{ borderRight: "1px solid var(--border-subtle)" }}
        >
          <History className="w-4 h-4 text-text-primary/35" />
          <span className="text-[0.625rem] font-semibold uppercase tracking-wider text-text-primary/35">
            History
          </span>
        </div>

        {/* Timeline entries */}
        {timelineEntries.map((entry, index) => {
          const isConnectorActive =
            selectedState === null || (selectedIndex !== -1 && index < selectedIndex);
          const nextEntry = timelineEntries[index + 1];

          return (
            <div key={`${entry.status}-${entry.timestamp}`} className="flex items-center">
              <TimelineBadge
                entry={entry}
                isSelected={
                  selectedState?.status === entry.status &&
                  selectedState?.timestamp === entry.timestamp
                }
                onClick={() => handleBadgeClick(entry)}
              />
              {nextEntry && (
                <TimelineConnector
                  isActive={isConnectorActive}
                  color={resolveTimelineColor(STATUS_CONFIG[entry.status].color)}
                />
              )}
            </div>
          );
        })}

        {/* Viewing historical state indicator */}
        {selectedState && (
          <div
            className="ml-auto pl-3 flex items-center gap-2"
            style={{ borderLeft: "1px solid var(--border-subtle)" }}
          >
            <span className="text-[0.625rem] font-medium text-text-primary/40">
              Viewing past state
            </span>
            <button
              onClick={() => onStateSelect(null)}
              className="px-2 py-1 rounded-md text-[0.625rem] font-semibold transition-colors"
              style={{
                backgroundColor: "var(--accent-muted)",
                color: "var(--accent-primary)",
              }}
            >
              Back to Current
            </button>
          </div>
        )}
      </div>
    </TooltipProvider>
  );
}
