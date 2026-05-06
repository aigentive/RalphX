/**
 * StreamingToolIndicator - Live tool call display during agent execution
 *
 * Shows a single, always-expanded card that displays a chain-of-thought
 * built from incoming tool calls. Each tool call is shown as a summary line
 * with the tool name and key argument values extracted dynamically.
 *
 * Features:
 * - Max height with scrollable content
 * - Auto-scrolls to show latest tool call
 * - Filters out result tools (result:toolu*) which don't show useful info
 *
 * This is used ONLY during streaming - final messages use ToolCallIndicator.
 */

import { useMemo, useRef, useEffect, useCallback, useState } from "react";
import { Wrench, Loader2, ChevronDown, ChevronRight } from "lucide-react";
import type { ToolCall } from "./ToolCallIndicator";
import { formatDuration } from "@/components/tasks/detail-views/shared/DurationDisplay";

// Maximum height for the tool list content area (~20% reduced from original 200)
const MAX_CONTENT_HEIGHT = 160;

/** Distance from bottom (px) within which we consider the user "near bottom" */
const NEAR_BOTTOM_THRESHOLD = 30;

// ============================================================================
// Types
// ============================================================================

interface StreamingToolIndicatorProps {
  /** Tool calls accumulated during streaming */
  toolCalls: ToolCall[];
  /** Whether the agent is still running */
  isActive?: boolean;
  /** Start timestamps per tool call ID (toolCallId → epoch ms) — for elapsed timer display */
  toolCallStartTimes?: Record<string, number>;
}

// ============================================================================
// Helpers
// ============================================================================

/**
 * Extract a readable summary from tool arguments
 * Only uses first-level keys that are not nested objects
 */
function extractArgumentSummary(args: unknown): string[] {
  if (!args || typeof args !== "object" || Array.isArray(args)) {
    return [];
  }

  const summaries: string[] = [];
  const entries = Object.entries(args as Record<string, unknown>);

  for (const [key, value] of entries) {
    // Skip nested objects and arrays
    if (value && typeof value === "object") {
      continue;
    }

    // Skip null/undefined
    if (value == null) {
      continue;
    }

    // Format the value
    const strValue = String(value);
    const truncated = strValue.length > 60 ? strValue.slice(0, 57) + "..." : strValue;

    // Format the key (snake_case to readable)
    const readableKey = key.replace(/_/g, " ");

    summaries.push(`${readableKey}: ${truncated}`);
  }

  return summaries.slice(0, 3); // Max 3 key-value pairs
}

/**
 * Create a one-line summary for a tool call
 */
function createToolSummary(toolCall: ToolCall): { primary: string; details: string[] } {
  const { name, arguments: args } = toolCall;
  // Normalize tool name to lowercase for matching
  const normalizedName = name.toLowerCase();

  // Known tool formats with primary info extraction
  switch (normalizedName) {
    case "bash": {
      const typedArgs = args as { command?: string; description?: string } | undefined;
      const desc = typedArgs?.description;
      const cmd = typedArgs?.command;
      return {
        primary: desc || (cmd ? `$ ${cmd.slice(0, 50)}${cmd.length > 50 ? "..." : ""}` : "Running command"),
        details: desc && cmd ? [`$ ${cmd.slice(0, 60)}${cmd.length > 60 ? "..." : ""}`] : [],
      };
    }
    case "read": {
      const typedArgs = args as { file_path?: string } | undefined;
      return {
        primary: typedArgs?.file_path || "Reading file",
        details: [],
      };
    }
    case "write": {
      const typedArgs = args as { file_path?: string } | undefined;
      return {
        primary: typedArgs?.file_path || "Writing file",
        details: [],
      };
    }
    case "edit": {
      const typedArgs = args as { file_path?: string } | undefined;
      return {
        primary: typedArgs?.file_path || "Editing file",
        details: [],
      };
    }
    case "glob": {
      const typedArgs = args as { pattern?: string; path?: string } | undefined;
      return {
        primary: typedArgs?.pattern || "Searching files",
        details: typedArgs?.path ? [`in ${typedArgs.path}`] : [],
      };
    }
    case "grep": {
      const typedArgs = args as { pattern?: string; path?: string } | undefined;
      return {
        primary: typedArgs?.pattern ? `"${typedArgs.pattern}"` : "Searching content",
        details: typedArgs?.path ? [`in ${typedArgs.path}`] : [],
      };
    }
    case "create_task_proposal": {
      const typedArgs = args as { title?: string } | undefined;
      return {
        primary: typedArgs?.title || "Creating proposal",
        details: [],
      };
    }
    case "update_task_proposal": {
      const typedArgs = args as { proposal_id?: string; title?: string } | undefined;
      return {
        primary: typedArgs?.title || `Updating ${typedArgs?.proposal_id?.slice(0, 8) || "proposal"}`,
        details: [],
      };
    }
    case "get_task_context": {
      return {
        primary: "Fetching task context",
        details: [],
      };
    }
    default: {
      // Dynamic extraction for unknown tools
      const details = extractArgumentSummary(args);
      return {
        primary: name.replace(/_/g, " "),
        details,
      };
    }
  }
}

/**
 * Get a verb for the tool action
 */
function getToolVerb(name: string): string {
  const normalizedName = name.toLowerCase();
  switch (normalizedName) {
    case "bash":
      return "Running";
    case "read":
      return "Reading";
    case "write":
      return "Writing";
    case "edit":
      return "Editing";
    case "glob":
      return "Finding";
    case "grep":
      return "Searching";
    case "create_task_proposal":
      return "Creating";
    case "update_task_proposal":
      return "Updating";
    case "delete_task_proposal":
      return "Deleting";
    case "get_task_context":
    case "get_artifact":
      return "Fetching";
    default:
      return "Calling";
  }
}

// ============================================================================
// Component
// ============================================================================

export function StreamingToolIndicator({
  toolCalls,
  isActive = true,
  toolCallStartTimes,
}: StreamingToolIndicatorProps) {
  const contentRef = useRef<HTMLDivElement>(null);
  const isNearBottomRef = useRef(true);
  const [isExpanded, setIsExpanded] = useState(false);
  const [elapsedSeconds, setElapsedSeconds] = useState(0);

  // Track scroll position to determine if user is near bottom
  const handleScroll = useCallback(() => {
    const el = contentRef.current;
    if (!el) return;
    const distanceFromBottom = el.scrollHeight - el.scrollTop - el.clientHeight;
    isNearBottomRef.current = distanceFromBottom <= NEAR_BOTTOM_THRESHOLD;
  }, []);

  // Find the last active (no result) non-result tool call for elapsed timer
  const lastActiveToolCallId = useMemo(() => {
    const active = toolCalls.filter(
      (tc) => !tc.name.startsWith("result:toolu") && tc.result === undefined
    );
    return active[active.length - 1]?.id ?? null;
  }, [toolCalls]);

  const activeStartTime = lastActiveToolCallId
    ? (toolCallStartTimes?.[lastActiveToolCallId] ?? null)
    : null;

  // Live elapsed timer — 1-second interval when a tool call is active
  useEffect(() => {
    if (!activeStartTime || !isActive) {
      setElapsedSeconds(0);
      return;
    }
    const update = () =>
      setElapsedSeconds(Math.floor((Date.now() - activeStartTime) / 1000));
    update();
    const id = setInterval(update, 1000);
    return () => clearInterval(id);
  }, [activeStartTime, isActive]);

  // Filter out result tools (result:toolu*) and process into summary lines
  const summaryLines = useMemo(() => {
    return toolCalls
      .filter((tc) => !tc.name.startsWith("result:toolu"))
      .map((tc) => {
        const { primary, details } = createToolSummary(tc);
        const verb = getToolVerb(tc.name);
        return {
          id: tc.id,
          name: tc.name,
          verb,
          primary,
          details,
          hasError: Boolean(tc.error),
          isActiveCall: tc.id === lastActiveToolCallId,
        };
      });
  }, [toolCalls, lastActiveToolCallId]);

  // Auto-scroll to bottom when new tool calls arrive (only if user is near bottom and expanded)
  useEffect(() => {
    if (isExpanded && contentRef.current && isNearBottomRef.current) {
      contentRef.current.scrollTop = contentRef.current.scrollHeight;
    }
  }, [summaryLines.length, isExpanded]);

  if (summaryLines.length === 0) {
    return null;
  }

  return (
    <div
      data-testid="streaming-tool-indicator"
      className="rounded-lg overflow-hidden mb-2"
      style={{
        backgroundColor: "var(--bg-elevated)",
        border: "1px solid var(--border-subtle)",
      }}
    >
      {/* Header — clickable to toggle expand/collapse */}
      <button
        type="button"
        onClick={() => setIsExpanded((prev) => !prev)}
        className="flex items-center gap-2 px-3 py-2 w-full text-left hover:opacity-80 transition-opacity"
        style={{
          borderBottom: isExpanded ? "1px solid var(--border-subtle)" : "none",
        }}
        aria-expanded={isExpanded}
        aria-label={`${isActive ? "Working" : "Tool calls"}. ${summaryLines.length} tool call${summaryLines.length !== 1 ? "s" : ""}. Click to ${isExpanded ? "collapse" : "expand"}.`}
      >
        {isActive ? (
          <Loader2
            size={14}
            className="animate-spin flex-shrink-0"
            style={{ color: "var(--accent-primary)" }}
          />
        ) : (
          <Wrench
            size={14}
            className="flex-shrink-0"
            style={{ color: "var(--accent-primary)" }}
          />
        )}
        <span
          className="text-xs font-medium flex-1"
          style={{ color: "var(--text-secondary)" }}
        >
          {isActive ? "Working..." : "Done"}{" "}
          <span style={{ color: "var(--text-muted)" }}>
            ({summaryLines.length} tool call{summaryLines.length !== 1 ? "s" : ""})
          </span>
        </span>
        {isExpanded ? (
          <ChevronDown size={14} className="flex-shrink-0" style={{ color: "var(--text-muted)" }} />
        ) : (
          <ChevronRight size={14} className="flex-shrink-0" style={{ color: "var(--text-muted)" }} />
        )}
      </button>

      {/* Chain of thought - tool call summaries (scrollable, only when expanded) */}
      {isExpanded && (
        <div
          ref={contentRef}
          onScroll={handleScroll}
          className="px-3 py-2 space-y-1.5 overflow-y-auto"
          style={{ maxHeight: `${MAX_CONTENT_HEIGHT}px`, overscrollBehavior: "contain" }}
        >
          {summaryLines.map((line, index) => (
            <div
              key={line.id || index}
              className="text-xs"
              style={{
                color: line.hasError ? "var(--status-error)" : "var(--text-secondary)",
              }}
            >
              {/* Line 1: Step number + Verb */}
              <div className="flex items-start gap-2">
                <span
                  className="flex-shrink-0 w-4 text-right tabular-nums"
                  style={{ color: "var(--text-muted)" }}
                >
                  {index + 1}.
                </span>
                <span className="font-medium" style={{ color: "var(--text-primary)" }}>
                  {line.verb}
                </span>
              </div>

              {/* Line 2: File path (indented, monospace, break-all) */}
              <div className="flex gap-2">
                <span className="w-4 flex-shrink-0" />
                <span
                  className="font-mono text-[0.6875rem] break-all"
                  style={{
                    color: line.hasError ? "var(--status-error)" : "var(--text-secondary)",
                  }}
                >
                  {line.primary}
                </span>
              </div>

              {/* Additional details */}
              {line.details.length > 0 && (
                <div className="flex gap-2">
                  <span className="w-4 flex-shrink-0" />
                  <div
                    className="text-[0.6875rem] font-mono"
                    style={{ color: "var(--text-muted)" }}
                  >
                    {line.details.map((detail, i) => (
                      <div key={i} className="truncate">
                        {detail}
                      </div>
                    ))}
                  </div>
                </div>
              )}

              {/* Elapsed timer — shown only for the currently active tool call */}
              {isActive && line.isActiveCall && elapsedSeconds > 0 && (
                <div
                  data-testid="elapsed-timer"
                  className="flex gap-2"
                >
                  <span className="w-4 flex-shrink-0" />
                  <span
                    className="text-[0.6875rem] tabular-nums"
                    style={{ color: "var(--text-muted)" }}
                  >
                    Running {formatDuration(elapsedSeconds)}
                  </span>
                </div>
              )}
            </div>
          ))}

          {/* Active indicator at the end */}
          {isActive && (
            <div className="flex items-center gap-2 text-xs pt-1">
              <span className="w-4" />
              <div className="flex items-center gap-1">
                <div
                  className="w-1.5 h-1.5 rounded-full animate-pulse"
                  style={{ backgroundColor: "var(--accent-primary)" }}
                />
                <div
                  className="w-1.5 h-1.5 rounded-full animate-pulse"
                  style={{ backgroundColor: "var(--accent-primary)", animationDelay: "0.15s" }}
                />
                <div
                  className="w-1.5 h-1.5 rounded-full animate-pulse"
                  style={{ backgroundColor: "var(--accent-primary)", animationDelay: "0.3s" }}
                />
              </div>
            </div>
          )}
        </div>
      )}
    </div>
  );
}
