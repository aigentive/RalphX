/**
 * AgentsPublishFileDiff
 *
 * Per-file collapsible card in the inline diff view.
 * Parent manages fallback diff fetching via `diff`; page-capable diffs fetch rows lazily.
 *
 * Performance contract (frontend-interaction-performance.md):
 * - Header (file path, status badge, +/−, buttons) paints synchronously.
 * - Body (SimpleDiffView) only mounts when isExpanded=true AND diff data is available.
 * - Collapsed cards: no SimpleDiffView, no skeleton — zero mount cost.
 *
 * Icon-only buttons (icon-only-buttons.md): aria-label + Tooltip on every icon button.
 * WKWebView CSS: explicit background-color / border-color with shallow-chain tokens.
 */

import { useEffect, useRef } from "react";
import { Copy, ChevronRight, Maximize2, RefreshCw } from "lucide-react";
import { cn } from "@/lib/utils";
import { ConflictDiffViewer } from "@/components/diff/ConflictDiffViewer";
import { PagedDiffView } from "@/components/diff/PagedDiffView";
import { SimpleDiffView } from "@/components/diff/SimpleDiffView";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { Skeleton } from "@/components/ui/skeleton";
import type {
  ConflictDiff,
  FileChange,
  FileDiff,
  FileDiffPage,
  DiffRefKind,
  PrDiffAnnotation,
  WorkspaceReviewHunkAnnotation,
} from "@/api/diff";
import {
  canUsePagedInlineDiff,
  requiresExplicitDiffHydration,
} from "./inlineDiffGuards";

export type DiffState = FileDiff | "loading" | "error" | undefined;
export type ConflictDiffState = ConflictDiff | "loading" | "error" | undefined;
export type DiffPageSummary = Pick<FileDiffPage, "totalRows" | "isBinary">;

export interface AgentsPublishFileDiffProps {
  file: FileChange;
  diff: DiffState;
  conflictDiff?: ConflictDiffState;
  isConflictMode?: boolean;
  isExpanded: boolean;
  onToggle: () => void;
  onCopyPath: (path: string) => void;
  onOpenFullscreen: (path: string) => void;
  onRetry?: (() => void) | undefined;
  /** Workspace conversation ID — enables lazy range fetch in SimpleDiffView. */
  conversationId?: string | undefined;
  /** Which diff reference to use for range fetches. */
  refKind?: DiffRefKind | undefined;
  /** Which diff reference to use for paged row fetching. */
  diffPageRefKind?: DiffRefKind | undefined;
  /** Optional remount key for paged rows when same-ref content changes. */
  diffPageReloadKey?: string | undefined;
  /** Outer inline diff scroller used by row-virtualized paged diffs. */
  inlineDiffScrollParent?: HTMLElement | null | undefined;
  /** Lightweight page metadata used to reserve stable inline row height. */
  diffPageSummary?: DiffPageSummary | undefined;
  /** Whether this file is in the viewport (±200px) — controls body hydration. */
  shouldHydrate: boolean;
  /** GitHub PR review/check annotations for this file. */
  annotations?: PrDiffAnnotation[] | undefined;
  /** Workspace Review hunk notes for this file/ref. */
  hunkAnnotations?: WorkspaceReviewHunkAnnotation[] | undefined;
  /** Whether the user has clicked "Show anyway" for a generated file. */
  isShowAnywayOverridden: boolean;
  /** Called when the user clicks "Show anyway" on a generated-file placeholder. */
  onShowAnyway: () => void;
  /** Focus the path control after an external jump request opens this file. */
  isFocusTarget?: boolean;
}

function statusLetter(status: FileChange["status"]): string {
  switch (status) {
    case "added":
      return "A";
    case "deleted":
      return "D";
    default:
      return "M";
  }
}

function statusColor(status: FileChange["status"]): string {
  switch (status) {
    case "added":
      return "var(--status-success)";
    case "deleted":
      return "var(--status-error)";
    default:
      return "var(--text-muted)";
  }
}

export function AgentsPublishFileDiff({
  file,
  diff,
  conflictDiff,
  isConflictMode = false,
  isExpanded,
  onToggle,
  onCopyPath,
  onOpenFullscreen,
  onRetry,
  conversationId,
  refKind,
  diffPageRefKind,
  diffPageReloadKey,
  inlineDiffScrollParent,
  diffPageSummary,
  shouldHydrate,
  annotations = [],
  hunkAnnotations = [],
  isShowAnywayOverridden,
  onShowAnyway,
  isFocusTarget = false,
}: AgentsPublishFileDiffProps) {
  const diffData = diff !== "loading" && diff !== "error" ? diff : undefined;
  const conflictDiffData =
    conflictDiff !== "loading" && conflictDiff !== "error" ? conflictDiff : undefined;
  const showExplicitPlaceholder =
    !isConflictMode && requiresExplicitDiffHydration(file) && !isShowAnywayOverridden;
  const usePagedDiff = canUsePagedInlineDiff({
    file,
    isConflictMode,
    conversationId,
    diffPageRefKind,
    isShowAnywayOverridden,
  });
  const diffPageIdentity = diffPageRefKind
    ? diffPageRefKind.kind === "commit"
      ? `${diffPageRefKind.kind}:${diffPageRefKind.sha}`
      : diffPageRefKind.kind
    : "none";
  const pathButtonRef = useRef<HTMLButtonElement | null>(null);

  useEffect(() => {
    if (isFocusTarget) {
      pathButtonRef.current?.focus({ preventScroll: true });
    }
  }, [isFocusTarget]);

  return (
    <div
      className="flex min-h-0 min-w-0 w-full max-w-full flex-col overflow-hidden rounded-md border"
      data-testid={`publish-file-diff-${file.path}`}
      style={{
        backgroundColor: "var(--bg-surface)",
        borderColor: "var(--border-subtle)",
        boxShadow: isFocusTarget ? "0 0 0 1px var(--accent-border)" : undefined,
      }}
    >
      {/* Header — always renders synchronously */}
      <div
        className="flex items-center gap-1.5 px-2 py-1.5"
        style={{
          borderBottomWidth: isExpanded ? "1px" : "0",
          borderBottomStyle: "solid",
          borderBottomColor: "var(--border-subtle)",
        }}
      >
        {/* Collapse toggle */}
        <button
          type="button"
          data-testid="file-diff-toggle"
          aria-label={isExpanded ? "Collapse file" : "Expand file"}
          onClick={onToggle}
          className="flex items-center justify-center rounded p-0.5 transition-colors hover:bg-[var(--bg-hover)]"
          style={{ color: "var(--text-muted)" }}
        >
          <ChevronRight
            className={cn(
              "h-3.5 w-3.5 transition-transform duration-150",
              isExpanded && "rotate-90",
            )}
            aria-hidden="true"
          />
        </button>

        {/* Status badge */}
        <span
          data-testid="file-status-badge"
          className="w-4 shrink-0 text-center text-[0.6875rem] font-semibold"
          style={{ color: statusColor(file.status) }}
        >
          {statusLetter(file.status)}
        </span>

        {/* File path — clickable to toggle expand/collapse */}
        <Tooltip>
          <TooltipTrigger asChild>
            <button
              ref={pathButtonRef}
              type="button"
              data-testid="file-diff-path-toggle"
              aria-label={isExpanded ? "Collapse file" : "Expand file"}
              onClick={onToggle}
              className="flex-1 min-w-0 truncate text-left font-mono text-[0.8125rem] outline-none rounded hover:underline focus-visible:[outline:1px_solid_var(--accent-border)] focus-visible:[outline-offset:2px]"
              style={{ color: "var(--text-primary)" }}
            >
              {file.path}
            </button>
          </TooltipTrigger>
          <TooltipContent side="top">
            <p className="font-mono text-xs">{file.path}</p>
          </TooltipContent>
        </Tooltip>

        {/* Copy path — directly after the path */}
        <Tooltip>
          <TooltipTrigger asChild>
            <button
              type="button"
              data-testid="file-diff-copy-path"
              aria-label="Copy file path"
              onClick={() => onCopyPath(file.path)}
              className="flex items-center justify-center rounded p-0.5 transition-colors hover:bg-[var(--bg-hover)]"
              style={{ color: "var(--text-muted)" }}
            >
              <Copy className="h-3.5 w-3.5" aria-hidden="true" />
            </button>
          </TooltipTrigger>
          <TooltipContent side="top">
            <p>Copy path</p>
          </TooltipContent>
        </Tooltip>

        {/* +/− counts */}
        {file.additions > 0 && (
          <span
            className="shrink-0 text-[0.6875rem] font-mono"
            style={{ color: "var(--status-success)" }}
          >
            +{file.additions}
          </span>
        )}
        {file.deletions > 0 && (
          <span
            className="shrink-0 text-[0.6875rem] font-mono"
            style={{ color: "var(--status-error)" }}
          >
            −{file.deletions}
          </span>
        )}

        {annotations.length > 0 && (
          <span
            data-testid="file-diff-annotation-count"
            className="shrink-0 rounded border px-1.5 py-0.5 text-[0.6875rem] font-medium"
            style={{
              borderColor: "var(--status-warning-border)",
              color: "var(--status-warning)",
            }}
          >
            {annotations.length}
          </span>
        )}
        {hunkAnnotations.length > 0 && (
          <span
            data-testid="file-diff-hunk-annotation-count"
            className="shrink-0 rounded border px-1.5 py-0.5 text-[0.6875rem] font-medium"
            style={{
              borderColor: "var(--status-info-border)",
              color: "var(--status-info)",
            }}
          >
            {hunkAnnotations.length} review
          </span>
        )}

        {/* Open fullscreen */}
        <Tooltip>
          <TooltipTrigger asChild>
            <button
              type="button"
              data-testid="file-diff-open-fullscreen"
              aria-label="Open in full diff dialog"
              onClick={() => onOpenFullscreen(file.path)}
              className="flex items-center justify-center rounded p-0.5 transition-colors hover:bg-[var(--bg-hover)]"
              style={{ color: "var(--text-muted)" }}
            >
              <Maximize2 className="h-3.5 w-3.5" aria-hidden="true" />
            </button>
          </TooltipTrigger>
          <TooltipContent side="top">
            <p>Open in full dialog</p>
          </TooltipContent>
        </Tooltip>
      </div>

      {/* Body — only mounted when expanded */}
      {isExpanded && (
        <div
          className="flex min-h-0 flex-col"
          style={{ minHeight: "60px" }}
        >
          {isConflictMode ? (
            conflictDiff === "loading" ? (
              <div className="px-3 py-3">
                <Skeleton className="h-24 w-full" />
              </div>
            ) : conflictDiff === "error" ? (
              <div
                data-testid="file-diff-error"
                className="flex items-center gap-2 px-3 py-4 text-xs"
                style={{ color: "var(--status-error)" }}
              >
                <span>Failed to load conflict diff</span>
                {onRetry && (
                  <button
                    type="button"
                    onClick={onRetry}
                    className="inline-flex items-center gap-1 rounded px-2 py-1 text-xs"
                    style={{
                      backgroundColor: "var(--status-error-muted)",
                      color: "var(--status-error)",
                    }}
                  >
                    <RefreshCw className="h-3 w-3" aria-hidden="true" />
                    Retry
                  </button>
                )}
              </div>
            ) : conflictDiffData ? (
              <ConflictDiffViewer conflictDiff={conflictDiffData} />
            ) : shouldHydrate ? (
              <div className="px-3 py-3">
                <Skeleton className="h-24 w-full" />
              </div>
            ) : null
          ) : showExplicitPlaceholder ? (
            /* Generated-file placeholder — shown until user clicks "Show anyway" */
            <div
              data-testid="file-diff-generated-placeholder"
              className="flex items-center gap-2 px-3 py-4 text-xs"
              style={{ color: "var(--text-muted)" }}
            >
              <span>Generated file</span>
              {file.additions > 0 && (
                <span className="font-mono" style={{ color: "var(--status-success)" }}>
                  +{file.additions}
                </span>
              )}
              {file.deletions > 0 && (
                <span className="font-mono" style={{ color: "var(--status-error)" }}>
                  −{file.deletions}
                </span>
              )}
              <button
                type="button"
                data-testid="file-diff-show-anyway"
                aria-label="Show generated file diff"
                onClick={onShowAnyway}
                className="rounded px-2 py-0.5 text-xs transition-colors hover:bg-[var(--bg-hover)]"
                style={{ color: "var(--text-secondary)" }}
              >
                Show anyway
              </button>
            </div>
          ) : !shouldHydrate ? (
            /* Lightweight frame while the mounted row registers for hydration. */
            <div
              data-testid="file-diff-pre-hydration"
              aria-label="Loading file diff"
              aria-busy="true"
              className="space-y-1 p-3"
              style={{ minHeight: "60px", color: "var(--text-muted)" }}
            >
              <span className="sr-only">Loading diff</span>
              <Skeleton className="h-4 w-3/4" />
              <Skeleton className="h-4 w-full" />
            </div>
          ) : (
            /* Hydrated body — render loading / error / diff / empty states */
            <>
              {diff === "loading" && (
                <div data-testid="file-diff-skeleton" className="space-y-1 p-3">
                  <Skeleton className="h-4 w-3/4" />
                  <Skeleton className="h-4 w-full" />
                  <Skeleton className="h-4 w-2/3" />
                </div>
              )}

              {diff === "error" && (
                <div
                  data-testid="file-diff-error"
                  className="flex flex-col items-center gap-2 py-6 text-xs"
                  style={{ color: "var(--text-muted)" }}
                >
                  <p>Could not load diff for this file.</p>
                  {onRetry !== undefined && (
                    <button
                      type="button"
                      data-testid="file-diff-retry"
                      aria-label="Retry loading diff"
                      onClick={onRetry}
                      className="inline-flex items-center gap-1 rounded px-2 py-1 text-xs transition-colors hover:bg-[var(--bg-hover)]"
                      style={{ color: "var(--text-secondary)" }}
                    >
                      <RefreshCw className="h-3 w-3" aria-hidden="true" />
                      Retry
                    </button>
                  )}
                </div>
              )}

              {usePagedDiff && (
                <PagedDiffView
                  key={[
                    conversationId,
                    file.path,
                    diffPageIdentity,
                    diffPageReloadKey ?? "stable",
                  ].join("\u0000")}
                  conversationId={conversationId!}
                  filePath={file.path}
                  refKind={diffPageRefKind!}
                  annotations={annotations}
                  hunkAnnotations={hunkAnnotations}
                  scrollContainer={false}
                  inlineScrollParent={inlineDiffScrollParent}
                  defaultWrapLines={false}
                  initialTotalRows={diffPageSummary?.totalRows}
                  initialIsBinary={diffPageSummary?.isBinary}
                />
              )}

              {!usePagedDiff && diffData !== undefined && (
                <SimpleDiffView
                  hunks={diffData.hunks}
                  oldTotalLines={diffData.oldTotalLines}
                  newTotalLines={diffData.newTotalLines}
                  isBinary={diffData.isBinary}
                  language={diffData.language}
                  conversationId={conversationId}
                  filePath={file.path}
                  refKind={refKind}
                  scrollContainer={false}
                  stickyGutter={false}
                  annotations={annotations}
                  hunkAnnotations={hunkAnnotations}
                />
              )}

              {!usePagedDiff && diff === undefined && (
                <div
                  className="flex items-center justify-center py-6 text-xs"
                  style={{ color: "var(--text-muted)" }}
                >
                  No diff available
                </div>
              )}
            </>
          )}
        </div>
      )}
    </div>
  );
}
