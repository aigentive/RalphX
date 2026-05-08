import { useEffect, useMemo, useState } from "react";
import {
  ChevronDown,
  ChevronRight,
  FileEdit,
  FileText,
  FolderSearch,
  Loader2,
  Search,
  Terminal,
  Wrench,
} from "lucide-react";
import { createSummary, formatValue, getToolVerb } from "./ToolCallIndicator.helpers";
import type { ToolCall } from "./tool-widgets/shared.constants";
import { useLazyToolCallDetail } from "./useLazyToolCallDetail";

interface ToolCallPreviewCardProps {
  toolCall: ToolCall;
  className?: string;
  compact?: boolean;
  isStreaming?: boolean;
}

function ToolIcon({
  name,
  hasError,
  size = 14,
}: {
  name: string;
  hasError: boolean;
  size?: number;
}) {
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

export function ToolCallPreviewCard({
  toolCall,
  className = "",
  compact = false,
  isStreaming = false,
}: ToolCallPreviewCardProps) {
  const [isExpanded, setIsExpanded] = useState(false);
  const {
    detailError,
    displayToolCall,
    fullToolCall,
    isLoadingDetail,
    loadDetail,
  } = useLazyToolCallDetail(toolCall);
  const summary = useMemo(() => createSummary(toolCall), [toolCall]);
  const verb = useMemo(() => getToolVerb(toolCall.name), [toolCall.name]);
  const hasError = Boolean(displayToolCall.error);
  const iconSize = compact ? 12 : 14;
  const chevronSize = compact ? 12 : 14;

  useEffect(() => {
    if (isExpanded) void loadDetail();
  }, [isExpanded, loadDetail]);

  return (
    <div
      data-testid="tool-call-preview-card"
      className={`${compact ? "rounded-md" : "rounded-lg"} overflow-hidden max-w-full ${compact ? "mb-1" : ""} ${className}`}
      style={{
        backgroundColor: hasError ? "var(--status-error-muted)" : "var(--bg-elevated)",
        border: "none",
      }}
    >
      <button
        data-testid="tool-call-toggle"
        onClick={() => setIsExpanded(!isExpanded)}
        className={`w-full ${compact ? "px-2 py-1.5" : "px-3 py-2"} text-left hover:opacity-80 transition-opacity`}
        aria-expanded={isExpanded}
        aria-label={`Tool call: ${toolCall.name}. Click to ${isExpanded ? "collapse" : "expand"} details.`}
      >
        <div className="flex items-center gap-2">
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
          <ToolIcon name={toolCall.name} hasError={hasError} size={iconSize} />
          <span
            className={`${compact ? "text-[0.5625rem]" : "text-[0.625rem]"} px-1.5 py-0.5 rounded flex-shrink-0`}
            style={{
              backgroundColor: hasError ? "var(--status-error-muted)" : "var(--bg-surface)",
              color: hasError ? "var(--text-primary)" : "var(--text-secondary)",
              fontFamily: "var(--font-mono)",
            }}
          >
            {toolCall.name}
          </span>
          <span
            className={`${compact ? "text-[0.6875rem]" : "text-xs"} font-medium`}
            style={{ color: "var(--text-secondary)" }}
          >
            {verb}
          </span>
          {isStreaming && !hasError && (
            <Loader2
              size={compact ? 10 : 12}
              className="animate-spin ml-auto flex-shrink-0"
              style={{ color: "var(--accent-primary)" }}
            />
          )}
        </div>
        <div className="flex gap-2 mt-0.5">
          <div className="flex gap-2 flex-shrink-0">
            <span style={{ width: `${chevronSize}px` }} />
            <span style={{ width: `${iconSize}px` }} />
          </div>
          <span
            className={`${compact ? "text-[0.625rem]" : "text-[0.6875rem]"} font-mono break-all`}
            style={{ color: hasError ? "var(--status-error)" : "var(--text-secondary)" }}
          >
            {summary.title}
          </span>
        </div>
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

      {isExpanded && (
        <div
          data-testid="tool-call-details"
          className={`${compact ? "px-2 pb-2" : "px-3 pb-3"} space-y-2 pt-2`}
          style={{ borderTop: "1px solid var(--overlay-faint)" }}
        >
          {isLoadingDetail && (
            <div
              data-testid="tool-call-detail-loading"
              className={`${compact ? "text-[0.625rem]" : "text-[0.6875rem]"}`}
              style={{ color: "var(--text-muted)" }}
            >
              Loading full result...
            </div>
          )}
          {detailError && (
            <div
              className={`${compact ? "text-[0.625rem]" : "text-[0.6875rem]"}`}
              style={{ color: "var(--status-error)" }}
            >
              {detailError}
            </div>
          )}
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
                backgroundColor: "var(--bg-surface)",
                color: "var(--text-primary)",
                fontFamily: "var(--font-mono)",
                wordBreak: "break-word",
                whiteSpace: "pre-wrap",
              }}
            >
              {formatValue(displayToolCall.arguments).text}
            </pre>
          </div>
          {displayToolCall.result != null && !hasError && (
            <div>
              <div
                className={`${compact ? "text-[0.5625rem]" : "text-[0.625rem]"} font-medium mb-1 uppercase tracking-wide`}
                style={{ color: "var(--text-muted)" }}
              >
                {fullToolCall ? "Result" : "Preview"}
              </div>
              <pre
                className={`${compact ? "text-[0.625rem]" : "text-[0.6875rem]"} px-2 py-1.5 rounded overflow-x-auto max-w-full ${compact ? "max-h-32" : "max-h-48"}`}
                style={{
                  backgroundColor: "var(--bg-surface)",
                  color: "var(--text-primary)",
                  fontFamily: "var(--font-mono)",
                  wordBreak: "break-word",
                  whiteSpace: "pre-wrap",
                }}
              >
                {formatValue(displayToolCall.result).text}
              </pre>
            </div>
          )}
        </div>
      )}
    </div>
  );
}
