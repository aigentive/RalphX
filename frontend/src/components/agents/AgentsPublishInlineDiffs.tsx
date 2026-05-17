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
 *   - Diffs fetched for hydrated expanded files only; off-range/collapsed cards pay zero query cost.
 *
 * Performance contract (frontend-interaction-performance.md):
 *   - Sticky bar always renders synchronously.
 *   - File cards receive diff as prop — parent manages timing, cards never fetch.
 *
 * WKWebView CSS: explicit background-color / border-color with shallow-chain tokens.
 */

import { memo, useState, useCallback, useMemo, useRef, useEffect } from "react";
import { useQueries, useQuery } from "@tanstack/react-query";
import { ArrowDownToLine, ChevronDown, ChevronUp } from "lucide-react";
import { Virtuoso, type ListRange, type VirtuosoHandle } from "react-virtuoso";
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
import {
  AGENT_WORKSPACE_STALE_MS,
  agentWorkspaceKeys,
} from "./agentWorkspaceQueries";

const EMPTY_PR_DIFF_ANNOTATIONS: PrDiffAnnotation[] = [];
const VIRTUAL_RANGE_OVERSCAN_FILES = 0;

export interface AgentsPublishInlineDiffsProps {
  conversationId: string;
  review: AgentWorkspaceReview | null;
  commits: DiffViewerCommit[];
  isLoading: boolean;
  annotations?: PrDiffAnnotation[] | undefined;
  error?: unknown;
  onOpenInDialog?: ((filePath: string) => void) | undefined;
}

interface AgentsPublishVirtualFileRowProps {
  file: FileChange;
  diff: DiffState;
  isExpanded: boolean;
  onTogglePath: (path: string) => void;
  onCopyPath: (path: string) => void;
  onOpenFullscreenPath: (path: string) => void;
  conversationId: string;
  refKind: DiffRefKind;
  shouldHydrate: boolean;
  annotations: PrDiffAnnotation[];
  isShowAnywayOverridden: boolean;
  onShowAnywayPath: (path: string) => void;
}

const AgentsPublishVirtualFileRow = memo(function AgentsPublishVirtualFileRow({
  file,
  diff,
  isExpanded,
  onTogglePath,
  onCopyPath,
  onOpenFullscreenPath,
  conversationId,
  refKind,
  shouldHydrate,
  annotations,
  isShowAnywayOverridden,
  onShowAnywayPath,
}: AgentsPublishVirtualFileRowProps) {
  const handleToggle = useCallback(() => {
    onTogglePath(file.path);
  }, [file.path, onTogglePath]);

  const handleShowAnyway = useCallback(() => {
    onShowAnywayPath(file.path);
  }, [file.path, onShowAnywayPath]);

  return (
    <AgentsPublishFileDiff
      file={file}
      diff={diff}
      isExpanded={isExpanded}
      onToggle={handleToggle}
      onCopyPath={onCopyPath}
      onOpenFullscreen={onOpenFullscreenPath}
      conversationId={conversationId}
      refKind={refKind}
      shouldHydrate={shouldHydrate}
      annotations={annotations}
      isShowAnywayOverridden={isShowAnywayOverridden}
      onShowAnyway={handleShowAnyway}
    />
  );
});

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
  const virtuosoRef = useRef<VirtuosoHandle>(null);
  const [visibleRange, setVisibleRange] = useState<ListRange | null>(null);
  // Lazy hydration tracks which file paths have entered the virtual range.
  // Paths are added on first range entry and never removed, so body teardown does not thrash.
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
    queryKey: [...agentWorkspaceKeys.diff(conversationId), "commit-files", commitSha],
    queryFn: () => {
      if (!commitSha) throw new Error("commitSha required");
      return diffApi.getAgentConversationWorkspaceCommitFileChanges(conversationId, commitSha);
    },
    enabled: isCommitMode && Boolean(commitSha),
    staleTime: AGENT_WORKSPACE_STALE_MS,
  });

  // ── Staged file list (only active in staged mode) ──────────────────────
  const stagedFilesQuery = useQuery({
    queryKey: [...agentWorkspaceKeys.diff(conversationId), "staged-files"],
    queryFn: () => diffApi.getAgentConversationWorkspaceStagedFileChanges(conversationId),
    enabled: supportsWorktreeModes && isStagedMode,
    staleTime: AGENT_WORKSPACE_STALE_MS,
  });

  // ── Unstaged file list (only active in unstaged mode) ─────────────────
  const unstagedFilesQuery = useQuery({
    queryKey: [...agentWorkspaceKeys.diff(conversationId), "unstaged-files"],
    queryFn: () => diffApi.getAgentConversationWorkspaceUnstagedFileChanges(conversationId),
    enabled: supportsWorktreeModes && isUnstagedMode,
    staleTime: AGENT_WORKSPACE_STALE_MS,
  });

  // ── Cumulative file list (only active in cumulative mode) ─────────────
  const cumulativeFilesQuery = useQuery({
    queryKey: [...agentWorkspaceKeys.diff(conversationId), "cumulative-files"],
    queryFn: () => diffApi.getAgentConversationWorkspaceCumulativeFileChanges(conversationId),
    enabled: isCumulativeMode,
    staleTime: AGENT_WORKSPACE_STALE_MS,
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
    setVisibleRange(null);
  }, [conversationId, effectiveMode]);

  const bufferedVisiblePathSet = useMemo(() => {
    const paths = new Set<string>();
    if (!visibleRange || currentFiles.length === 0) {
      return paths;
    }

    const start = Math.max(0, visibleRange.startIndex - VIRTUAL_RANGE_OVERSCAN_FILES);
    const end = Math.min(
      currentFiles.length - 1,
      visibleRange.endIndex + VIRTUAL_RANGE_OVERSCAN_FILES,
    );

    for (let index = start; index <= end; index += 1) {
      const path = currentFiles[index]?.path;
      if (path) {
        paths.add(path);
      }
    }

    return paths;
  }, [currentFiles, visibleRange]);

  const hydrateVisibleRange = useCallback(
    (range: ListRange) => {
      setVisibleRange(range);
      setHydratedPaths((prev) => {
        let changed = false;
        const next = new Set(prev);
        const start = Math.max(0, range.startIndex - VIRTUAL_RANGE_OVERSCAN_FILES);
        const end = Math.min(
          currentFiles.length - 1,
          range.endIndex + VIRTUAL_RANGE_OVERSCAN_FILES,
        );

        for (let index = start; index <= end; index += 1) {
          const path = currentFiles[index]?.path;
          if (path && !prev.has(path)) {
            next.add(path);
            changed = true;
          }
        }

        return changed ? next : prev;
      });
    },
    [currentFiles],
  );

  // Only fetch diffs for visible expanded files — collapsed/off-range cards pay no query cost.
  const expandedFiles = useMemo(
    () => currentFiles.filter((f) => !collapsedPaths.has(f.path)),
    [currentFiles, collapsedPaths],
  );

  const fetchableFiles = useMemo(
    () =>
      expandedFiles.filter(
        (file) =>
          bufferedVisiblePathSet.has(file.path) &&
          (!file.isGenerated || userShowAnywayPaths.has(file.path)),
      ),
    [bufferedVisiblePathSet, expandedFiles, userShowAnywayPaths],
  );

  // ── Uncommitted diffs ─────────────────────────────────────────────────
  const uncommittedDiffQueries = useQueries({
    queries: (!isCommitMode && !isStagedMode && !isUnstagedMode && !isCumulativeMode
      ? fetchableFiles
      : []
    ).map((file) => ({
      queryKey: [...agentWorkspaceKeys.diff(conversationId), "uncommitted", file.path],
      queryFn: () => diffApi.getAgentConversationWorkspaceFileDiff(conversationId, file.path),
      staleTime: AGENT_WORKSPACE_STALE_MS,
    })),
  });

  // ── Commit diffs ──────────────────────────────────────────────────────
  const commitDiffQueries = useQueries({
    queries: (isCommitMode && commitSha ? fetchableFiles : []).map((file) => ({
      queryKey: [...agentWorkspaceKeys.diff(conversationId), "commit", commitSha, file.path],
      queryFn: () => {
        if (!commitSha) throw new Error("commitSha required");
        return diffApi.getAgentConversationWorkspaceCommitFileDiff(
          conversationId,
          commitSha,
          file.path,
        );
      },
      staleTime: AGENT_WORKSPACE_STALE_MS,
    })),
  });

  // ── Staged diffs ──────────────────────────────────────────────────────
  const stagedDiffQueries = useQueries({
    queries: (supportsWorktreeModes && isStagedMode ? fetchableFiles : []).map((file) => ({
      queryKey: [...agentWorkspaceKeys.diff(conversationId), "staged", file.path],
      queryFn: () =>
        diffApi.getAgentConversationWorkspaceStagedFileDiff(conversationId, file.path),
      staleTime: AGENT_WORKSPACE_STALE_MS,
    })),
  });

  // ── Unstaged diffs ────────────────────────────────────────────────────
  const unstagedDiffQueries = useQueries({
    queries: (supportsWorktreeModes && isUnstagedMode ? fetchableFiles : []).map((file) => ({
      queryKey: [...agentWorkspaceKeys.diff(conversationId), "unstaged", file.path],
      queryFn: () =>
        diffApi.getAgentConversationWorkspaceUnstagedFileDiff(conversationId, file.path),
      staleTime: AGENT_WORKSPACE_STALE_MS,
    })),
  });

  // ── Cumulative diffs ──────────────────────────────────────────────────
  const cumulativeDiffQueries = useQueries({
    queries: (isCumulativeMode ? fetchableFiles : []).map((file) => ({
      queryKey: [...agentWorkspaceKeys.diff(conversationId), "cumulative", file.path],
      queryFn: () =>
        diffApi.getAgentConversationWorkspaceCumulativeFileDiff(conversationId, file.path),
      staleTime: AGENT_WORKSPACE_STALE_MS,
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
    fetchableFiles.forEach((file, idx) => {
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
    fetchableFiles,
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

  const handleCopyPath = useCallback((path: string) => {
    void navigator.clipboard?.writeText(path).catch(() => undefined);
  }, []);

  const handleOpenFullscreen = useCallback(
    (path: string) => {
      onOpenInDialog?.(path);
    },
    [onOpenInDialog],
  );

  const handleShowAnyway = useCallback((path: string) => {
    setUserShowAnywayPaths((prev) => {
      if (prev.has(path)) {
        return prev;
      }
      return new Set([...prev, path]);
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
    const index = currentFiles.findIndex((file) => file.path === path);
    if (index < 0) {
      return;
    }

    hydrateVisibleRange({ startIndex: index, endIndex: index });
    virtuosoRef.current?.scrollToIndex({
      index,
      align: "start",
      behavior: "auto",
    });
  }, [currentFiles, hydrateVisibleRange]);

  const computeFileKey = useCallback(
    (_index: number, file: FileChange) => file.path,
    [],
  );

  const renderFileRow = useCallback(
    (_index: number, fileChange: FileChange) => (
      <div className="py-1 first:pt-3 last:pb-3">
        <AgentsPublishVirtualFileRow
          file={fileChange}
          diff={diffByPath.get(fileChange.path)}
          isExpanded={!collapsedPaths.has(fileChange.path)}
          onTogglePath={handleToggle}
          onCopyPath={handleCopyPath}
          onOpenFullscreenPath={handleOpenFullscreen}
          conversationId={conversationId}
          refKind={refKind}
          shouldHydrate={hydratedPaths.has(fileChange.path)}
          annotations={annotationsByPath.get(fileChange.path) ?? EMPTY_PR_DIFF_ANNOTATIONS}
          isShowAnywayOverridden={userShowAnywayPaths.has(fileChange.path)}
          onShowAnywayPath={handleShowAnyway}
        />
      </div>
    ),
    [
      annotationsByPath,
      collapsedPaths,
      conversationId,
      diffByPath,
      handleCopyPath,
      handleOpenFullscreen,
      handleShowAnyway,
      handleToggle,
      hydratedPaths,
      refKind,
      userShowAnywayPaths,
    ],
  );

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
        <Virtuoso
          ref={virtuosoRef}
          data={currentFiles}
          data-testid="inline-diffs-virtual-list"
          className="min-h-0 flex-1 px-3"
          style={{ height: "100%" }}
          computeItemKey={computeFileKey}
          rangeChanged={hydrateVisibleRange}
          increaseViewportBy={{ top: 240, bottom: 480 }}
          itemContent={renderFileRow}
        />
      )}
    </div>
  );
}
