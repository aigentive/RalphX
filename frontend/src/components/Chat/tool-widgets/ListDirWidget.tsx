/**
 * ListDirWidget - Directory listing card for fs_list_dir MCP calls.
 */

import React, { useMemo } from "react";
import { FileText, Folder, FolderOpen } from "lucide-react";
import { WidgetCard, WidgetHeader, Badge } from "./shared";
import type { ToolCallWidgetProps } from "./shared";
import {
  colors,
  normalizeDisplayPath,
  parseToolResultAsLines,
  shortenPath,
} from "./shared.constants";

interface DirectoryEntry {
  kind: "dir" | "file" | "other";
  label: string;
}

const DIRECTORY_METADATA_RE =
  /^(DIRECTORY|ENTRIES|DIRECTORIES_ONLY|INCLUDE_HIDDEN|RESPECT_GITIGNORE):/;
const DIRECTORY_NOTE_RE = /^NOTE:\s*(.+)$/i;
const DIRECTORY_ERROR_RE = /^ERROR:\s*(.+)$/i;

function extractDirectoryPath(args: unknown): string {
  if (args && typeof args === "object") {
    const record = args as Record<string, unknown>;
    if (typeof record.path === "string") return record.path;
    if (typeof record.base_path === "string") return record.base_path;
  }
  return "directory";
}

function parseDirectoryEntries(result: unknown): {
  entries: DirectoryEntry[];
  error?: string;
  empty: boolean;
  note?: string;
} {
  const entries: DirectoryEntry[] = [];
  const noteLines: string[] = [];
  const lines = parseToolResultAsLines(result);

  for (const line of lines) {
    if (DIRECTORY_METADATA_RE.test(line)) {
      continue;
    }

    const noteMatch = line.match(DIRECTORY_NOTE_RE);
    if (noteMatch?.[1]) {
      noteLines.push(noteMatch[1].trim());
      continue;
    }

    const errorMatch = line.match(DIRECTORY_ERROR_RE);
    if (errorMatch?.[1]) {
      return {
        entries: [],
        error: `ERROR: ${errorMatch[1].trim()}`,
        empty: true,
      };
    }

    if (line.startsWith("DIR  ")) {
      entries.push({ kind: "dir", label: line.slice(5).trim() });
      continue;
    }

    if (line.startsWith("FILE ")) {
      entries.push({ kind: "file", label: line.slice(5).trim() });
      continue;
    }

    if (line.length > 0) {
      entries.push({ kind: "other", label: line });
    }
  }

  return {
    entries,
    empty: entries.length === 0,
    ...(noteLines.length > 0 && { note: noteLines.join(" ") }),
  };
}

function DirectoryEntryIcon({ kind }: { kind: DirectoryEntry["kind"] }) {
  if (kind === "dir") {
    return <Folder size={12} style={{ color: colors.textMuted, flexShrink: 0 }} />;
  }
  return <FileText size={12} style={{ color: colors.textMuted, flexShrink: 0 }} />;
}

export const ListDirWidget = React.memo(function ListDirWidget({
  toolCall,
  compact = false,
}: ToolCallWidgetProps) {
  const rawPath = useMemo(() => extractDirectoryPath(toolCall.arguments), [toolCall.arguments]);
  const displayPath = useMemo(
    () => shortenPath(normalizeDisplayPath(rawPath), compact ? 40 : 50),
    [rawPath, compact],
  );
  const parsed = useMemo(() => parseDirectoryEntries(toolCall.result), [toolCall.result]);

  const badgeText =
    parsed.entries.length === 0
      ? "empty"
      : parsed.entries.length === 1
        ? "1 entry"
        : `${parsed.entries.length} entries`;

  const header = (
    <WidgetHeader
      icon={<FolderOpen size={14} />}
      title={displayPath}
      mono
      compact={compact}
      badge={
        <Badge variant={parsed.error ? "error" : "muted"} compact>
          {parsed.error ? "error" : badgeText}
        </Badge>
      }
    />
  );

  if (toolCall.result === undefined) {
    return (
      <WidgetCard header={header} compact={compact}>
        <span style={{ fontSize: 10.5, color: colors.textMuted }}>Listing...</span>
      </WidgetCard>
    );
  }

  if (parsed.error || parsed.empty) {
    return (
      <WidgetCard header={header} compact={compact}>
        <span
          style={{
            fontSize: 10.5,
            color: parsed.error ? "var(--status-error)" : colors.textMuted,
            fontFamily: parsed.error ? "var(--font-mono)" : undefined,
          }}
        >
          {parsed.error || parsed.note || "No entries found"}
        </span>
      </WidgetCard>
    );
  }

  return (
    <WidgetCard header={header} compact={compact} alwaysExpanded={parsed.entries.length <= 6}>
      <div
        style={{
          fontFamily: "var(--font-mono)",
          fontSize: 11,
          lineHeight: 1.6,
          color: colors.textSecondary,
          padding: "4px 0",
        }}
      >
        {parsed.entries.map((entry, index) => (
          <div
            key={`${entry.kind}:${entry.label}:${index}`}
            style={{
              display: "flex",
              alignItems: "center",
              gap: 6,
              padding: "1px 0",
            }}
          >
            <DirectoryEntryIcon kind={entry.kind} />
            <span
              style={{
                overflow: "hidden",
                textOverflow: "ellipsis",
                whiteSpace: "nowrap",
              }}
            >
              {entry.label}
            </span>
          </div>
        ))}
      </div>
      {parsed.note && (
        <div style={{ fontSize: 10, color: colors.textMuted, paddingTop: 4 }}>
          {parsed.note}
        </div>
      )}
    </WidgetCard>
  );
});
