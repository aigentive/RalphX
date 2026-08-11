/**
 * McpContextWidgets — Widgets for MCP context and memory tools.
 *
 * Handles:
 * - mcp__ralphx__get_parent_session_context — "Session Context" + success badge
 * - mcp__ralphx__search_memories — query text + result count badge
 *
 * Context and memory tools are plumbing, so they stay compact.
 */

import React from "react";
import { Database, Search } from "lucide-react";
import { InlineIndicator, Badge } from "./shared";
import { colors, getString } from "./shared.constants";
import type { ToolCallWidgetProps } from "./shared.constants";

// ============================================================================
// SessionContextWidget — mcp__ralphx__get_parent_session_context
// ============================================================================

export const SessionContextWidget = React.memo(function SessionContextWidget({
  toolCall,
  className,
}: ToolCallWidgetProps) {
  const hasResult = toolCall.result != null;
  const hasError = Boolean(toolCall.error);

  return (
    <div
      className={className}
      style={{ display: "flex", alignItems: "center", gap: 6, padding: "2px 0", margin: "2px 0" }}
    >
      <Database size={12} style={{ color: colors.textMuted, flexShrink: 0 }} />
      <span style={{ fontSize: 10.5, color: colors.textSecondary }}>Session Context</span>
      {hasError ? (
        <Badge variant="error" compact>error</Badge>
      ) : hasResult ? (
        <Badge variant="success" compact>loaded</Badge>
      ) : (
        <Badge variant="muted" compact>loading</Badge>
      )}
    </div>
  );
});
// ============================================================================
// SearchMemoriesWidget — mcp__ralphx__search_memories
// ============================================================================

/** Count results from search_memories response */
function countResults(result: unknown): number | null {
  if (result == null) return null;
  if (Array.isArray(result)) {
    // MCP wrapper: [{type: "text", text: "..."}]
    const first = result[0];
    if (first && typeof first === "object" && "text" in first) {
      const text = String((first as { text: string }).text);
      const lines = text.split("\n").filter(Boolean);
      return lines.length;
    }
    return result.length;
  }
  if (typeof result === "string") {
    return result.split("\n").filter(Boolean).length;
  }
  return null;
}

export const SearchMemoriesWidget = React.memo(function SearchMemoriesWidget({
  toolCall,
  className,
}: ToolCallWidgetProps) {
  const query = getString(toolCall.arguments, "query");
  const hasResult = toolCall.result != null;
  const hasError = Boolean(toolCall.error);
  const resultCount = hasResult ? countResults(toolCall.result) : null;

  return (
    <div
      className={className}
      style={{ display: "flex", alignItems: "center", gap: 6, padding: "2px 0", margin: "2px 0" }}
    >
      <Search size={12} style={{ color: colors.textMuted, flexShrink: 0 }} />
      <span style={{ fontSize: 10.5, color: colors.textSecondary }}>Search Memories</span>
      {query && (
        <span
          style={{
            fontSize: 10,
            color: colors.textMuted,
            overflow: "hidden",
            textOverflow: "ellipsis",
            whiteSpace: "nowrap",
            maxWidth: 180,
            fontFamily: "var(--font-mono)",
          }}
          title={query}
        >
          {query}
        </span>
      )}
      {hasError ? (
        <Badge variant="error" compact>error</Badge>
      ) : resultCount != null ? (
        <Badge variant="muted" compact>{resultCount} results</Badge>
      ) : hasResult ? (
        <Badge variant="success" compact>done</Badge>
      ) : (
        <InlineIndicator text="" />
      )}
    </div>
  );
});
