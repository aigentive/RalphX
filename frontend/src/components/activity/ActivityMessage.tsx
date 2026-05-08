/**
 * ActivityMessage - Individual message display component
 *
 * Smart content rendering based on event type:
 * - tool_result: Formatted JSON with syntax highlighting
 * - tool_call: Tool name badge + formatted arguments
 * - thinking: Markdown rendering
 * - text: Markdown rendering (headers, lists, code blocks, tables)
 * - error: Plain text with whitespace preserved
 */

import { useCallback, useMemo } from "react";
import { ChevronDown, Copy, Check } from "lucide-react";
import ReactMarkdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";
import type { UnifiedActivityMessage } from "./ActivityView.types";
import {
  getMessageIcon,
  getMessageColor,
  getMessageBgColor,
  getToolName,
  formatTimestamp,
  highlightJSON,
  safeJsonParse,
  cleanToolName,
  formatToolArguments,
  generateResultPreview,
} from "./ActivityView.utils";
import { ActivityContext } from "./ActivityContext";
import { markdownComponents } from "@/components/Chat/MessageItem.markdown";
import { navigateToIdeationSession } from "@/lib/navigation";

export interface ActivityMessageProps {
  message: UnifiedActivityMessage;
  isExpanded: boolean;
  onToggle: () => void;
  copied: boolean;
  onCopy: () => void;
}

export function ActivityMessage({
  message,
  isExpanded,
  onToggle,
  copied,
  onCopy,
}: ActivityMessageProps) {
  const { type, content, timestamp, metadata } = message;
  const hasDetails = type === "tool_call" || type === "tool_result" || metadata;
  const rawToolName = getToolName(content);
  const toolName = rawToolName ? cleanToolName(rawToolName) : null;
  const followupSessionId =
    typeof metadata?.followupSessionId === "string" ? metadata.followupSessionId : null;

  // Smart content rendering based on event type
  const renderedContent = useMemo(() => {
    switch (type) {
      case "tool_result": {
        // Semantic tool result rendering with human-readable preview
        const { preview, isError } = generateResultPreview(content);
        const result = safeJsonParse(content);
        const hasValidJson = !result.error && typeof result.data === "object" && result.data !== null;

        return (
          <div className="mt-1 space-y-1">
            {/* Human-readable preview */}
            <div className="flex items-start gap-2">
              <span
                className={cn(
                  "text-xs font-medium shrink-0",
                  isError ? "text-[var(--status-error)]" : "text-[var(--status-success)]"
                )}
              >
                {isError ? "✗" : "✓"}
              </span>
              <p className="text-sm text-[var(--text-secondary)]">{preview}</p>
            </div>

            {/* Expandable full JSON (only in expanded state with valid JSON) */}
            {isExpanded && hasValidJson && (
              <div className="pt-2">
                <pre className="text-xs font-mono p-2 rounded-md bg-[var(--bg-base)] text-[var(--text-secondary)] overflow-x-auto max-h-[200px] overflow-y-auto">
                  {highlightJSON(JSON.stringify(result.data, null, 2))}
                </pre>
              </div>
            )}

            {/* Fallback for non-JSON content when expanded */}
            {isExpanded && !hasValidJson && content.length > 100 && (
              <pre className="text-xs font-mono p-2 rounded-md bg-[var(--bg-base)] text-[var(--text-secondary)] overflow-x-auto max-h-[200px] overflow-y-auto mt-1 whitespace-pre-wrap">
                {content}
              </pre>
            )}
          </div>
        );
      }

      case "tool_call": {
        // Semantic tool call rendering with clean name and formatted arguments
        const formattedArgs = formatToolArguments(metadata as Record<string, unknown> | undefined);

        if (formattedArgs.length > 0) {
          return (
            <div className="mt-1 space-y-1">
              {/* Formatted key-value arguments */}
              <div className="space-y-0.5">
                {formattedArgs.map(({ key, value }) => (
                  <div key={key} className="flex gap-2 text-xs font-mono">
                    <span className="text-[var(--text-muted)] shrink-0">{key}</span>
                    <span className="text-[var(--text-secondary)] break-all">{value}</span>
                  </div>
                ))}
              </div>
            </div>
          );
        }

        // Fallback to plain content if no structured metadata
        const truncatedContent = !isExpanded && content.length > 200 ? content.slice(0, 200) + "..." : content;
        return (
          <p className="text-sm text-[var(--text-primary)] whitespace-pre-wrap break-words mt-1">
            {truncatedContent}
          </p>
        );
      }

      case "thinking": {
        // Render thinking content as markdown
        const truncatedContent = !isExpanded && content.length > 500 ? content.slice(0, 500) + "..." : content;
        return (
          <div className="text-sm text-[var(--text-primary)] mt-1 prose-sm prose-invert max-w-none">
            <ReactMarkdown
              remarkPlugins={[remarkGfm]}
              components={markdownComponents}
            >
              {truncatedContent}
            </ReactMarkdown>
          </div>
        );
      }

      case "system": {
        const truncatedContent = !isExpanded && content.length > 200 ? content.slice(0, 200) + "..." : content;
        return (
          <div className="mt-1 space-y-2">
            <div className="text-sm text-[var(--text-primary)] prose-sm prose-invert max-w-none">
              <ReactMarkdown
                remarkPlugins={[remarkGfm]}
                components={markdownComponents}
              >
                {truncatedContent}
              </ReactMarkdown>
            </div>
            {followupSessionId && (
              <Button
                type="button"
                variant="outline"
                size="sm"
                className="h-7 px-2 text-[0.6875rem] border-[var(--border-subtle)] bg-[var(--bg-base)] hover:bg-[var(--bg-hover)]"
                onClick={(event) => {
                  event.stopPropagation();
                  navigateToIdeationSession(followupSessionId);
                }}
              >
                Open Follow-up
              </Button>
            )}
          </div>
        );
      }

      case "text":
      default: {
        // Render text messages as markdown (same as thinking blocks)
        const truncatedContent = !isExpanded && content.length > 200 ? content.slice(0, 200) + "..." : content;
        return (
          <div className="text-sm text-[var(--text-primary)] mt-1 prose-sm prose-invert max-w-none">
            <ReactMarkdown
              remarkPlugins={[remarkGfm]}
              components={markdownComponents}
            >
              {truncatedContent}
            </ReactMarkdown>
          </div>
        );
      }

      case "error": {
        // Error messages rendered as plain text (preserve exact formatting)
        const truncatedContent = !isExpanded && content.length > 200 ? content.slice(0, 200) + "..." : content;
        return (
          <p className="text-sm text-[var(--text-primary)] whitespace-pre-wrap break-words mt-1">
            {truncatedContent}
          </p>
        );
      }
    }
  }, [type, content, metadata, isExpanded, followupSessionId]);

  const handleCopy = useCallback(
    (e: React.MouseEvent) => {
      e.stopPropagation();
      if (metadata) {
        try {
          navigator.clipboard.writeText(JSON.stringify(metadata, null, 2));
          onCopy();
        } catch {
          // Silently fail if metadata can't be stringified (shouldn't happen, but safe)
        }
      }
    },
    [metadata, onCopy]
  );

  return (
    <div
      data-testid="activity-message"
      data-type={type}
      className="rounded-lg transition-all"
      style={{
        backgroundColor: getMessageBgColor(type),
        borderLeft: `3px solid ${getMessageColor(type)}`,
      }}
    >
      {/* Header */}
      <div
        className={cn(
          "flex items-start gap-3 px-3 py-2.5 select-none",
          hasDetails && "cursor-pointer hover:bg-white/[0.02]"
        )}
        onClick={hasDetails ? onToggle : undefined}
      >
        {/* Expand/Collapse Icon */}
        {hasDetails && (
          <ChevronDown
            className={cn(
              "w-3 h-3 mt-1 text-[var(--text-muted)] transition-transform shrink-0",
              !isExpanded && "-rotate-90"
            )}
          />
        )}
        {!hasDetails && <span className="w-3 shrink-0" />}

        {/* Type Icon */}
        <span className="mt-0.5 shrink-0" style={{ color: getMessageColor(type) }}>
          {getMessageIcon(type)}
        </span>

        {/* Content */}
        <div className="flex-1 min-w-0">
          <div className="flex items-center gap-2">
            {/* Show tool name badge for tool calls, or type label for other messages */}
            {toolName ? (
              <span
                className="text-xs font-mono font-medium"
                style={{ color: getMessageColor(type) }}
              >
                {toolName}
              </span>
            ) : (
              <span className="text-xs text-[var(--text-muted)] capitalize">
                {type.replace("_", " ")}
              </span>
            )}
          </div>
          {/* Context: Source (task/session) and role */}
          <ActivityContext
            taskId={message.taskId}
            sessionId={message.sessionId}
            role={message.role}
          />
          {/* Smart content rendering based on event type */}
          {renderedContent}
        </div>

        {/* Timestamp - right-aligned with subtle styling */}
        <span className="text-[0.6875rem] text-[var(--text-muted)] opacity-60 shrink-0 ml-auto pl-3 tabular-nums">
          {formatTimestamp(timestamp)}
        </span>
      </div>

      {/* Expanded Details / Raw JSON */}
      {hasDetails && isExpanded && metadata && (
        <div className="mx-3 pb-3 border-t border-[var(--border-subtle)]">
          <div className="pt-2.5 relative">
            <div className="flex items-center justify-between mb-1.5">
              <span className="text-[0.625rem] font-medium text-[var(--text-muted)] uppercase tracking-wide">
                {type === "tool_call" ? "Raw JSON" : "Details"}
              </span>
              <Button
                variant="ghost"
                size="icon"
                className="h-6 w-6 hover:bg-[var(--bg-hover)]"
                onClick={handleCopy}
              >
                {copied ? (
                  <Check className="w-3.5 h-3.5 text-[var(--status-success)]" />
                ) : (
                  <Copy className="w-3.5 h-3.5 text-[var(--text-muted)]" />
                )}
              </Button>
            </div>
            <pre className="text-xs font-mono p-3 rounded-md bg-[var(--bg-base)] text-[var(--text-secondary)] overflow-x-auto max-h-[300px] overflow-y-auto">
              {(() => {
                try {
                  return highlightJSON(JSON.stringify(metadata, null, 2));
                } catch {
                  // Fallback to string representation if stringify fails
                  return String(metadata);
                }
              })()}
            </pre>
          </div>
        </div>
      )}
    </div>
  );
}
