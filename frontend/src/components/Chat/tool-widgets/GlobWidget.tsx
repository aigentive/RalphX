/**
 * GlobWidget — File search result card for Glob tool calls.
 *
 * Design: folder-search icon + glob pattern in monospace + "N matches" badge.
 * Body: matched file paths listed.
 * Inline (no collapse) when ≤3 results, collapsible when more.
 *
 * Reference: Widget 9 in mockups/tool-call-widgets.html
 */

import React, { useMemo } from "react";
import { FolderSearch } from "lucide-react";
import { WidgetCard, WidgetHeader, Badge } from "./shared";
import type { ToolCallWidgetProps } from "./shared";
import { colors, parseSearchResult } from "./shared.constants";
import { FileList } from "./GrepWidget";

/** Parse Glob arguments from tool call */
function parseGlobArgs(args: unknown): {
  pattern: string;
  path: string | undefined;
} {
  const typed = args as {
    pattern?: string;
    path?: string;
    file_pattern?: string;
    base_path?: string;
  } | undefined;
  return {
    pattern: typed?.pattern || typed?.file_pattern || "**/*",
    path: typed?.path || typed?.base_path,
  };
}

export const GlobWidget = React.memo(function GlobWidget({
  toolCall,
  compact = false,
}: ToolCallWidgetProps) {
  const { pattern, path } = parseGlobArgs(toolCall.arguments);
  const parsed = useMemo(() => parseSearchResult(toolCall.result), [toolCall.result]);
  const fileCount = parsed.paths.length;

  // Build title: glob pattern in monospace
  const title = path ? `${pattern} in ${path}` : pattern;

  // Badge text
  const badgeText = fileCount === 0
    ? "no matches"
    : fileCount === 1
      ? "1 match"
      : `${fileCount} matches`;

  const header = (
    <WidgetHeader
      icon={<FolderSearch size={14} />}
      title={title}
      mono
      compact={compact}
      badge={
        <Badge variant="muted">
          {badgeText}
        </Badge>
      }
    />
  );

  // Pending result (tool still running)
  if (toolCall.result === undefined) {
    return (
      <WidgetCard header={header} compact={compact}>
        <span style={{ fontSize: 10.5, color: colors.textMuted }}>
          Searching...
        </span>
      </WidgetCard>
    );
  }

  // No results: header + muted note
  if (parsed.isEmpty) {
    return (
      <WidgetCard header={header} compact={compact}>
        <span style={{ fontSize: 10.5, color: colors.textMuted }}>
          {parsed.note || "No files matched"}
        </span>
      </WidgetCard>
    );
  }

  return (
    <WidgetCard
      header={header}
      compact={compact}
      alwaysExpanded={fileCount <= 3}
    >
      <FileList files={parsed.paths} />
      {parsed.note && (
        <div style={{ fontSize: 10, color: colors.textMuted, paddingTop: 4 }}>
          {parsed.note}
        </div>
      )}
    </WidgetCard>
  );
});
