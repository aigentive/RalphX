/**
 * ToolCallIndicator - Displays tool calls made by Claude during chat (final render)
 *
 * Features:
 * - Collapsible view (summary by default, expand for raw data)
 * - Shows tool name and smart summary extracted from arguments
 * - Expands to show formatted JSON (no nested collapse)
 * - Compact, readable design
 */

import React, { Suspense, useState, useMemo } from "react";
import { Wrench, ChevronDown, ChevronRight, FileText, Terminal, FileEdit, Search, FolderSearch, Loader2 } from "lucide-react";
import { createSummary, formatValue, getToolVerb } from "./ToolCallIndicator.helpers";
import { isDiffToolCall, isTaskToolCall } from "./DiffToolCallView.utils";
import { DiffToolCallView } from "./DiffToolCallView";
import { TaskToolCallCard } from "./TaskToolCallCard";
import { getToolCallWidget } from "./tool-widgets/registry";
import { ToolCallPreviewCard } from "./ToolCallPreviewCard";

// Re-export ToolCall from canonical location for backwards compatibility
export type { ToolCall } from "./tool-widgets/shared.constants";
import type { ToolCall } from "./tool-widgets/shared.constants";

interface ToolCallIndicatorProps {
  /** The tool call to display */
  toolCall: ToolCall;
  /** Optional additional className for container */
  className?: string;
  /** Compact mode for rendering inside task cards — smaller padding, text, icons */
  compact?: boolean;
  /** Streaming mode — tool call is still in progress (no result yet). Shows loading spinner. */
  isStreaming?: boolean;
}

// ============================================================================
// Component
// ============================================================================

/**
 * Render tool icon based on tool name
 */
function ToolIcon({ name, hasError, size = 14 }: { name: string; hasError: boolean; size?: number }) {
  /* macOS Tahoe: flat colors */
  const style = { color: hasError ? "var(--status-error)" : "var(--accent-primary)" };
  const className = "flex-shrink-0";

  switch (name) {
    case "bash":
      return <Terminal size={size} className={className} style={style} />;
    case "read":
    case "write":
      return <FileText size={size} className={className} style={style} />;
    case "edit":
      return <FileEdit size={size} className={className} style={style} />;
    case "glob":
      return <FolderSearch size={size} className={className} style={style} />;
    case "grep":
      return <Search size={size} className={className} style={style} />;
    default:
      return <Wrench size={size} className={className} style={style} />;
  }
}

export const ToolCallIndicator = React.memo(function ToolCallIndicator({ toolCall, className = "", compact = false, isStreaming = false }: ToolCallIndicatorProps) {
  // Hooks must be called unconditionally (React rules-of-hooks)
  const [isExpanded, setIsExpanded] = useState(false);
  const summary = useMemo(() => createSummary(toolCall), [toolCall]);
  const verb = useMemo(() => getToolVerb(toolCall.name), [toolCall.name]);
  const hasError = Boolean(toolCall.error);

  if (toolCall.resultPreviewTruncated) {
    return (
      <ToolCallPreviewCard
        toolCall={toolCall}
        className={className}
        compact={compact}
        isStreaming={isStreaming}
      />
    );
  }

  // Delegate Edit/Write to DiffToolCallView for inline diff rendering
  // Quick check: arguments must have file_path for diff to work (same gate as DiffToolCallView)
  if (isDiffToolCall(toolCall.name)) {
    const args = toolCall.arguments;
    const hasFilePath = args != null && typeof args === "object" && typeof (args as Record<string, unknown>).file_path === "string" && (args as Record<string, unknown>).file_path !== "";
    if (hasFilePath && !hasError) {
      return <DiffToolCallView toolCall={toolCall} className={className} compact={compact} />;
    }
  }

  // Delegate Task tool calls to TaskToolCallCard for subagent rendering (never compact — tasks don't nest)
  if (isTaskToolCall(toolCall.name)) {
    return <TaskToolCallCard toolCall={toolCall} className={className} />;
  }

  // Check widget registry for specialized renderers
  const SpecializedWidget = getToolCallWidget(toolCall.name);
  if (SpecializedWidget) {
    return (
      <Suspense fallback={null}>
        {React.createElement(SpecializedWidget, { toolCall, compact, className })}
      </Suspense>
    );
  }

  const iconSize = compact ? 12 : 14;
  const chevronSize = compact ? 12 : 14;

  return (
    <div
      data-testid="tool-call-indicator"
      className={`${compact ? "rounded-md" : "rounded-lg"} overflow-hidden max-w-full ${compact ? "mb-1" : ""} ${className}`}
      style={{
        /* macOS Tahoe: flat solid background, no border */
        backgroundColor: hasError ? "var(--status-error-muted)" : "var(--bg-elevated)",
        border: "none",
      }}
    >
      {/* Header - Always visible */}
      <button
        data-testid="tool-call-toggle"
        onClick={() => setIsExpanded(!isExpanded)}
        className={`w-full ${compact ? "px-2 py-1.5" : "px-3 py-2"} text-left hover:opacity-80 transition-opacity`}
        aria-expanded={isExpanded}
        aria-label={`Tool call: ${toolCall.name}. Click to ${isExpanded ? "collapse" : "expand"} details.`}
      >
        {/* Line 1: Chevron + Icon + Tool name badge + Verb + Error indicator */}
        <div className="flex items-center gap-2">
          {/* Expand/collapse icon */}
          {isExpanded ? (
            <ChevronDown
              size={chevronSize}
              className="flex-shrink-0"
              style={{ color: "var(--text-muted)" }}
            />
          ) : (
            <ChevronRight
              size={chevronSize}
              className="flex-shrink-0"
              style={{ color: "var(--text-muted)" }}
            />
          )}

          {/* Tool icon */}
          <ToolIcon name={toolCall.name} hasError={hasError} size={iconSize} />

          {/* Tool name badge - macOS Tahoe flat style */}
          <span
            className={`${compact ? "text-[0.5625rem]" : "text-[0.625rem]"} px-1.5 py-0.5 rounded flex-shrink-0`}
            style={{
              /* macOS Tahoe: subtle solid background */
              backgroundColor: hasError ? "var(--status-error-muted)" : "var(--bg-surface)",
              color: hasError ? "var(--text-primary)" : "var(--text-secondary)",
              fontFamily: "var(--font-mono)",
            }}
          >
            {toolCall.name}
          </span>

          {/* Verb */}
          <span
            className={`${compact ? "text-[0.6875rem]" : "text-xs"} font-medium`}
            style={{ color: "var(--text-secondary)" }}
          >
            {verb}
          </span>

          {/* Streaming indicator — tool call in progress */}
          {isStreaming && !hasError && (
            <Loader2
              size={compact ? 10 : 12}
              className="animate-spin ml-auto flex-shrink-0"
              style={{ color: "var(--accent-primary)" }}
            />
          )}

          {/* Error indicator */}
          {hasError && (
            <span
              className={`${compact ? "text-[0.5625rem]" : "text-[0.625rem]"} font-medium px-1.5 py-0.5 rounded ml-auto`}
              style={{
                /* macOS Tahoe: subtle error background */
                backgroundColor: "var(--status-error-muted)",
                color: "var(--status-error)",
              }}
            >
              Failed
            </span>
          )}
        </div>

        {/* Line 2: File path/summary (indented to align with verb, monospace, break-all) */}
        <div className="flex gap-2 mt-0.5">
          {/* Spacer to align with verb (chevron + icon + badge widths) */}
          <div className="flex gap-2 flex-shrink-0">
            <span style={{ width: `${chevronSize}px` }} />
            <span style={{ width: `${iconSize}px` }} />
          </div>
          <span
            className={`${compact ? "text-[0.625rem]" : "text-[0.6875rem]"} font-mono break-all`}
            style={{
              color: hasError ? "var(--status-error)" : "var(--text-secondary)",
            }}
          >
            {summary.title}
          </span>
        </div>

        {/* Line 3: Subtitle (if present) */}
        {summary.subtitle && (
          <div className="flex gap-2 mt-0.5">
            <div className="flex gap-2 flex-shrink-0">
              <span style={{ width: `${chevronSize}px` }} />
              <span style={{ width: `${iconSize}px` }} />
            </div>
            <span
              className={`${compact ? "text-[0.5625rem]" : "text-[0.625rem]"}`}
              style={{ color: "var(--text-muted)" }}
            >
              {summary.subtitle}
            </span>
          </div>
        )}
      </button>

      {/* Expanded details - NO nested collapse, show raw data directly */}
      {isExpanded && (
        <div
          data-testid="tool-call-details"
          className={`${compact ? "px-2 pb-2" : "px-3 pb-3"} space-y-2 pt-2`}
          style={{
            /* macOS Tahoe: no border separator */
            borderTop: "1px solid var(--overlay-faint)",
          }}
        >
          {/* Arguments - shown directly */}
          <div>
            <div
              className={`${compact ? "text-[0.5625rem]" : "text-[0.625rem]"} font-medium mb-1 uppercase tracking-wide`}
              style={{ color: "var(--text-muted)" }}
            >
              Arguments
            </div>
            <pre
              className={`${compact ? "text-[0.625rem]" : "text-[0.6875rem]"} px-2 py-1.5 rounded overflow-x-auto max-w-full ${compact ? "max-h-32" : "max-h-48"}`}
              style={{
                /* macOS Tahoe: flat dark background */
                backgroundColor: "var(--bg-surface)",
                color: "var(--text-primary)",
                fontFamily: "var(--font-mono)",
                wordBreak: "break-word",
                whiteSpace: "pre-wrap",
              }}
            >
              {formatValue(toolCall.arguments).text}
            </pre>
          </div>

          {/* Result - shown directly (if present and not null) */}
          {toolCall.result != null && !hasError && (
            <div>
              <div
                className={`${compact ? "text-[0.5625rem]" : "text-[0.625rem]"} font-medium mb-1 uppercase tracking-wide`}
                style={{ color: "var(--text-muted)" }}
              >
                Result
              </div>
              <pre
                className={`${compact ? "text-[0.625rem]" : "text-[0.6875rem]"} px-2 py-1.5 rounded overflow-x-auto max-w-full ${compact ? "max-h-32" : "max-h-48"}`}
                style={{
                  /* macOS Tahoe: flat dark background */
                  backgroundColor: "var(--bg-surface)",
                  color: "var(--text-primary)",
                  fontFamily: "var(--font-mono)",
                  wordBreak: "break-word",
                  whiteSpace: "pre-wrap",
                }}
              >
                {formatValue(toolCall.result).text}
              </pre>
            </div>
          )}

          {/* Error (if present) */}
          {hasError && (
            <div>
              <div
                className={`${compact ? "text-[0.5625rem]" : "text-[0.625rem]"} font-medium mb-1 uppercase tracking-wide`}
                style={{ color: "var(--status-error)" }}
              >
                Error
              </div>
              <pre
                className={`${compact ? "text-[0.625rem]" : "text-[0.6875rem]"} px-2 py-1.5 rounded overflow-x-auto`}
                style={{
                  /* macOS Tahoe: error tinted background */
                  backgroundColor: "var(--status-error-muted)",
                  color: "var(--status-error)",
                  fontFamily: "var(--font-mono)",
                }}
              >
                {toolCall.error}
              </pre>
            </div>
          )}
        </div>
      )}
    </div>
  );
});
