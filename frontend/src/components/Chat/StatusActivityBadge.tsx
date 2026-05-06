/**
 * StatusActivityBadge - Unified status badge with activity navigation
 *
 * Replaces both the Badge + WorkerExecutingIndicator with a single component
 * that shows agent status and provides one-click navigation to Activity view
 * with context filter set.
 *
 * Behavior by state:
 * - Idle, no activity: Hidden
 * - Idle, has activity: Muted Activity icon (clickable)
 * - Agent active: Badge with status + spinner + Activity icon
 */

import { useState, useEffect } from "react";
import { Activity, Loader2, CirclePause } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { useUiStore } from "@/stores/uiStore";
import { useChatStore, selectToolCallStartTimes, selectLastToolCallCompletionTimestamp } from "@/stores/chatStore";
import { useIdeationStore } from "@/stores/ideationStore";
import { AGENT_MERGER, AGENT_WORKER, AGENT_REVIEWER } from "@/constants/agents";
import type { ContextType, ModelDisplay } from "@/types/chat-conversation";
import type { AgentStatus } from "@/stores/chatStore";
import { ModelChip } from "./ModelChip";
import { useFeatureFlags } from "@/hooks/useFeatureFlags";

// ============================================================================
// Constants
// ============================================================================

/** Grace period after last tool completion before "Tool active" label clears (ms) */
const TOOL_CALL_GRACE_MS = 5_000;

/** Inactivity threshold before "Completing..." label appears during deferral window (ms) */
const POST_STREAM_THRESHOLD_MS = 3_000;

// ============================================================================
// Types
// ============================================================================

export type AgentType =
  | typeof AGENT_WORKER
  | typeof AGENT_REVIEWER
  | typeof AGENT_MERGER
  | "agent"
  | "idle";

export interface StatusActivityBadgeProps {
  /** Whether an agent is currently active (running/sending) */
  isAgentActive: boolean;
  /** Type of agent that is active */
  agentType: AgentType;
  /** Context type for the agent conversation (used for label and activity navigation) */
  contextType: ContextType;
  /** Context ID (taskId or sessionId) for activity filtering */
  contextId: string | null;
  /** Whether there is historical activity available for this context */
  hasActivity?: boolean;
  /** Tri-state agent status for nuanced display */
  agentStatus?: AgentStatus;
  /** Store key for subscribing to lastAgentEventTimestamp */
  storeKey?: string;
  /** Effective model to display as chip on the left of the status row */
  modelDisplay?: ModelDisplay;
  /** Hide model chip when the parent renders model context separately */
  hideModelChip?: boolean;
  /** Compact inline rendering for stacked header layouts */
  layout?: "badge" | "inline";
}

// ============================================================================
// Helper Functions
// ============================================================================

function getStatusText(agentType: AgentType): string {
  switch (agentType) {
    case AGENT_WORKER:
      return "Worker running...";
    case AGENT_REVIEWER:
      return "Reviewing...";
    case AGENT_MERGER:
      return "Merger running...";
    case "agent":
      return "Agent responding...";
    default:
      return "Working...";
  }
}

/** Format elapsed seconds as human-readable string */
function formatElapsed(seconds: number): string {
  if (seconds < 60) return `${seconds}s ago`;
  const minutes = Math.floor(seconds / 60);
  const secs = seconds % 60;
  if (secs === 0) return `${minutes}m ago`;
  return `${minutes}m ${secs}s ago`;
}

/** Get color class for elapsed time */
function getElapsedColor(seconds: number): string {
  if (seconds < 60) return "text-status-success";
  if (seconds < 180) return "text-status-warning";
  return "text-status-error";
}

// ============================================================================
// Last Activity Display
// ============================================================================

interface LastActivityProps {
  lastEventTimestamp: number;
}

function LastActivity({ lastEventTimestamp }: LastActivityProps) {
  const [elapsed, setElapsed] = useState(() =>
    Math.floor((Date.now() - lastEventTimestamp) / 1000)
  );

  useEffect(() => {
    const interval = setInterval(() => {
      setElapsed(Math.floor((Date.now() - lastEventTimestamp) / 1000));
    }, 1000);
    return () => clearInterval(interval);
  }, [lastEventTimestamp]);

  const colorClass = getElapsedColor(elapsed);

  return (
    <span className={`text-xs ${colorClass} shrink-0`}>
      Last: {formatElapsed(elapsed)}
    </span>
  );
}

// ============================================================================
// Component
// ============================================================================

export function StatusActivityBadge({
  isAgentActive,
  agentType,
  contextType,
  contextId,
  hasActivity = false,
  agentStatus = "idle",
  storeKey,
  modelDisplay,
  hideModelChip = false,
  layout = "badge",
}: StatusActivityBadgeProps) {
  const { data: featureFlags } = useFeatureFlags();
  const setActivityFilter = useUiStore((s) => s.setActivityFilter);
  const setCurrentView = useUiStore((s) => s.setCurrentView);
  const lastEventTimestamp = useChatStore((s) =>
    storeKey ? (s.lastAgentEventTimestamp[storeKey] ?? 0) : 0
  );
  const toolCallStartTimes = useChatStore(selectToolCallStartTimes(storeKey ?? ""));
  const lastToolCallCompletionTimestamp = useChatStore(
    selectLastToolCallCompletionTimestamp(storeKey ?? "")
  );

  // Derive sessionId for verification child lookup (storeKey format: "session:{sessionId}")
  const sessionId =
    storeKey?.startsWith("session:") ? storeKey.slice(8) : null;
  const activeVerificationChildId = useIdeationStore((s) =>
    sessionId ? (s.activeVerificationChildId[sessionId] ?? null) : null
  );

  // Track current time for grace period calculation — avoids calling Date.now() during render
  const [now, setNow] = useState(Date.now);
  useEffect(() => {
    if (lastToolCallCompletionTimestamp <= 0) return;
    const remaining = TOOL_CALL_GRACE_MS - (Date.now() - lastToolCallCompletionTimestamp);
    if (remaining <= 0) {
      setNow(Date.now());
      return;
    }
    const timer = setTimeout(() => { setNow(Date.now()); }, remaining);
    return () => { clearTimeout(timer); };
  }, [lastToolCallCompletionTimestamp]);

  // Trigger re-render when post-stream threshold is reached so isPostStreamWork updates
  useEffect(() => {
    if (agentStatus !== "generating" || lastEventTimestamp <= 0) return;
    const elapsed = Date.now() - lastEventTimestamp;
    const remaining = POST_STREAM_THRESHOLD_MS - elapsed;
    if (remaining <= 0) {
      setNow(Date.now());
      return;
    }
    const timer = setTimeout(() => { setNow(Date.now()); }, remaining);
    return () => { clearTimeout(timer); };
  }, [agentStatus, lastEventTimestamp]);

  // Navigate to activity view with context filter
  const handleActivityClick = () => {
    // Set filter based on context type
    if (contextType === "ideation") {
      setActivityFilter({ sessionId: contextId, taskId: null });
    } else {
      // task, task_detail, kanban with selected task
      setActivityFilter({ taskId: contextId, sessionId: null });
    }
    setCurrentView("activity");
  };

  const isWaiting = agentStatus === "waiting_for_input";
  const showActivityButton = featureFlags.activityPage;
  const showModelChip = Boolean(modelDisplay) && !hideModelChip;
  const isInlineLayout = layout === "inline";

  // Hidden state: idle with no activity
  if (!isAgentActive && !isWaiting && !hasActivity) {
    return null;
  }

  // Idle but has activity: show muted activity icon
  if (!isAgentActive && !isWaiting && hasActivity) {
    return (
      <div className="flex items-center gap-1.5 shrink-0">
        {showModelChip && modelDisplay && <ModelChip model={modelDisplay} />}
        {showActivityButton && (
          <Button
            variant="ghost"
            size="sm"
            onClick={handleActivityClick}
            className="shrink-0 h-7 px-2 text-text-primary/40 hover:text-text-primary/60"
            aria-label="View activity"
          >
            <Activity className="w-3.5 h-3.5" />
          </Button>
        )}
      </div>
    );
  }

  // Waiting for input: show subtle badge without spinner
  if (isWaiting) {
    if (isInlineLayout) {
      return (
        <div className="flex items-center gap-1.5 min-w-0 text-[0.6875rem] text-text-primary/55">
          <CirclePause className="h-3 w-3 shrink-0 text-text-primary/45" />
          <span className="truncate">Awaiting input</span>
          {showActivityButton && (
            <Button
              variant="ghost"
              size="sm"
              onClick={handleActivityClick}
              className="ml-1 shrink-0 h-6 px-1.5 text-[var(--accent-primary)] hover:text-[var(--accent-primary)]/80"
              aria-label="View activity"
            >
              <Activity className="w-3.5 h-3.5" />
            </Button>
          )}
        </div>
      );
    }

    return (
      <div className="flex items-center gap-1.5 shrink-0">
        {showModelChip && modelDisplay && <ModelChip model={modelDisplay} />}
        <Badge variant="secondary" className="shrink-0">
          <CirclePause className="w-3 h-3 mr-1.5 text-text-primary/50" />
          Awaiting input
        </Badge>
        {showActivityButton && (
          <Button
            variant="ghost"
            size="sm"
            onClick={handleActivityClick}
            className="shrink-0 h-7 px-2 text-[var(--accent-primary)] hover:text-[var(--accent-primary)]/80"
            aria-label="View activity"
          >
            <Activity className="w-3.5 h-3.5" />
          </Button>
        )}
      </div>
    );
  }

  // Determine active tool state and grace period
  const hasActiveToolCalls = Object.keys(toolCallStartTimes).length > 0;
  const isInGracePeriod =
    !hasActiveToolCalls &&
    lastToolCallCompletionTimestamp > 0 &&
    now - lastToolCallCompletionTimestamp < TOOL_CALL_GRACE_MS;
  const showToolActive = hasActiveToolCalls || isInGracePeriod;

  // True when generating but no new agent events in >3s (deferral window post-stream work)
  const isPostStreamWork =
    agentStatus === "generating" &&
    lastEventTimestamp > 0 &&
    now - lastEventTimestamp > POST_STREAM_THRESHOLD_MS;

  // Determine badge label and color for generating state
  let badgeLabel: string;
  let badgeColorClass: string;
  if (showToolActive) {
    badgeLabel = "Tool active";
    badgeColorClass = "text-status-success";
  } else if (activeVerificationChildId) {
    badgeLabel = "Verifying...";
    badgeColorClass = "text-status-info";
  } else if (isPostStreamWork && (contextType === "merge" || contextType === "review")) {
    badgeLabel = `Completing ${contextType}...`;
    badgeColorClass = "";
  } else {
    badgeLabel = getStatusText(agentType);
    badgeColorClass = "";
  }

  if (isInlineLayout) {
    return (
      <div
        className="flex items-center gap-1.5 min-w-0 text-[0.6875rem] text-text-primary/55"
        data-testid="chat-session-status-inline"
      >
        <Loader2 className="h-3 w-3 shrink-0 animate-spin text-text-primary/55" />
        <span className={badgeColorClass ? `${badgeColorClass} truncate` : "truncate"}>
          {badgeLabel}
        </span>
        {storeKey && lastEventTimestamp > 0 && !showToolActive && !activeVerificationChildId && (
          <LastActivity lastEventTimestamp={lastEventTimestamp} />
        )}
        {showActivityButton && (
          <Button
            variant="ghost"
            size="sm"
            onClick={handleActivityClick}
            className="ml-1 shrink-0 h-6 px-1.5 text-[var(--accent-primary)] hover:text-[var(--accent-primary)]/80"
            aria-label="View activity"
          >
            <Activity className="w-3.5 h-3.5" />
          </Button>
        )}
      </div>
    );
  }

  // Active/generating state: badge with status text, spinner, last activity, and activity button
  return (
    <div className="flex items-center gap-1.5 shrink-0">
      {showModelChip && modelDisplay && <ModelChip model={modelDisplay} />}
      <Badge variant="secondary" className="shrink-0">
        <Loader2 className="w-3 h-3 mr-1.5 animate-spin" />
        <span className={badgeColorClass || undefined}>{badgeLabel}</span>
      </Badge>
      {storeKey && lastEventTimestamp > 0 && !showToolActive && !activeVerificationChildId && (
        <LastActivity lastEventTimestamp={lastEventTimestamp} />
      )}
      {showActivityButton && (
        <Button
          variant="ghost"
          size="sm"
          onClick={handleActivityClick}
          className="shrink-0 h-7 px-2 text-[var(--accent-primary)] hover:text-[var(--accent-primary)]/80"
          aria-label="View activity"
        >
          <Activity className="w-3.5 h-3.5" />
        </Button>
      )}
    </div>
  );
}
