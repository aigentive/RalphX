/**
 * IssuesSummaryWidget - Renders get_task_issues results
 *
 * Two modes from the mockup:
 * - Empty result: InlineIndicator with checkmark + "No open issues" (Widget 3)
 * - Has issues: Collapsible card with severity badges and issue list
 */

import React, { useState, useMemo } from "react";
import type { ToolCallWidgetProps } from "./shared.constants";

// ============================================================================
// Types
// ============================================================================

interface IssueData {
  title: string;
  severity: string;
  status?: string;
  description?: string | null;
  file_path?: string | null;
  line_number?: number | null;
}

type IssuesSummaryWidgetProps = ToolCallWidgetProps;

// ============================================================================
// Helpers
// ============================================================================

function parseIssues(result: unknown): IssueData[] {
  if (!result) return [];

  // Handle MCP result wrapper: [{text: "..."}]
  let data = result;
  if (Array.isArray(result) && result.length === 1 && typeof result[0] === "object" && result[0] !== null && "text" in result[0]) {
    try {
      data = JSON.parse((result[0] as { text: string }).text);
    } catch {
      return [];
    }
  }

  if (!Array.isArray(data)) return [];

  return data.filter((item): item is IssueData =>
    item != null &&
    typeof item === "object" &&
    typeof (item as Record<string, unknown>).title === "string" &&
    typeof (item as Record<string, unknown>).severity === "string"
  );
}

const DEFAULT_SEVERITY_STYLE = { bg: "var(--bg-hover)", text: "var(--text-muted)" } as const;

const SEVERITY_COLORS: Record<string, { bg: string; text: string }> = {
  critical: { bg: "var(--status-error-muted)", text: "var(--status-error)" },
  major: { bg: "var(--accent-muted)", text: "var(--accent-primary)" },
  minor: { bg: "var(--status-info-muted)", text: "var(--status-info)" },
  suggestion: DEFAULT_SEVERITY_STYLE,
};

function getSeverityStyle(severity: string): { bg: string; text: string } {
  return SEVERITY_COLORS[severity] ?? DEFAULT_SEVERITY_STYLE;
}

// ============================================================================
// Sub-components
// ============================================================================

function ChevronIcon({ isOpen, compact }: { isOpen: boolean; compact?: boolean }) {
  const size = compact ? 8 : 10;
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
      style={{
        color: "var(--text-muted)",
        flexShrink: 0,
        transition: "transform 200ms",
        transform: isOpen ? "rotate(90deg)" : "rotate(0deg)",
      }}
    >
      <polyline points="9 18 15 12 9 6" />
    </svg>
  );
}

function AlertIcon({ compact }: { compact?: boolean }) {
  const size = compact ? 12 : 14;
  return (
    <svg
      width={size}
      height={size}
      viewBox="0 0 24 24"
      fill="none"
      stroke="currentColor"
      strokeWidth="2"
      strokeLinecap="round"
      strokeLinejoin="round"
      style={{ color: "var(--text-muted)", flexShrink: 0 }}
    >
      <circle cx="12" cy="12" r="10" />
      <line x1="12" y1="8" x2="12" y2="12" />
      <line x1="12" y1="16" x2="12.01" y2="16" />
    </svg>
  );
}

function shortenPath(filePath: string): string {
  const parts = filePath.split("/");
  if (parts.length <= 3) return filePath;
  return ".../" + parts.slice(-2).join("/");
}

// ============================================================================
// Component
// ============================================================================

export const IssuesSummaryWidget = React.memo(function IssuesSummaryWidget({
  toolCall,
  compact = false,
}: IssuesSummaryWidgetProps) {
  const [isOpen, setIsOpen] = useState(false);
  const issues = useMemo(() => parseIssues(toolCall.result), [toolCall.result]);

  // Empty state: InlineIndicator (Widget 3 from mockup)
  if (issues.length === 0) {
    return (
      <div
        data-testid="issues-summary-empty"
        style={{
          display: "flex",
          alignItems: "center",
          gap: "5px",
          padding: "2px 0",
          margin: "2px 0",
        }}
      >
        <svg
          width="12"
          height="12"
          viewBox="0 0 24 24"
          fill="none"
          stroke="var(--text-muted)"
          strokeWidth="2"
          strokeLinecap="round"
          strokeLinejoin="round"
        >
          <polyline points="20 6 9 17 4 12" />
        </svg>
        <span style={{ fontSize: "0.6562rem", color: "var(--text-muted)" }}>
          No open issues
        </span>
      </div>
    );
  }

  // Count by severity for the badge
  const criticalCount = issues.filter((i) => i.severity === "critical").length;
  const majorCount = issues.filter((i) => i.severity === "major").length;
  const hasCritical = criticalCount > 0;

  const badgeStyle: React.CSSProperties = hasCritical
    ? { background: "var(--status-error-muted)", color: "var(--status-error)" }
    : majorCount > 0
      ? { background: "var(--accent-muted)", color: "var(--accent-primary)" }
      : { background: "var(--bg-hover)", color: "var(--text-muted)" };

  return (
    <div
      data-testid="issues-summary-widget"
      style={{
        background: "var(--bg-surface)",
        borderRadius: "10px",
        overflow: "hidden",
        border: "1px solid var(--border-subtle)",
      }}
    >
      {/* Header */}
      <button
        data-testid="issues-summary-toggle"
        onClick={() => setIsOpen(!isOpen)}
        style={{
          display: "flex",
          alignItems: "center",
          gap: "7px",
          padding: compact ? "5px 8px" : "7px 10px",
          cursor: "pointer",
          userSelect: "none",
          transition: "background 200ms",
          minHeight: compact ? "28px" : "32px",
          width: "100%",
          border: "none",
          background: "transparent",
          textAlign: "left",
        }}
        className="hover:opacity-80"
        aria-expanded={isOpen}
        aria-label={`Review Issues. ${issues.length} issue${issues.length !== 1 ? "s" : ""}. Click to ${isOpen ? "collapse" : "expand"}.`}
      >
        <ChevronIcon isOpen={isOpen} compact={compact} />
        <AlertIcon compact={compact} />
        <span
          style={{
            fontSize: compact ? "11px" : "11.5px",
            fontWeight: 500,
            color: "var(--text-secondary)",
            flex: 1,
            overflow: "hidden",
            textOverflow: "ellipsis",
            whiteSpace: "nowrap",
          }}
        >
          Review Issues
        </span>
        <span
          style={{
            fontSize: "0.5938rem",
            padding: "1px 6px",
            borderRadius: "6px",
            fontWeight: 500,
            flexShrink: 0,
            whiteSpace: "nowrap",
            ...badgeStyle,
          }}
        >
          {issues.length} issue{issues.length !== 1 ? "s" : ""}
        </span>
      </button>

      {/* Body */}
      <div
        style={{
          maxHeight: isOpen ? "2000px" : "0px",
          overflow: "hidden",
          transition: "max-height 200ms ease",
        }}
      >
        <div
          style={{
            padding: "0 10px 8px",
            borderTop: "1px solid var(--border-subtle)",
            paddingTop: "8px",
          }}
        >
          <div
            style={{
              display: "flex",
              flexDirection: "column",
              gap: compact ? "4px" : "6px",
            }}
            data-testid="issues-list"
          >
            {issues.map((issue, index) => {
              const sevStyle = getSeverityStyle(issue.severity);
              return (
                <div
                  key={index}
                  data-testid={`issue-item-${index}`}
                  style={{
                    display: "flex",
                    alignItems: "flex-start",
                    gap: "7px",
                    padding: compact ? "3px 0" : "4px 0",
                  }}
                >
                  {/* Severity badge */}
                  <span
                    style={{
                      fontSize: "0.5625rem",
                      padding: "1px 5px",
                      borderRadius: "4px",
                      fontWeight: 600,
                      textTransform: "uppercase",
                      letterSpacing: "0.04em",
                      flexShrink: 0,
                      whiteSpace: "nowrap",
                      background: sevStyle.bg,
                      color: sevStyle.text,
                    }}
                  >
                    {issue.severity}
                  </span>

                  {/* Issue details */}
                  <div style={{ flex: 1, minWidth: 0 }}>
                    <div
                      style={{
                        fontSize: compact ? "11px" : "11.5px",
                        color: "var(--text-secondary)",
                        lineHeight: "1.4",
                      }}
                    >
                      {issue.title}
                    </div>
                    {issue.file_path && (
                      <div
                        style={{
                          fontSize: "0.625rem",
                          color: "var(--text-muted)",
                          fontFamily: "var(--font-mono)",
                          marginTop: "1px",
                          overflow: "hidden",
                          textOverflow: "ellipsis",
                          whiteSpace: "nowrap",
                        }}
                      >
                        {shortenPath(issue.file_path)}
                        {issue.line_number ? `:${issue.line_number}` : ""}
                      </div>
                    )}
                  </div>
                </div>
              );
            })}
          </div>
        </div>
      </div>
    </div>
  );
});
