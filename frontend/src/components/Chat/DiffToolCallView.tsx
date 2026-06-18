/**
 * DiffToolCallView - Renders Edit/Write tool calls as inline diff cards
 *
 * Features:
 * - Collapsed: first hunk preview only
 * - Expanded: full hunk diff hydrates after the expand paint
 * - Header: chevron + tool icon + file path + additions/deletions stats
 * - Falls back to null if no file_path or error, letting parent render generic view
 */

import React, { useCallback, useEffect, useMemo, useState } from "react";
import { Check, ChevronDown, ChevronRight, Copy, FileEdit, FileText } from "lucide-react";
import type { DiffHunk, DiffLine, FileDiff } from "@/api/diff";
import { SimpleDiffView } from "@/components/diff/SimpleDiffView";
import { withAlpha } from "@/lib/theme-colors";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { selectActiveProject, useProjectStore } from "@/stores/projectStore";
import type { ToolCall } from "./ToolCallIndicator";
import { useMessageFileLinkContext } from "./MessageFileLinkContext";
import {
  type DiffResult,
  computeFileDiff,
  extractEditDiff,
  extractWriteDiff,
  getDiffFilePathDisplay,
  getLineBackground,
  getLineNumColor,
  getLinePrefix,
  getPrefixColor,
} from "./DiffToolCallView.utils";

// ============================================================================
// Constants
// ============================================================================

/** Height for ~3.65 lines at 20px line-height */
const COLLAPSED_HEIGHT = 73;
/** Height of gradient blur overlay */
const GRADIENT_HEIGHT = 24;

// ============================================================================
// Types
// ============================================================================

interface DiffToolCallViewProps {
  toolCall: ToolCall;
  isStreaming?: boolean;
  className?: string;
  /** Compact mode for rendering inside task cards — smaller padding, text, icons */
  compact?: boolean;
}

// ============================================================================
// Helpers
// ============================================================================

function extractDiff(toolCall: ToolCall): DiffResult | null {
  const name = toolCall.name.toLowerCase();
  if (name === "edit") return extractEditDiff(toolCall);
  if (name === "write") return extractWriteDiff(toolCall);
  return null;
}

// ============================================================================
// Component
// ============================================================================

export const DiffToolCallView = React.memo(function DiffToolCallView({
  toolCall,
  isStreaming,
  className = "",
  compact = false,
}: DiffToolCallViewProps) {
  const [isExpanded, setIsExpanded] = useState(false);
  const [copiedPath, setCopiedPath] = useState(false);
  const [hydratedFullDiff, setHydratedFullDiff] = useState<FileDiff | null>(null);
  const [isHydratingFullDiff, setIsHydratingFullDiff] = useState(false);

  const diff = useMemo(() => extractDiff(toolCall), [toolCall]);
  const fileLinkContext = useMessageFileLinkContext();
  const activeProjectRootPath = useProjectStore(
    (state) => selectActiveProject(state)?.workingDirectory ?? null
  );
  const diffFilePath = diff?.filePath ?? "";
  const handleCopyPath = useCallback(async () => {
    if (!diffFilePath) return;

    try {
      if (!navigator.clipboard) return;
      await navigator.clipboard.writeText(diffFilePath);
      setCopiedPath(true);
      window.setTimeout(() => setCopiedPath(false), 2000);
    } catch {
      // Clipboard failures should not interrupt diff review.
    }
  }, [diffFilePath]);

  const filePath = diff?.filePath ?? "";
  const fullDiff = diff ? hydratedFullDiff ?? diff.fullDiff : null;
  const canHydrateFullDiff =
    diff?.displayKind === "diff" &&
    diff.oldContent != null &&
    diff.newContent != null;

  const iconSize = compact ? 12 : 14;
  const copyIconSize = compact ? 12 : 13;

  useEffect(() => {
    setHydratedFullDiff(null);
    setIsHydratingFullDiff(false);
  }, [toolCall.id, filePath]);

  useEffect(() => {
    if (!diff || !isExpanded || fullDiff || !canHydrateFullDiff) return;

    let cancelled = false;
    let timeoutId: number | null = null;
    let frameId: number | null = null;

    setIsHydratingFullDiff(true);
    const hydrate = () => {
      timeoutId = window.setTimeout(() => {
        if (cancelled || diff.oldContent == null || diff.newContent == null) return;
        const nextDiff = computeFileDiff(filePath, diff.oldContent, diff.newContent);
        if (!cancelled) {
          setHydratedFullDiff(nextDiff);
          setIsHydratingFullDiff(false);
        }
      }, 0);
    };

    if (typeof window.requestAnimationFrame === "function") {
      frameId = window.requestAnimationFrame(hydrate);
    } else {
      hydrate();
    }

    return () => {
      cancelled = true;
      setIsHydratingFullDiff(false);
      if (frameId != null) window.cancelAnimationFrame(frameId);
      if (timeoutId != null) window.clearTimeout(timeoutId);
    };
  }, [canHydrateFullDiff, diff, filePath, fullDiff, isExpanded]);

  // Fall back to null so parent can render generic view
  if (!diff) return null;

  const { additions, deletions } = diff;
  const workspaceRootPath = fileLinkContext?.workspaceRootPath ?? activeProjectRootPath;
  const displayFilePath = getDiffFilePathDisplay(filePath, workspaceRootPath);
  const isEdit = toolCall.name.toLowerCase() === "edit";
  const statsLabel =
    additions != null || deletions != null
      ? `${additions ?? 0} additions, ${deletions ?? 0} deletions.`
      : diff.newFile
        ? "New file."
      : diff.baselineUnavailable
        ? "Baseline unavailable."
        : "Diff preview.";

  return (
    <div
      data-testid="diff-tool-call-view"
      className={`${compact ? "rounded-md" : "rounded-lg"} overflow-hidden max-w-full ${compact ? "mb-1" : ""} ${className}`}
      style={{
        backgroundColor: "var(--bg-elevated)",
        border: "none",
      }}
    >
      {/* Header */}
      <div
        className={`w-full flex items-center gap-1.5 ${compact ? "px-2 py-1.5" : "px-3 py-2"}`}
      >
        <button
          onClick={() => setIsExpanded(!isExpanded)}
          className="min-w-0 flex flex-1 items-center gap-2 text-left hover:opacity-80 transition-opacity"
          aria-expanded={isExpanded}
          aria-label={`${toolCall.name} ${filePath}. ${statsLabel} Click to ${isExpanded ? "collapse" : "expand"}.`}
        >
          {/* Chevron */}
          {isExpanded ? (
            <ChevronDown size={iconSize} className="flex-shrink-0" style={{ color: "var(--text-muted)" }} />
          ) : (
            <ChevronRight size={iconSize} className="flex-shrink-0" style={{ color: "var(--text-muted)" }} />
          )}

          {/* Tool icon */}
          {isEdit ? (
            <FileEdit size={iconSize} className="flex-shrink-0" style={{ color: "var(--accent-primary)" }} />
          ) : (
            <FileText size={iconSize} className="flex-shrink-0" style={{ color: "var(--accent-primary)" }} />
          )}

          {/* Tool name badge */}
          <span
            className={`${compact ? "text-[0.5625rem]" : "text-[0.625rem]"} px-1.5 py-0.5 rounded flex-shrink-0`}
            style={{
              backgroundColor: "var(--bg-surface)",
              color: "var(--text-secondary)",
              fontFamily: "var(--font-mono)",
            }}
          >
            {toolCall.name}
          </span>

          {/* File path */}
          <Tooltip>
            <TooltipTrigger asChild>
              <span
                data-testid="diff-tool-call-file-path"
                className={`${compact ? "text-[0.6875rem]" : "text-xs"} truncate font-mono flex-1 min-w-0`}
                style={{ color: "var(--text-secondary)" }}
              >
                {displayFilePath}
              </span>
            </TooltipTrigger>
            <TooltipContent
              side="top"
              style={{ maxWidth: "min(720px, calc(100vw - 2rem))" }}
            >
              <p className="font-mono text-xs break-all">{filePath}</p>
            </TooltipContent>
          </Tooltip>

          {/* Stats badge */}
          <span className={`flex-shrink-0 flex items-center gap-1 ${compact ? "text-[0.5625rem]" : "text-[0.625rem]"} font-mono`}>
            {additions != null && additions > 0 && (
              <span style={{ color: "var(--status-success)" }}>+{additions}</span>
            )}
            {deletions != null && deletions > 0 && (
              <span style={{ color: "var(--status-error)" }}>-{deletions}</span>
            )}
          </span>

          {diff.baselineUnavailable && (
            <span
              className={`${compact ? "text-[0.5625rem]" : "text-[0.625rem]"} flex-shrink-0 rounded px-1.5 py-0.5`}
              style={{
                backgroundColor: "var(--status-warning-muted)",
                color: "var(--status-warning)",
              }}
            >
              Baseline unavailable
            </span>
          )}

          {diff.newFile && (
            <span
              className={`${compact ? "text-[0.5625rem]" : "text-[0.625rem]"} flex-shrink-0 rounded px-1.5 py-0.5`}
              style={{
                backgroundColor: "var(--status-success-muted)",
                color: "var(--status-success)",
              }}
            >
              New file
            </span>
          )}

          {/* Streaming indicator */}
          {isStreaming && (
            <span
              className={`${compact ? "text-[0.5625rem]" : "text-[0.625rem]"} px-1.5 py-0.5 rounded flex-shrink-0 animate-pulse`}
              style={{
                backgroundColor: withAlpha("var(--accent-primary)", 15),
                color: "var(--accent-primary)",
              }}
            >
              writing...
            </span>
          )}
        </button>

        <Tooltip>
          <TooltipTrigger asChild>
            <button
              type="button"
              data-testid="diff-tool-call-copy-path"
              aria-label={copiedPath ? "Copied" : "Copy file path"}
              onClick={() => void handleCopyPath()}
              className={`flex shrink-0 items-center justify-center rounded-sm transition-colors hover:bg-[var(--bg-hover)] ${compact ? "h-5 w-5" : "h-6 w-6"}`}
              style={{ color: copiedPath ? "var(--status-success)" : "var(--text-muted)" }}
            >
              {copiedPath ? (
                <Check size={copyIconSize} aria-hidden="true" />
              ) : (
                <Copy size={copyIconSize} aria-hidden="true" />
              )}
            </button>
          </TooltipTrigger>
          <TooltipContent side="top" className="text-xs">
            {copiedPath ? "Copied" : "Copy path"}
          </TooltipContent>
        </Tooltip>
      </div>

      {/* Diff content */}
      <div
        style={{
          position: "relative",
          overflow: "hidden",
          maxHeight: isExpanded ? "none" : `${COLLAPSED_HEIGHT}px`,
        }}
      >
        {diff.displayKind === "final-content" ? (
          <FinalContentBlock content={diff.finalContent ?? ""} isExpanded={isExpanded} />
        ) : isExpanded ? (
          <ExpandedDiff
            diff={fullDiff}
            previewDiff={diff.previewDiff}
            isHydrating={isHydratingFullDiff}
          />
        ) : (
          <HunkPreview diff={diff.previewDiff} />
        )}

        {/* Gradient blur overlay (collapsed only) */}
        {!isExpanded && (
          <div
            style={{
              position: "absolute",
              bottom: 0,
              left: 0,
              right: 0,
              height: `${GRADIENT_HEIGHT}px`,
              background: "linear-gradient(transparent, var(--bg-elevated))",
              pointerEvents: "none",
            }}
          />
        )}
      </div>
    </div>
  );
});

// ============================================================================
// Sub-components
// ============================================================================

const DiffLineRow = React.memo(function DiffLineRow({ line }: { line: DiffLine }) {
  return (
    <div
      className="flex"
      style={{
        backgroundColor: getLineBackground(line.kind),
        minHeight: "20px",
      }}
    >
      {/* Old line number */}
      <span
        className="select-none text-right flex-shrink-0 pr-2"
        style={{
          width: "48px",
          color: getLineNumColor(line.kind),
          userSelect: "none",
        }}
      >
        {line.oldLineNum ?? ""}
      </span>

      {/* New line number */}
      <span
        className="select-none text-right flex-shrink-0 pr-2 border-r"
        style={{
          width: "48px",
          color: getLineNumColor(line.kind),
          userSelect: "none",
          borderColor: "var(--border-subtle)",
        }}
      >
        {line.newLineNum ?? ""}
      </span>

      {/* Prefix (+/-/space) */}
      <span
        className="flex-shrink-0"
        style={{
          width: "24px",
          color: getPrefixColor(line.kind),
          textAlign: "center",
        }}
      >
        {getLinePrefix(line.kind)}
      </span>

      {/* Content */}
      <span
        className="whitespace-pre overflow-hidden text-ellipsis flex-1 min-w-0 pr-4"
        style={{ color: "var(--text-secondary)" }}
      >
        {line.content || " "}
      </span>
    </div>
  );
});

function HunkPreview({ diff }: { diff: FileDiff | null }) {
  const hunk = diff?.hunks[0];
  if (!hunk) {
    return (
      <div
        data-testid="diff-tool-call-preview-diff"
        className="px-3 py-2 text-xs"
        style={{ color: "var(--text-muted)" }}
      >
        No changes
      </div>
    );
  }

  return (
    <div
      data-testid="diff-tool-call-preview-diff"
      style={{
        fontFamily: "var(--font-mono)",
        fontSize: "0.6875rem",
        lineHeight: "18px",
      }}
    >
      <HunkHeader hunk={hunk} />
      {hunk.lines.map((line, index) => (
        <DiffLineRow key={index} line={line} />
      ))}
    </div>
  );
}

function HunkHeader({ hunk }: { hunk: DiffHunk }) {
  return (
    <div
      className="px-3 py-1 font-mono text-[0.6875rem]"
      style={{
        backgroundColor: "var(--overlay-weak)",
        borderBottom: "1px solid var(--overlay-weak)",
        color: withAlpha("var(--text-primary)", 60),
      }}
    >
      {hunk.header}
    </div>
  );
}

function ExpandedDiff({
  diff,
  previewDiff,
  isHydrating,
}: {
  diff: FileDiff | null;
  previewDiff: FileDiff | null;
  isHydrating: boolean;
}) {
  if (!diff) {
    return (
      <div>
        {previewDiff ? <HunkPreview diff={previewDiff} /> : null}
        <div
          className="px-3 py-2 text-xs"
          style={{ color: "var(--text-muted)" }}
        >
          {isHydrating ? "Loading full diff..." : "Full diff unavailable"}
        </div>
      </div>
    );
  }

  return (
    <div data-testid="diff-tool-call-full-diff">
      <SimpleDiffView
        hunks={diff.hunks}
        oldTotalLines={diff.oldTotalLines}
        newTotalLines={diff.newTotalLines}
        isBinary={diff.isBinary}
        language={diff.language}
        scrollContainer={false}
        density="compact"
        defaultWrapLines={false}
        showWrapToggle={false}
        showContextGaps={false}
      />
    </div>
  );
}

function FinalContentBlock({
  content,
  isExpanded,
}: {
  content: string;
  isExpanded: boolean;
}) {
  const visibleContent = isExpanded ? content : content.split("\n").slice(0, 4).join("\n");
  return (
    <div
      data-testid="diff-tool-call-final-content"
      className="px-3 py-2"
      style={{
        backgroundColor: "var(--bg-base)",
        color: "var(--text-secondary)",
        fontFamily: "var(--font-mono)",
        fontSize: "0.6875rem",
        lineHeight: "20px",
        whiteSpace: "pre",
        overflow: "hidden",
        textOverflow: "ellipsis",
      }}
    >
      {visibleContent || "Final file content is empty."}
    </div>
  );
}
