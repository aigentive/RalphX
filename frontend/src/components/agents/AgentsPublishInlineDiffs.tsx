/**
 * AgentsPublishInlineDiffs
 *
 * Container that orchestrates fetch-management + filter + file cards.
 * Receives (conversationId, review, commits) from parent — no re-fetching at parent level.
 *
 * Fetch contract:
 *   - Uncommitted mode: per-file diff via getAgentConversationWorkspaceFileDiff
 *   - Commit mode: file list via getAgentConversationWorkspaceCommitFileChanges,
 *                  then per-file diff via getAgentConversationWorkspaceCommitFileDiff
 *   - Diffs fetched for expanded files only; collapsed cards pay zero query cost.
 *
 * Performance contract (frontend-interaction-performance.md):
 *   - Sticky bar always renders synchronously.
 *   - File cards receive diff as prop — parent manages timing, cards never fetch.
 *
 * WKWebView CSS: explicit background-color / border-color with shallow-chain tokens.
 */

import { useState, useCallback, useMemo, useRef, useEffect } from "react";
import { useQueries, useQuery } from "@tanstack/react-query";
import { ArrowDownToLine, ChevronDown, ChevronUp } from "lucide-react";
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover";
import { Input } from "@/components/ui/input";
import { Skeleton } from "@/components/ui/skeleton";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { diffApi } from "@/api/diff";
import type { AgentWorkspaceReview, FileChange, DiffRefKind, PrDiffAnnotation } from "@/api/diff";
import type { Commit as DiffViewerCommit } from "@/components/diff";
import { AgentsPublishDiffFilter } from "./AgentsPublishDiffFilter";
import type { DiffFilterMode } from "./AgentsPublishDiffFilter";
import { AgentsPublishFileDiff } from "./AgentsPublishFileDiff";
import type { DiffState } from "./AgentsPublishFileDiff";

export interface AgentsPublishInlineDiffsProps {
  conversationId: string;
  review: AgentWorkspaceReview | null;
  commits: DiffViewerCommit[];
  isLoading: boolean;
  annotations?: PrDiffAnnotation[] | undefined;
  error?: unknown;
  onOpenInDialog?: ((filePath: string) => void) | undefined;
}

export function AgentsPublishInlineDiffs({
  conversationId,
  review,
  commits,
  isLoading,
  annotations = [],
  error,
  onOpenInDialog,
}: AgentsPublishInlineDiffsProps) {
  const [mode, setMode] = useState<DiffFilterMode>("uncommitted");
  // Set of collapsed file paths; empty = all expanded (default).
  const [collapsedPaths, setCollapsedPaths] = useState<Set<string>>(new Set());
  // Jump-to-file popover
  const [jumpOpen, setJumpOpen] = useState(false);
  const [jumpSearch, setJumpSearch] = useState("");
  // Refs for scrolling to file cards on jump
  const cardRefs = useRef<Map<string, HTMLElement | null>>(new Map());
  // Lazy hydration — tracks which file paths have entered the viewport (±200px).
  // Paths are added on first intersection and never removed (avoids teardown thrash).
  const [hydratedPaths, setHydratedPaths] = useState<Set<string>>(new Set());
  // Show-anyway overrides — paths where the user has dismissed the generated-file placeholder.
  const [userShowAnywayPaths, setUserShowAnywayPaths] = useState<Set<string>>(new Set());

  const supportsWorktreeModes = review?.supportsWorktreeModes ?? true;
  const effectiveMode =
    !supportsWorktreeModes &&
    (mode === "uncommitted" || mode === "staged" || mode === "unstaged")
      ? "cumulative"
      : mode;
  const isStagedMode = effectiveMode === "staged";
  const isUnstagedMode = effectiveMode === "unstaged";
  const isCumulativeMode = effectiveMode === "cumulative";
  const isCommitMode =
    effectiveMode !== "uncommitted" &&
    !isStagedMode &&
    !isUnstagedMode &&
    !isCumulativeMode;
  const commitSha = isCommitMode ? effectiveMode : undefined;

  /** Map the current mode to the backend DiffRefKind for range fetches. */
  const refKind = useMemo<DiffRefKind>(() => {
    if (isStagedMode) return { kind: "staged" };
    if (isUnstagedMode) return { kind: "unstaged" };
    if (isCumulativeMode) return { kind: "cumulative_head" };
    if (isCommitMode && commitSha !== undefined) return { kind: "commit", sha: commitSha };
    return { kind: "head" }; // uncommitted = diff vs HEAD
  }, [isStagedMode, isUnstagedMode, isCumulativeMode, isCommitMode, commitSha]);
  const canRenderPrAnnotations = refKind.kind === "head" || refKind.kind === "cumulative_head";
  const annotationsByPath = useMemo(() => {
    const map = new Map<string, PrDiffAnnotation[]>();
    if (!canRenderPrAnnotations) {
      return map;
    }
    for (const annotation of annotations) {
      if (!annotation.path) continue;
      const existing = map.get(annotation.path);
      if (existing) {
        existing.push(annotation);
      } else {
        map.set(annotation.path, [annotation]);
      }
    }
    return map;
  }, [annotations, canRenderPrAnnotations]);

  // ── Commit file list (only active in commit mode) ──────────────────────
  const commitFilesQuery = useQuery({
    queryKey: ["commit-files", conversationId, commitSha],
    queryFn: () => {
      if (!commitSha) throw new Error("commitSha required");
      return diffApi.getAgentConversationWorkspaceCommitFileChanges(conversationId, commitSha);
    },
    enabled: isCommitMode && Boolean(commitSha),
  });

  // ── Staged file list (only active in staged mode) ──────────────────────
  const stagedFilesQuery = useQuery({
    queryKey: ["staged-files", conversationId],
    queryFn: () => diffApi.getAgentConversationWorkspaceStagedFileChanges(conversationId),
    enabled: supportsWorktreeModes && isStagedMode,
  });

  // ── Unstaged file list (only active in unstaged mode) ─────────────────
  const unstagedFilesQuery = useQuery({
    queryKey: ["unstaged-files", conversationId],
    queryFn: () => diffApi.getAgentConversationWorkspaceUnstagedFileChanges(conversationId),
    enabled: supportsWorktreeModes && isUnstagedMode,
  });

  // ── Cumulative file list (only active in cumulative mode) ─────────────
  const cumulativeFilesQuery = useQuery({
    queryKey: ["cumulative-files", conversationId],
    queryFn: () => diffApi.getAgentConversationWorkspaceCumulativeFileChanges(conversationId),
    enabled: isCumulativeMode,
  });

  // ── Current file list (mode-dependent) ────────────────────────────────
  const currentFiles = useMemo<FileChange[]>(() => {
    if (isCommitMode) return commitFilesQuery.data ?? [];
    if (isStagedMode && supportsWorktreeModes) return stagedFilesQuery.data ?? [];
    if (isUnstagedMode && supportsWorktreeModes) return unstagedFilesQuery.data ?? [];
    if (isCumulativeMode) return cumulativeFilesQuery.data ?? review?.changes ?? [];
    return review?.changes ?? [];
  }, [
    isCommitMode,
    isStagedMode,
    isUnstagedMode,
    isCumulativeMode,
    supportsWorktreeModes,
    commitFilesQuery.data,
    stagedFilesQuery.data,
    unstagedFilesQuery.data,
    cumulativeFilesQuery.data,
    review,
  ]);

  // ── Mode change → reset hydrated set (new mode = new file list) ─────────
  useEffect(() => {
    setHydratedPaths(new Set());
  }, [effectiveMode]);

  // ── IntersectionObserver — lazy hydration ─────────────────────────────
  useEffect(() => {
    if (typeof IntersectionObserver === "undefined") return;
    if (currentFiles.length === 0) return;

    const observer = new IntersectionObserver(
      (entries) => {
        setHydratedPaths((prev) => {
          let changed = false;
          const next = new Set(prev);
          for (const entry of entries) {
            if (entry.isIntersecting) {
              const path = (entry.target as HTMLElement).dataset["filePath"];
              if (path !== undefined && !prev.has(path)) {
                next.add(path);
                changed = true;
              }
            }
          }
          return changed ? next : prev;
        });
      },
      { rootMargin: "200px" },
    );

    for (const f of currentFiles) {
      const el = cardRefs.current.get(f.path);
      if (el) {
        observer.observe(el);
      }
    }

    return () => {
      observer.disconnect();
    };
  }, [currentFiles]);

  // Only fetch diffs for expanded files — collapsed cards pay no query cost.
  const expandedFiles = useMemo(
    () => currentFiles.filter((f) => !collapsedPaths.has(f.path)),
    [currentFiles, collapsedPaths],
  );

  // ── Uncommitted diffs ─────────────────────────────────────────────────
  const uncommittedDiffQueries = useQueries({
    queries: (!isCommitMode && !isStagedMode && !isUnstagedMode && !isCumulativeMode
      ? expandedFiles
      : []
    ).map((file) => ({
      queryKey: ["uncommitted-diff", conversationId, file.path],
      queryFn: () => diffApi.getAgentConversationWorkspaceFileDiff(conversationId, file.path),
    })),
  });

  // ── Commit diffs ──────────────────────────────────────────────────────
  const commitDiffQueries = useQueries({
    queries: (isCommitMode && commitSha ? expandedFiles : []).map((file) => ({
      queryKey: ["commit-diff", conversationId, commitSha, file.path],
      queryFn: () => {
        if (!commitSha) throw new Error("commitSha required");
        return diffApi.getAgentConversationWorkspaceCommitFileDiff(
          conversationId,
          commitSha,
          file.path,
        );
      },
    })),
  });

  // ── Staged diffs ──────────────────────────────────────────────────────
  const stagedDiffQueries = useQueries({
    queries: (supportsWorktreeModes && isStagedMode ? expandedFiles : []).map((file) => ({
      queryKey: ["staged-diff", conversationId, file.path],
      queryFn: () =>
        diffApi.getAgentConversationWorkspaceStagedFileDiff(conversationId, file.path),
    })),
  });

  // ── Unstaged diffs ────────────────────────────────────────────────────
  const unstagedDiffQueries = useQueries({
    queries: (supportsWorktreeModes && isUnstagedMode ? expandedFiles : []).map((file) => ({
      queryKey: ["unstaged-diff", conversationId, file.path],
      queryFn: () =>
        diffApi.getAgentConversationWorkspaceUnstagedFileDiff(conversationId, file.path),
    })),
  });

  // ── Cumulative diffs ──────────────────────────────────────────────────
  const cumulativeDiffQueries = useQueries({
    queries: (isCumulativeMode ? expandedFiles : []).map((file) => ({
      queryKey: ["cumulative-diff", conversationId, file.path],
      queryFn: () =>
        diffApi.getAgentConversationWorkspaceCumulativeFileDiff(conversationId, file.path),
    })),
  });

  // ── Map path → DiffState for card props ───────────────────────────────
  const diffByPath = useMemo(() => {
    const map = new Map<string, DiffState>();
    const activeQueries = isCommitMode
      ? commitDiffQueries
      : isStagedMode
        ? stagedDiffQueries
        : isUnstagedMode
          ? unstagedDiffQueries
          : isCumulativeMode
            ? cumulativeDiffQueries
            : uncommittedDiffQueries;
    expandedFiles.forEach((file, idx) => {
      const q = activeQueries[idx];
      if (!q) return;
      if (q.isPending) {
        map.set(file.path, "loading");
      } else if (q.isError) {
        map.set(file.path, "error");
      } else if (q.data !== undefined) {
        map.set(file.path, q.data);
      }
    });
    return map;
  }, [
    isCommitMode,
    isStagedMode,
    isUnstagedMode,
    isCumulativeMode,
    uncommittedDiffQueries,
    commitDiffQueries,
    stagedDiffQueries,
    unstagedDiffQueries,
    cumulativeDiffQueries,
    expandedFiles,
  ]);

  // ── Jump-to-file filtered list ────────────────────────────────────────
  const filteredJumpFiles = useMemo(() => {
    if (!jumpSearch.trim()) return currentFiles;
    const q = jumpSearch.toLowerCase();
    return currentFiles.filter((f) => f.path.toLowerCase().includes(q));
  }, [currentFiles, jumpSearch]);

  // ── Handlers ──────────────────────────────────────────────────────────
  const handleModeChange = useCallback((next: DiffFilterMode) => {
    setMode(next);
  }, []);

  const handleToggle = useCallback((path: string) => {
    setCollapsedPaths((prev) => {
      const next = new Set(prev);
      if (next.has(path)) {
        next.delete(path);
      } else {
        next.add(path);
      }
      return next;
    });
  }, []);

  const collapseAll = useCallback(() => {
    setCollapsedPaths(new Set(currentFiles.map((f) => f.path)));
  }, [currentFiles]);

  const expandAll = useCallback(() => {
    setCollapsedPaths(new Set());
  }, []);

  const handleJumpToFile = useCallback((path: string) => {
    setJumpOpen(false);
    setJumpSearch("");
    const el = cardRefs.current.get(path);
    el?.scrollIntoView({ block: "start", behavior: "smooth" });
  }, []);

  // ── Derived counts ────────────────────────────────────────────────────
  // uncommittedCount always reflects the review's changes (not current-mode files)
  const uncommittedCount = review?.changes.length ?? 0;
  const totalAdditions = currentFiles.reduce((sum, f) => sum + (f.additions ?? 0), 0);
  const totalDeletions = currentFiles.reduce((sum, f) => sum + (f.deletions ?? 0), 0);

  return (
    <div
      data-testid="agents-publish-inline-diffs"
      className="flex min-h-0 flex-1 flex-col"
    >
      {/* Sticky bar — always renders synchronously */}
      <div
        data-testid="inline-diffs-sticky-bar"
        className="sticky top-0 z-10 flex flex-wrap items-center gap-2 rounded-t-lg border-b px-3 py-2"
        style={{
          backgroundColor: "var(--bg-surface)",
          borderColor: "var(--border-subtle)",
        }}
      >
        <AgentsPublishDiffFilter
          mode={effectiveMode}
          uncommittedCount={uncommittedCount}
          {...(stagedFilesQuery.data !== undefined && {
            stagedCount: stagedFilesQuery.data.length,
          })}
          {...(unstagedFilesQuery.data !== undefined && {
            unstagedCount: unstagedFilesQuery.data.length,
          })}
          commits={commits}
          supportsWorktreeModes={supportsWorktreeModes}
          onModeChange={handleModeChange}
        />

        <div className="ml-auto flex items-center gap-2">
          {/* File count */}
          <span className="text-[0.6875rem] font-medium" style={{ color: "var(--text-secondary)" }}>
            <span data-testid="inline-diffs-file-count">{currentFiles.length}</span>{" "}
            {currentFiles.length === 1 ? "file" : "files"}
          </span>

          {/* +/− totals */}
          {totalAdditions > 0 && (
            <span
              data-testid="inline-diffs-additions"
              className="font-mono text-[0.6875rem] font-medium"
              style={{ color: "var(--status-success)" }}
            >
              +{totalAdditions}
            </span>
          )}
          {totalDeletions > 0 && (
            <span
              data-testid="inline-diffs-deletions"
              className="font-mono text-[0.6875rem] font-medium"
              style={{ color: "var(--status-error)" }}
            >
              −{totalDeletions}
            </span>
          )}

          {/* Collapse all */}
          <Tooltip>
            <TooltipTrigger asChild>
              <button
                type="button"
                data-testid="inline-diffs-collapse-all"
                aria-label="Collapse all files"
                onClick={collapseAll}
                className="flex items-center justify-center rounded p-1 transition-colors hover:bg-[var(--bg-hover)]"
                style={{ color: "var(--text-muted)" }}
              >
                <ChevronUp className="h-3.5 w-3.5" aria-hidden="true" />
              </button>
            </TooltipTrigger>
            <TooltipContent side="top">
              <p>Collapse all</p>
            </TooltipContent>
          </Tooltip>

          {/* Expand all */}
          <Tooltip>
            <TooltipTrigger asChild>
              <button
                type="button"
                data-testid="inline-diffs-expand-all"
                aria-label="Expand all files"
                onClick={expandAll}
                className="flex items-center justify-center rounded p-1 transition-colors hover:bg-[var(--bg-hover)]"
                style={{ color: "var(--text-muted)" }}
              >
                <ChevronDown className="h-3.5 w-3.5" aria-hidden="true" />
              </button>
            </TooltipTrigger>
            <TooltipContent side="top">
              <p>Expand all</p>
            </TooltipContent>
          </Tooltip>

          {/* Jump to file — rightmost, icon-only + Tooltip + Popover */}
          <Popover
            open={jumpOpen}
            onOpenChange={(o) => {
              setJumpOpen(o);
              if (!o) setJumpSearch("");
            }}
            modal={false}
          >
            <Tooltip>
              <TooltipTrigger asChild>
                <PopoverTrigger asChild>
                  <button
                    type="button"
                    data-testid="inline-diffs-jump-to-file"
                    aria-label="Jump to file"
                    className="flex items-center justify-center rounded p-1 transition-colors hover:bg-[var(--bg-hover)]"
                    style={{ color: "var(--text-muted)" }}
                  >
                    <ArrowDownToLine className="h-3.5 w-3.5" aria-hidden="true" />
                  </button>
                </PopoverTrigger>
              </TooltipTrigger>
              <TooltipContent side="top">
                <p>Jump to file</p>
              </TooltipContent>
            </Tooltip>
            <PopoverContent
              data-testid="jump-to-file-popover"
              align="end"
              className="w-64 px-1.5 py-2"
              style={{
                backgroundColor: "var(--bg-elevated)",
                borderColor: "var(--border-subtle)",
                borderStyle: "solid",
                borderWidth: "1px",
                boxShadow: "var(--shadow-sm)",
              }}
            >
              <Input
                data-testid="jump-to-file-search"
                placeholder="Filter files…"
                value={jumpSearch}
                onChange={(e) => setJumpSearch(e.target.value)}
                className="mb-1.5 h-7 text-xs"
              />
              <div className="max-h-44 overflow-y-auto">
                {filteredJumpFiles.length === 0 ? (
                  <p
                    className="px-1.5 py-2 text-center text-xs"
                    style={{ color: "var(--text-muted)" }}
                  >
                    No matching files
                  </p>
                ) : (
                  filteredJumpFiles.map((f) => (
                    <button
                      key={f.path}
                      type="button"
                      data-testid={`jump-to-file-item-${f.path}`}
                      onClick={() => handleJumpToFile(f.path)}
                      className="flex w-full items-center rounded px-1.5 py-1 text-left text-xs transition-colors hover:bg-[var(--overlay-weak)]"
                      style={{ color: "var(--text-secondary)" }}
                    >
                      <span className="truncate font-mono text-[0.6875rem]">{f.path}</span>
                    </button>
                  ))
                )}
              </div>
            </PopoverContent>
          </Popover>
        </div>
      </div>

      {/* Body */}
      {isLoading ? (
        <div data-testid="inline-diffs-loading" className="flex flex-col gap-2 p-3">
          {[1, 2, 3].map((i) => (
            <Skeleton key={i} className="h-10 w-full rounded-md" />
          ))}
        </div>
      ) : error ? (
        <div
          data-testid="inline-diffs-error"
          className="flex flex-col items-center justify-center px-4 py-12 text-center"
          style={{ color: "var(--text-muted)" }}
        >
          <p className="text-sm">Could not load workspace changes</p>
          <p className="mt-1 max-w-xl text-xs" style={{ color: "var(--text-muted)" }}>
            {error instanceof Error ? error.message : String(error)}
          </p>
        </div>
      ) : currentFiles.length === 0 ? (
        <div
          data-testid="inline-diffs-empty"
          className="flex flex-col items-center justify-center py-12 text-center"
          style={{ color: "var(--text-muted)" }}
        >
          <p className="text-sm">No changed files</p>
          <p className="mt-1 text-xs" style={{ color: "var(--text-muted)" }}>
            No changes detected in this workspace.
          </p>
        </div>
      ) : (
        <div className="flex min-h-0 flex-1 flex-col gap-2 overflow-y-auto p-3">
          {currentFiles.map((fileChange) => (
            <div
              key={fileChange.path}
              className="flex flex-col last:min-h-0 last:flex-1"
              data-file-path={fileChange.path}
              ref={(el: HTMLDivElement | null) => {
                if (el) {
                  cardRefs.current.set(fileChange.path, el);
                } else {
                  cardRefs.current.delete(fileChange.path);
                }
              }}
            >
              <AgentsPublishFileDiff
                file={fileChange}
                diff={diffByPath.get(fileChange.path)}
                isExpanded={!collapsedPaths.has(fileChange.path)}
                onToggle={() => handleToggle(fileChange.path)}
                onCopyPath={(path) => {
                  void navigator.clipboard?.writeText(path).catch(() => undefined);
                }}
                onOpenFullscreen={(path) => onOpenInDialog?.(path)}
                conversationId={conversationId}
                refKind={refKind}
                shouldHydrate={hydratedPaths.has(fileChange.path)}
                annotations={annotationsByPath.get(fileChange.path) ?? []}
                isShowAnywayOverridden={userShowAnywayPaths.has(fileChange.path)}
                onShowAnyway={() => {
                  setUserShowAnywayPaths((prev) => new Set([...prev, fileChange.path]));
                }}
              />
            </div>
          ))}
        </div>
      )}
    </div>
  );
}
