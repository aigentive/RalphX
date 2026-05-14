/**
 * AgentsPublishFileDiff
 *
 * Per-file collapsible card in the inline diff view.
 * Parent manages diff fetching and passes state via `diff` prop.
 *
 * Performance contract (frontend-interaction-performance.md):
 * - Header (file path, status badge, +/−, buttons) paints synchronously.
 * - Body (SimpleDiffView) only mounts when isExpanded=true AND diff data is available.
 * - Collapsed cards: no SimpleDiffView, no skeleton — zero mount cost.
 *
 * Icon-only buttons (icon-only-buttons.md): aria-label + Tooltip on every icon button.
 * WKWebView CSS: explicit background-color / border-color with shallow-chain tokens.
 */

import { Copy, ChevronRight, Maximize2, RefreshCw } from "lucide-react";
import { cn } from "@/lib/utils";
import { SimpleDiffView } from "@/components/diff/SimpleDiffView";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { Skeleton } from "@/components/ui/skeleton";
import type { FileChange, FileDiff } from "@/api/diff";

export type DiffState = FileDiff | "loading" | "error" | undefined;

export interface AgentsPublishFileDiffProps {
  file: FileChange;
  diff: DiffState;
  isExpanded: boolean;
  onToggle: () => void;
  onCopyPath: (path: string) => void;
  onOpenFullscreen: (path: string) => void;
  onRetry?: (() => void) | undefined;
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
  isExpanded,
  onToggle,
  onCopyPath,
  onOpenFullscreen,
  onRetry,
}: AgentsPublishFileDiffProps) {
  const diffData = diff !== "loading" && diff !== "error" ? diff : undefined;

  return (
    <div
      className="overflow-hidden rounded-md border"
      data-testid={`publish-file-diff-${file.path}`}
      style={{
        backgroundColor: "var(--bg-surface)",
        borderColor: "var(--border-subtle)",
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

        {/* File path */}
        <Tooltip>
          <TooltipTrigger asChild>
            <span
              className="flex-1 truncate font-mono text-[0.8125rem]"
              style={{ color: "var(--text-primary)" }}
            >
              {file.path}
            </span>
          </TooltipTrigger>
          <TooltipContent side="top">
            <p className="font-mono text-xs">{file.path}</p>
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

        {/* Copy path */}
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
        <div style={{ minHeight: "60px", maxHeight: "480px", overflowY: "auto" }}>
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

          {diffData !== undefined && (
            <SimpleDiffView
              oldContent={diffData.oldContent}
              newContent={diffData.newContent}
              language={diffData.language}
            />
          )}

          {diff === undefined && (
            <div
              className="flex items-center justify-center py-6 text-xs"
              style={{ color: "var(--text-muted)" }}
            >
              No diff available
            </div>
          )}
        </div>
      )}
    </div>
  );
}
