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

import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useTaskStateTransitions } from "@/hooks/useTaskStateTransitions";
import { Loader2, History, ChevronLeft, ChevronRight } from "lucide-react";
import {
  Tooltip,
  TooltipContent,
  TooltipProvider,
  TooltipTrigger,
} from "@/components/ui/tooltip";
import type { StateTransition } from "@/api/tasks";
import type { InternalStatus } from "@/types/task";
import type {
  TaskHistoryState,
  TaskRuntimeHistoryContextType,
} from "@/types/task-history";
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

type StageAttemptFamily = "execution" | "review" | "merge";

interface TimelineEntry extends TaskHistoryState {
  status: InternalStatus;
  timestamp: string;
  isCurrent: boolean;
  label: string;
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

interface TimelineScrollState {
  canScrollLeft: boolean;
  canScrollRight: boolean;
  hasOverflow: boolean;
}

function TimelineBadge({ entry, isSelected, onClick }: TimelineBadgeProps) {
  const config = STATUS_CONFIG[entry.status];
  const isActive = isSelected || entry.isCurrent;
  const colorRef = resolveTimelineColor(config.color);
  const bgRef = resolveTimelineTint(config.color, 15);
  const glowInnerRef = resolveTimelineTint(config.color, 30);
  const glowOuterRef = resolveTimelineTint(config.color, 20);
  const dotGlowRef = resolveTimelineTint(config.color, 40);
  const transcriptLabel = entry.hasConversation ? "Chat available" : "No chat";

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
          data-attempt-index={entry.attemptIndex}
          data-context-type={entry.contextType}
          data-has-conversation={entry.hasConversation}
          aria-label={`${entry.label}${entry.hasConversation ? ", transcript available" : ", no transcript recorded"}`}
          className="group relative flex min-h-[3rem] min-w-[8.5rem] shrink-0 items-start gap-2 rounded-lg px-3 py-2 text-left transition-all duration-200"
          style={{
            backgroundColor: isActive ? bgRef : "transparent",
            boxShadow: isSelected
              ? `0 0 0 2px ${glowInnerRef}, 0 2px 8px ${glowOuterRef}`
              : undefined,
          }}
        >
          {/* Status dot */}
          <div
            className="relative mt-1 w-2 h-2 rounded-full shrink-0 transition-transform duration-200 group-hover:scale-125"
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

          <span className="flex min-w-0 flex-col gap-0.5">
            <span
              data-testid="timeline-badge-label"
              className="text-[0.6875rem] font-semibold leading-tight tracking-tight transition-colors duration-200"
              style={{
                color: isActive ? colorRef : withAlpha("var(--text-primary)", 45),
              }}
            >
              {entry.label}
            </span>

            <span
              data-testid="timeline-badge-chat-meta"
              className="text-[0.5625rem] font-semibold uppercase leading-none"
              style={{
                color: entry.hasConversation
                  ? withAlpha(colorRef, 78)
                  : withAlpha("var(--text-primary)", 30),
              }}
            >
              {transcriptLabel}
            </span>
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
          <span>{entry.label}</span>
          <span>{formatRelativeTime(entry.timestamp)}</span>
          <span>{entry.hasConversation ? "Transcript available" : "No transcript recorded"}</span>
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

interface TimelineScrollButtonProps {
  direction: "left" | "right";
  disabled: boolean;
  onClick: () => void;
}

function TimelineScrollButton({
  direction,
  disabled,
  onClick,
}: TimelineScrollButtonProps) {
  const isLeft = direction === "left";
  const label = isLeft ? "Scroll history left" : "Scroll history right";
  const Icon = isLeft ? ChevronLeft : ChevronRight;

  return (
    <Tooltip delayDuration={200}>
      <TooltipTrigger asChild>
        <button
          type="button"
          aria-label={label}
          disabled={disabled}
          onClick={onClick}
          data-testid={`timeline-scroll-${direction}`}
          className="inline-flex h-7 w-7 shrink-0 items-center justify-center rounded-md transition-colors disabled:cursor-default disabled:opacity-35"
          style={{
            backgroundColor: disabled ? "transparent" : "var(--overlay-faint)",
            color: disabled
              ? withAlpha("var(--text-primary)", 28)
              : withAlpha("var(--text-primary)", 70),
          }}
        >
          <Icon className="h-3.5 w-3.5" />
        </button>
      </TooltipTrigger>
      <TooltipContent side="bottom" sideOffset={8}>
        {label}
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
  onStateSelect: (state: TaskHistoryState | null) => void;
  selectedState?: TaskHistoryState | null;
}

const TRANSIENT_STATUSES = new Set<InternalStatus>([
  "ready",
  "pending_review",
  "pending_merge",
]);

const INTERMEDIATE_RETRY_STATUSES = new Set<InternalStatus>([
  "merge_incomplete",
  "merge_conflict",
  "revision_needed",
  "qa_failed",
  "blocked",
  "paused",
]);

const EXECUTION_CONTEXT_STATUSES = new Set<InternalStatus>([
  "executing",
  "re_executing",
  "qa_refining",
  "qa_testing",
  "qa_passed",
  "qa_failed",
]);

const REVIEW_CONTEXT_STATUSES = new Set<InternalStatus>([
  "pending_review",
  "reviewing",
  "review_passed",
  "revision_needed",
  "approved",
  "escalated",
]);

const MERGE_CONTEXT_STATUSES = new Set<InternalStatus>([
  "pending_merge",
  "merging",
  "waiting_on_pr",
  "merge_incomplete",
  "merge_conflict",
  "merged",
]);

function deriveContextType(
  status: InternalStatus,
  explicitContextType?: TaskRuntimeHistoryContextType
): TaskRuntimeHistoryContextType | undefined {
  if (explicitContextType) {
    return explicitContextType;
  }
  if (EXECUTION_CONTEXT_STATUSES.has(status)) {
    return "task_execution";
  }
  if (REVIEW_CONTEXT_STATUSES.has(status)) {
    return "review";
  }
  if (MERGE_CONTEXT_STATUSES.has(status)) {
    return "merge";
  }
  return undefined;
}

function getAttemptFamily(status: InternalStatus): StageAttemptFamily | null {
  if (
    status === "executing" ||
    status === "re_executing" ||
    status === "qa_refining" ||
    status === "qa_testing" ||
    status === "qa_passed" ||
    status === "qa_failed"
  ) {
    return "execution";
  }
  if (status === "reviewing") {
    return "review";
  }
  if (
    status === "merging" ||
    status === "waiting_on_pr" ||
    status === "merge_incomplete" ||
    status === "merge_conflict" ||
    status === "merged"
  ) {
    return "merge";
  }
  return null;
}

function isAttemptStartStatus(status: InternalStatus): boolean {
  return (
    status === "executing" ||
    status === "re_executing" ||
    status === "reviewing" ||
    status === "merging"
  );
}

function formatAttemptLabel(
  family: StageAttemptFamily | null,
  attemptIndex: number | undefined,
  fallbackLabel: string
): string {
  if (!family || attemptIndex === undefined) {
    return fallbackLabel;
  }
  const familyLabel =
    family === "execution" ? "Execution" : family === "review" ? "Review" : "Merge";
  return `${familyLabel} attempt ${attemptIndex}`;
}

function shouldShowTransition(
  status: InternalStatus,
  currentStatus: InternalStatus
): boolean {
  if (TRANSIENT_STATUSES.has(status) && status !== currentStatus) {
    return false;
  }
  return !(
    isTerminalStatus(currentStatus) &&
    INTERMEDIATE_RETRY_STATUSES.has(status) &&
    status !== currentStatus
  );
}

function buildTimelineEntries(
  transitions: StateTransition[] | undefined,
  currentStatus: InternalStatus
): TimelineEntry[] {
  if (!transitions || transitions.length === 0) {
    if (TRANSIENT_STATUSES.has(currentStatus)) {
      return [];
    }
    const contextType = deriveContextType(currentStatus);
    const family = getAttemptFamily(currentStatus);
    const attemptIndex = family ? 1 : undefined;
    const timestamp = new Date().toISOString();
    return [
      {
        status: currentStatus,
        timestamp,
        isCurrent: true,
        label: formatAttemptLabel(
          family,
          attemptIndex,
          STATUS_CONFIG[currentStatus].label
        ),
        contextType,
        transitionId: `${currentStatus}-${timestamp}`,
        attemptIndex,
        hasConversation: false,
      },
    ];
  }

  const attemptCounts: Record<StageAttemptFamily, number> = {
    execution: 0,
    review: 0,
    merge: 0,
  };
  let activeAttemptFamily: StageAttemptFamily | null = null;

  const entries: TimelineEntry[] = [];
  for (const transition of transitions) {
    if (!shouldShowTransition(transition.toStatus, currentStatus)) {
      continue;
    }

    const family = getAttemptFamily(transition.toStatus);
    if (family) {
      if (
        activeAttemptFamily !== family ||
        isAttemptStartStatus(transition.toStatus) ||
        attemptCounts[family] === 0
      ) {
        attemptCounts[family] += 1;
      }
      activeAttemptFamily = family;
    } else {
      activeAttemptFamily = null;
    }

    const attemptIndex = family ? attemptCounts[family] : undefined;
    const contextType = deriveContextType(transition.toStatus, transition.contextType);
    const transitionId =
      transition.transitionId ?? `${transition.toStatus}-${transition.timestamp}`;
    const hasConversation = Boolean(transition.conversationId);
    const fallbackLabel = STATUS_CONFIG[transition.toStatus].label;

    entries.push({
      status: transition.toStatus,
      timestamp: transition.timestamp,
      isCurrent: false,
      label: formatAttemptLabel(family, attemptIndex, fallbackLabel),
      ...(transition.conversationId !== undefined && {
        conversationId: transition.conversationId,
      }),
      ...(transition.agentRunId !== undefined && { agentRunId: transition.agentRunId }),
      ...(contextType !== undefined && { contextType }),
      transitionId,
      ...(attemptIndex !== undefined && { attemptIndex }),
      hasConversation,
    });
  }

  let currentIndex = -1;
  for (let index = entries.length - 1; index >= 0; index -= 1) {
    if (entries[index]?.status === currentStatus) {
      currentIndex = index;
      break;
    }
  }

  if (currentIndex === -1 && !TRANSIENT_STATUSES.has(currentStatus)) {
    const timestamp = new Date().toISOString();
    const contextType = deriveContextType(currentStatus);
    const family = getAttemptFamily(currentStatus);
    const attemptIndex = family ? (attemptCounts[family] || 0) + 1 : undefined;
    entries.push({
      status: currentStatus,
      timestamp,
      isCurrent: true,
      label: formatAttemptLabel(
        family,
        attemptIndex,
        STATUS_CONFIG[currentStatus].label
      ),
      ...(contextType !== undefined && { contextType }),
      transitionId: `${currentStatus}-${timestamp}`,
      ...(attemptIndex !== undefined && { attemptIndex }),
      hasConversation: false,
    });
  } else if (currentIndex >= 0) {
    const currentEntry = entries[currentIndex];
    if (currentEntry) {
      entries[currentIndex] = {
        ...currentEntry,
        isCurrent: true,
      };
    }
  }

  return entries;
}

export function StateTimelineNav({
  taskId,
  currentStatus,
  onStateSelect,
  selectedState,
}: StateTimelineNavProps) {
  const { data: transitions, isLoading, error } = useTaskStateTransitions(taskId);
  const timelineViewportRef = useRef<HTMLDivElement | null>(null);
  const [scrollState, setScrollState] = useState<TimelineScrollState>({
    canScrollLeft: false,
    canScrollRight: false,
    hasOverflow: false,
  });

  const timelineEntries = useMemo((): TimelineEntry[] => {
    return buildTimelineEntries(transitions, currentStatus);
  }, [transitions, currentStatus]);

  const updateScrollState = useCallback(() => {
    const viewport = timelineViewportRef.current;
    if (!viewport) {
      return;
    }

    const maxScrollLeft = Math.max(0, viewport.scrollWidth - viewport.clientWidth);
    const nextState: TimelineScrollState = {
      hasOverflow: maxScrollLeft > 1,
      canScrollLeft: viewport.scrollLeft > 1,
      canScrollRight: viewport.scrollLeft < maxScrollLeft - 1,
    };

    setScrollState((previous) =>
      previous.hasOverflow === nextState.hasOverflow &&
      previous.canScrollLeft === nextState.canScrollLeft &&
      previous.canScrollRight === nextState.canScrollRight
        ? previous
        : nextState
    );
  }, []);

  useEffect(() => {
    updateScrollState();
    const viewport = timelineViewportRef.current;
    if (!viewport) {
      return;
    }

    if (typeof ResizeObserver === "undefined") {
      window.addEventListener("resize", updateScrollState);
      return () => window.removeEventListener("resize", updateScrollState);
    }

    const resizeObserver = new ResizeObserver(updateScrollState);
    resizeObserver.observe(viewport);
    return () => resizeObserver.disconnect();
  }, [timelineEntries.length, selectedState, updateScrollState]);

  const scrollTimeline = useCallback(
    (direction: "left" | "right") => {
      const viewport = timelineViewportRef.current;
      if (!viewport) {
        return;
      }
      const distance = Math.max(160, viewport.clientWidth * 0.7);
      viewport.scrollBy({
        left: direction === "left" ? -distance : distance,
        behavior: "smooth",
      });
      window.setTimeout(updateScrollState, 250);
    },
    [updateScrollState]
  );

  // Handle badge click
  const handleBadgeClick = (entry: TimelineEntry) => {
    if (entry.isCurrent) {
      onStateSelect(null);
    } else {
      onStateSelect({
        status: entry.status,
        timestamp: entry.timestamp,
        ...(entry.conversationId !== undefined && {
          conversationId: entry.conversationId,
        }),
        ...(entry.agentRunId !== undefined && { agentRunId: entry.agentRunId }),
        ...(entry.contextType !== undefined && { contextType: entry.contextType }),
        ...(entry.transitionId !== undefined && { transitionId: entry.transitionId }),
        ...(entry.attemptIndex !== undefined && { attemptIndex: entry.attemptIndex }),
        ...(entry.hasConversation !== undefined && {
          hasConversation: entry.hasConversation,
        }),
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
          (selectedState.transitionId &&
            e.transitionId === selectedState.transitionId) ||
          (e.status === selectedState.status &&
            e.timestamp === selectedState.timestamp)
      )
    : -1;

  return (
    <TooltipProvider delayDuration={200}>
      <div
        data-testid="state-timeline-nav"
        className="flex min-w-0 items-center gap-2 px-4 py-3"
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

        {scrollState.hasOverflow && (
          <TimelineScrollButton
            direction="left"
            disabled={!scrollState.canScrollLeft}
            onClick={() => scrollTimeline("left")}
          />
        )}

        {/* Timeline entries */}
        <div
          ref={timelineViewportRef}
          data-testid="timeline-scroll-viewport"
          className="flex min-w-0 flex-1 items-center gap-1 overflow-x-auto overscroll-x-contain"
          onScroll={updateScrollState}
        >
          {timelineEntries.map((entry, index) => {
            const isConnectorActive =
              selectedState === null || (selectedIndex !== -1 && index < selectedIndex);
            const nextEntry = timelineEntries[index + 1];

            return (
              <div
                key={`${entry.status}-${entry.timestamp}`}
                className="flex shrink-0 items-center"
              >
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
        </div>

        {scrollState.hasOverflow && (
          <TimelineScrollButton
            direction="right"
            disabled={!scrollState.canScrollRight}
            onClick={() => scrollTimeline("right")}
          />
        )}

        {/* Viewing historical state indicator */}
        {selectedState && (
          <div
            className="shrink-0 pl-3 flex items-center gap-2"
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
