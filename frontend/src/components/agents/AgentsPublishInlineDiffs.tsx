/**
 * AgentsPublishInlineDiffs
 *
 * Container that orchestrates fetch-management + filter + file cards.
 * Receives (conversationId, review, commits) from parent — no re-fetching at parent level.
 *
 * Fetch contract:
 *   - Workspace changes mode: per-file diff via getAgentConversationWorkspaceFileDiff
 *   - Staged/unstaged modes: lightweight file lists are prefetched for counts and
 *                             default view selection; file diffs still hydrate lazily
 *   - Commit mode: file list via getAgentConversationWorkspaceCommitFileChanges,
 *                  then per-file diff via getAgentConversationWorkspaceCommitFileDiff
 *   - Normal diffs fetched for hydrated expanded files only; large diffs fetch row pages.
 *
 * Performance contract (frontend-interaction-performance.md):
 *   - Sticky bar always renders synchronously.
 *   - File cards receive normal diff state as prop; large explicit diffs page their own rows.
 *
 * WKWebView CSS: explicit background-color / border-color with shallow-chain tokens.
 */

import { memo, useState, useCallback, useMemo, useRef, useEffect } from "react";
import { useQueries } from "@tanstack/react-query";
import { ArrowDownToLine, ChevronDown, ChevronUp, Maximize2 } from "lucide-react";
import { Virtuoso, type ListRange, type VirtuosoHandle } from "react-virtuoso";
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover";
import { Input } from "@/components/ui/input";
import { Skeleton } from "@/components/ui/skeleton";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { diffApi } from "@/api/diff";
import type {
  AgentWorkspaceChangeSummary,
  AgentWorkspaceReview,
  FileChange,
  DiffRefKind,
  PrDiffAnnotation,
} from "@/api/diff";
import type { Commit as DiffViewerCommit } from "@/components/diff";
import { cn } from "@/lib/utils";
import { AgentsPublishDiffFilter } from "./AgentsPublishDiffFilter";
import type { DiffFilterMode } from "./AgentsPublishDiffFilter";
import { AgentsPublishFileDiff } from "./AgentsPublishFileDiff";
import type { ConflictDiffState, DiffState } from "./AgentsPublishFileDiff";
import { isLargeInlineDiff } from "./inlineDiffGuards";
import {
  AGENT_WORKSPACE_STALE_MS,
  agentWorkspaceKeys,
} from "./agentWorkspaceQueries";
import type { AgentPublishFocusRequest } from "./agentPublishFocus";
import { useAgentWorkspaceChangeSummary } from "./useAgentWorkspaceChangeSummary";

const EMPTY_PR_DIFF_ANNOTATIONS: PrDiffAnnotation[] = [];
const VIRTUAL_RANGE_OVERSCAN_FILES = 0;
const PATCH_BACKED_HEAD_REF_PREFIX = "github-pr-diff/";
type BulkExpansionPreference = "expanded" | "collapsed" | "custom";

export interface AgentsPublishInlineDiffsProps {
  conversationId: string;
  review: AgentWorkspaceReview | null;
  commits: DiffViewerCommit[];
  isLoading: boolean;
  annotations?: PrDiffAnnotation[] | undefined;
  error?: unknown;
  onOpenInDialog?: ((filePath?: string) => void) | undefined;
  focusRequest?: AgentPublishFocusRequest | null | undefined;
  defaultMode?: DiffFilterMode | undefined;
  workspaceChangeLabel?: string | undefined;
  liveSummary?: AgentWorkspaceChangeSummary | null | undefined;
  repairMode?: boolean | undefined;
}

function getEmptyDiffStateCopy(
  mode: DiffFilterMode,
  workspaceChangeLabel: string | undefined,
) {
  if (mode === "unstaged") {
    return {
      title: "No unstaged files",
      detail: "No unstaged changes detected in this workspace.",
    };
  }
  if (mode === "conflicted") {
    return {
      title: "No conflicted files",
      detail: "No merge conflicts detected in this workspace.",
    };
  }
  if (mode === "staged") {
    return {
      title: "No staged files",
      detail: "No staged changes detected in this workspace.",
    };
  }
  if (mode === "cumulative") {
    return {
      title: "No committed files",
      detail: "No committed changes found in this workspace.",
    };
  }
  if (mode !== "uncommitted") {
    return {
      title: "No files in selected commit",
      detail: "No file changes detected for the selected commit.",
    };
  }
  if (workspaceChangeLabel === "Published changes") {
    return {
      title: "No published changes",
      detail: "No published file changes are available.",
    };
  }
  return {
    title: "No workspace changes",
    detail: "No workspace changes detected.",
  };
}

function buildEffectiveCollapsedPaths(
  preference: BulkExpansionPreference,
  files: FileChange[],
  customCollapsedPaths: ReadonlySet<string>,
): Set<string> {
  if (preference === "collapsed") {
    return new Set(files.map((file) => file.path));
  }
  if (preference === "expanded") {
    return new Set();
  }
  return new Set(customCollapsedPaths);
}

function findFirstRenderedAnnotationRow(
  root: HTMLElement,
  filePath: string,
): HTMLElement | null {
  const fileRows = root.querySelectorAll<HTMLElement>("[data-publish-file-path]");
  for (const row of fileRows) {
    if (row.dataset.publishFilePath !== filePath) {
      continue;
    }
    return row.querySelector<HTMLElement>('[data-testid="diff-annotation-row"]');
  }
  return null;
}

interface AgentsPublishVirtualFileRowProps {
  file: FileChange;
  diff: DiffState;
  conflictDiff?: ConflictDiffState | undefined;
  isConflictMode: boolean;
  isExpanded: boolean;
  onTogglePath: (path: string) => void;
  onCopyPath: (path: string) => void;
  onOpenFullscreenPath: (path: string) => void;
  conversationId: string;
  refKind?: DiffRefKind | undefined;
  diffPageRefKind?: DiffRefKind | undefined;
  shouldHydrate: boolean;
  annotations: PrDiffAnnotation[];
  isShowAnywayOverridden: boolean;
  onShowAnywayPath: (path: string) => void;
  isFocusTarget: boolean;
}

const AgentsPublishVirtualFileRow = memo(function AgentsPublishVirtualFileRow({
  file,
  diff,
  conflictDiff,
  isConflictMode,
  isExpanded,
  onTogglePath,
  onCopyPath,
  onOpenFullscreenPath,
  conversationId,
  refKind,
  diffPageRefKind,
  shouldHydrate,
  annotations,
  isShowAnywayOverridden,
  onShowAnywayPath,
  isFocusTarget,
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
      conflictDiff={conflictDiff}
      isConflictMode={isConflictMode}
      isExpanded={isExpanded}
      onToggle={handleToggle}
      onCopyPath={onCopyPath}
      onOpenFullscreen={onOpenFullscreenPath}
      conversationId={conversationId}
      refKind={refKind}
      diffPageRefKind={diffPageRefKind}
      shouldHydrate={shouldHydrate}
      annotations={annotations}
      isShowAnywayOverridden={isShowAnywayOverridden}
      onShowAnyway={handleShowAnyway}
      isFocusTarget={isFocusTarget}
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
  focusRequest,
  defaultMode,
  workspaceChangeLabel,
  liveSummary = null,
  repairMode = false,
}: AgentsPublishInlineDiffsProps) {
  // Set of collapsed file paths; empty = all expanded (default).
  const [collapsedPaths, setCollapsedPaths] = useState<Set<string>>(new Set());
  const [bulkExpansionPreference, setBulkExpansionPreference] =
    useState<BulkExpansionPreference>("expanded");
  // Jump-to-file popover
  const [jumpOpen, setJumpOpen] = useState(false);
  const [jumpSearch, setJumpSearch] = useState("");
  const inlineDiffsRootRef = useRef<HTMLDivElement | null>(null);
  const virtuosoRef = useRef<VirtuosoHandle>(null);
  const [visibleRange, setVisibleRange] = useState<ListRange | null>(null);
  // Lazy hydration tracks which file paths have entered the virtual range.
  // Paths are added on first range entry and never removed, so body teardown does not thrash.
  const [hydratedPaths, setHydratedPaths] = useState<Set<string>>(new Set());
  // Show-anyway overrides — paths where the user has dismissed a generated-file placeholder.
  const [userShowAnywayPaths, setUserShowAnywayPaths] = useState<Set<string>>(new Set());
  const [pendingFocusRequest, setPendingFocusRequest] =
    useState<AgentPublishFocusRequest | null>(null);
  const [focusTargetPath, setFocusTargetPath] = useState<string | null>(null);
  const [pendingAnnotationScrollPath, setPendingAnnotationScrollPath] =
    useState<string | null>(null);
  const autoScrolledAnnotationKeyRef = useRef<string | null>(null);
  const {
    commitSha,
    conflictedCount,
    currentFiles,
    currentFilesError,
    effectiveMode,
    isConflictedMode,
    isCommitMode,
    isCurrentFilesLoading,
    isCumulativeMode,
    isStagedMode,
    isUnstagedMode,
    refKind,
    repairChangeSignature,
    setMode,
    stagedCount,
    supportsWorktreeModes,
    totalAdditions,
    totalDeletions,
    workspaceChangeCount,
    unstagedCount,
  } = useAgentWorkspaceChangeSummary({
    conversationId,
    review,
    defaultMode,
    liveSummary,
    repairMode,
  });
  const rangeRefKind =
    review?.headRef.startsWith(PATCH_BACKED_HEAD_REF_PREFIX) === true
      ? undefined
      : refKind;
  const repairDiffQuerySignature = repairMode
    ? (repairChangeSignature ?? "repair:none")
    : undefined;
  const canRenderPrAnnotations =
    !isConflictedMode && (refKind.kind === "head" || refKind.kind === "cumulative_head");
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
  const firstAnnotatedFilePath = useMemo(() => {
    if (!canRenderPrAnnotations || annotationsByPath.size === 0) {
      return null;
    }
    return (
      currentFiles.find(
        (file) => (annotationsByPath.get(file.path)?.length ?? 0) > 0,
      )?.path ?? null
    );
  }, [annotationsByPath, canRenderPrAnnotations, currentFiles]);
  const annotationAutoScrollKey = useMemo(() => {
    if (!firstAnnotatedFilePath) {
      return null;
    }
    const parts = annotations.flatMap((annotation) => {
      if (!annotation.path || !annotationsByPath.has(annotation.path)) {
        return [];
      }
      return [
        [
          annotation.id,
          annotation.path,
          annotation.side ?? "",
          annotation.startLine,
          annotation.endLine ?? "",
        ].join(":"),
      ];
    });
    if (parts.length === 0) {
      return null;
    }
    return [conversationId, effectiveMode, parts.join("|")].join(":");
  }, [
    annotations,
    annotationsByPath,
    conversationId,
    effectiveMode,
    firstAnnotatedFilePath,
  ]);

  // ── Conversation/mode changes reset hydrated paths; keep same-list viewport ranges ──
  useEffect(() => {
    setHydratedPaths(new Set());
    setVisibleRange(null);
    setFocusTargetPath(null);
    setPendingAnnotationScrollPath(null);
  }, [conversationId]);

  useEffect(() => {
    setHydratedPaths(new Set());
    setFocusTargetPath(null);
    setPendingAnnotationScrollPath(null);
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

  const effectiveCollapsedPaths = useMemo(
    () =>
      buildEffectiveCollapsedPaths(
        bulkExpansionPreference,
        currentFiles,
        collapsedPaths,
      ),
    [bulkExpansionPreference, collapsedPaths, currentFiles],
  );

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

  useEffect(() => {
    if (!visibleRange || currentFiles.length === 0) {
      return;
    }
    hydrateVisibleRange(visibleRange);
  }, [currentFiles, hydrateVisibleRange, visibleRange]);

  // Only fetch diffs for visible expanded files — collapsed/off-range cards pay no query cost.
  const expandedFiles = useMemo(
    () => currentFiles.filter((f) => !effectiveCollapsedPaths.has(f.path)),
    [currentFiles, effectiveCollapsedPaths],
  );

  const fetchableFiles = useMemo(
    () =>
      expandedFiles.filter(
        (file) =>
          bufferedVisiblePathSet.has(file.path) &&
          !isLargeInlineDiff(file) &&
          (!file.isGenerated || userShowAnywayPaths.has(file.path)),
      ),
    [bufferedVisiblePathSet, expandedFiles, userShowAnywayPaths],
  );

  // ── Workspace-change diffs ─────────────────────────────────────────────
  const uncommittedDiffQueries = useQueries({
    queries: (!isCommitMode &&
      !isConflictedMode &&
      !isStagedMode &&
      !isUnstagedMode &&
      !isCumulativeMode
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
      queryKey: [
        ...agentWorkspaceKeys.diff(conversationId),
        repairMode ? "repair-staged" : "staged",
        ...(repairDiffQuerySignature !== undefined ? [repairDiffQuerySignature] : []),
        file.path,
      ],
      queryFn: () =>
        repairMode
          ? diffApi.getAgentConversationWorkspaceRepairStagedFileDiff(
              conversationId,
              file.path,
            )
          : diffApi.getAgentConversationWorkspaceStagedFileDiff(
              conversationId,
              file.path,
            ),
      staleTime: AGENT_WORKSPACE_STALE_MS,
    })),
  });

  // ── Unstaged diffs ────────────────────────────────────────────────────
  const unstagedDiffQueries = useQueries({
    queries: (supportsWorktreeModes && isUnstagedMode ? fetchableFiles : []).map((file) => ({
      queryKey: [
        ...agentWorkspaceKeys.diff(conversationId),
        repairMode ? "repair-unstaged" : "unstaged",
        ...(repairDiffQuerySignature !== undefined ? [repairDiffQuerySignature] : []),
        file.path,
      ],
      queryFn: () =>
        repairMode
          ? diffApi.getAgentConversationWorkspaceRepairUnstagedFileDiff(
              conversationId,
              file.path,
            )
          : diffApi.getAgentConversationWorkspaceUnstagedFileDiff(
              conversationId,
              file.path,
            ),
      staleTime: AGENT_WORKSPACE_STALE_MS,
    })),
  });

  // ── Conflict diffs ─────────────────────────────────────────────────────
  const conflictDiffQueries = useQueries({
    queries: (repairMode && isConflictedMode ? fetchableFiles : []).map((file) => ({
      queryKey: [
        ...agentWorkspaceKeys.diff(conversationId),
        "repair-conflicted",
        ...(repairDiffQuerySignature !== undefined ? [repairDiffQuerySignature] : []),
        file.path,
      ],
      queryFn: () =>
        diffApi.getAgentConversationWorkspaceRepairConflictFileDiff(
          conversationId,
          file.path,
        ),
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

  const conflictDiffByPath = useMemo(() => {
    const map = new Map<string, ConflictDiffState>();
    if (!isConflictedMode) {
      return map;
    }
    fetchableFiles.forEach((file, idx) => {
      const q = conflictDiffQueries[idx];
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
  }, [conflictDiffQueries, fetchableFiles, isConflictedMode]);

  // ── Jump-to-file filtered list ────────────────────────────────────────
  const filteredJumpFiles = useMemo(() => {
    if (!jumpSearch.trim()) return currentFiles;
    const q = jumpSearch.toLowerCase();
    return currentFiles.filter((f) => f.path.toLowerCase().includes(q));
  }, [currentFiles, jumpSearch]);

  // ── Handlers ──────────────────────────────────────────────────────────
  const handleModeChange = useCallback((next: DiffFilterMode) => {
    setMode(next);
  }, [setMode]);

  const handleToggle = useCallback((path: string) => {
    setBulkExpansionPreference("custom");
    setCollapsedPaths((prev) => {
      const next = buildEffectiveCollapsedPaths(
        bulkExpansionPreference,
        currentFiles,
        prev,
      );
      if (next.has(path)) {
        next.delete(path);
      } else {
        next.add(path);
      }
      return next;
    });
  }, [bulkExpansionPreference, currentFiles]);

  const handleCopyPath = useCallback((path: string) => {
    void navigator.clipboard?.writeText(path).catch(() => undefined);
  }, []);

  const handleOpenFullscreen = useCallback(
    (path: string) => {
      onOpenInDialog?.(path);
    },
    [onOpenInDialog],
  );

  const handleOpenDialog = useCallback(() => {
    onOpenInDialog?.();
  }, [onOpenInDialog]);

  const handleShowAnyway = useCallback((path: string) => {
    setUserShowAnywayPaths((prev) => {
      if (prev.has(path)) {
        return prev;
      }
      return new Set([...prev, path]);
    });
  }, []);

  const collapseAll = useCallback(() => {
    setBulkExpansionPreference("collapsed");
    setCollapsedPaths(new Set(currentFiles.map((f) => f.path)));
  }, [currentFiles]);

  const expandAll = useCallback(() => {
    setBulkExpansionPreference("expanded");
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

  useEffect(() => {
    if (!focusRequest || focusRequest.conversationId !== conversationId) {
      return;
    }
    setPendingFocusRequest(focusRequest);
    setMode(focusRequest.mode);
  }, [conversationId, focusRequest, setMode]);

  useEffect(() => {
    if (!pendingFocusRequest || pendingFocusRequest.conversationId !== conversationId) {
      return;
    }
    const index = currentFiles.findIndex(
      (file) => file.path === pendingFocusRequest.filePath,
    );
    if (index < 0) {
      if (!isCurrentFilesLoading && !isLoading) {
        setPendingFocusRequest(null);
      }
      return;
    }

    if (bulkExpansionPreference === "collapsed") {
      setBulkExpansionPreference("custom");
    }
    setCollapsedPaths((prev) => {
      const next = buildEffectiveCollapsedPaths(
        bulkExpansionPreference,
        currentFiles,
        prev,
      );
      if (!next.has(pendingFocusRequest.filePath)) {
        return next;
      }
      next.delete(pendingFocusRequest.filePath);
      return next;
    });
    hydrateVisibleRange({ startIndex: index, endIndex: index });
    virtuosoRef.current?.scrollToIndex({
      index,
      align: "start",
      behavior: "auto",
    });
    setFocusTargetPath(pendingFocusRequest.filePath);
    setPendingFocusRequest(null);
  }, [
    conversationId,
    currentFiles,
    bulkExpansionPreference,
    hydrateVisibleRange,
    isCurrentFilesLoading,
    isLoading,
    pendingFocusRequest,
  ]);

  const computeFileKey = useCallback(
    (_index: number, file: FileChange) => file.path,
    [],
  );

  const renderFileRow = useCallback(
    (_index: number, fileChange: FileChange) => (
      <div
        data-testid={`inline-diffs-file-row-${_index}`}
        data-publish-file-path={fileChange.path}
        className={cn(
          "box-border min-w-0 w-full overflow-x-hidden px-3",
          _index === 0 ? "pt-2" : "pt-0.5",
          _index === currentFiles.length - 1 ? "pb-2" : "pb-0.5",
        )}
      >
        <AgentsPublishVirtualFileRow
          file={fileChange}
          diff={diffByPath.get(fileChange.path)}
          conflictDiff={conflictDiffByPath.get(fileChange.path)}
          isConflictMode={isConflictedMode}
          isExpanded={!effectiveCollapsedPaths.has(fileChange.path)}
          onTogglePath={handleToggle}
          onCopyPath={handleCopyPath}
          onOpenFullscreenPath={handleOpenFullscreen}
          conversationId={conversationId}
          refKind={repairMode ? undefined : rangeRefKind}
          diffPageRefKind={
            !repairMode || isStagedMode || isUnstagedMode ? refKind : undefined
          }
          shouldHydrate={hydratedPaths.has(fileChange.path)}
          annotations={annotationsByPath.get(fileChange.path) ?? EMPTY_PR_DIFF_ANNOTATIONS}
          isShowAnywayOverridden={userShowAnywayPaths.has(fileChange.path)}
          onShowAnywayPath={handleShowAnyway}
          isFocusTarget={focusTargetPath === fileChange.path}
        />
      </div>
    ),
    [
      annotationsByPath,
      conversationId,
      currentFiles.length,
      conflictDiffByPath,
      diffByPath,
      effectiveCollapsedPaths,
      handleCopyPath,
      handleOpenFullscreen,
      handleShowAnyway,
      handleToggle,
      hydratedPaths,
      isConflictedMode,
      isStagedMode,
      isUnstagedMode,
      focusTargetPath,
      refKind,
      rangeRefKind,
      repairMode,
      userShowAnywayPaths,
    ],
  );

  const displayError = error ?? currentFilesError;
  const isFileListLoading = isLoading || isCurrentFilesLoading;
  const emptyStateCopy = getEmptyDiffStateCopy(effectiveMode, workspaceChangeLabel);

  useEffect(() => {
    if (
      !firstAnnotatedFilePath ||
      !annotationAutoScrollKey ||
      isFileListLoading ||
      currentFiles.length === 0 ||
      autoScrolledAnnotationKeyRef.current === annotationAutoScrollKey
    ) {
      return;
    }
    const index = currentFiles.findIndex(
      (file) => file.path === firstAnnotatedFilePath,
    );
    if (index < 0) {
      return;
    }

    if (bulkExpansionPreference === "collapsed") {
      setBulkExpansionPreference("custom");
    }
    setCollapsedPaths((prev) => {
      const next = buildEffectiveCollapsedPaths(
        bulkExpansionPreference,
        currentFiles,
        prev,
      );
      if (!next.has(firstAnnotatedFilePath)) {
        return prev;
      }
      next.delete(firstAnnotatedFilePath);
      return next;
    });
    hydrateVisibleRange({ startIndex: index, endIndex: index });
    virtuosoRef.current?.scrollToIndex({
      index,
      align: "start",
      behavior: "auto",
    });
    setFocusTargetPath(firstAnnotatedFilePath);
    setPendingAnnotationScrollPath(firstAnnotatedFilePath);
    autoScrolledAnnotationKeyRef.current = annotationAutoScrollKey;
  }, [
    annotationAutoScrollKey,
    bulkExpansionPreference,
    currentFiles,
    firstAnnotatedFilePath,
    hydrateVisibleRange,
    isFileListLoading,
  ]);

  useEffect(() => {
    if (!pendingAnnotationScrollPath) {
      return;
    }
    const root = inlineDiffsRootRef.current;
    if (!root) {
      return;
    }
    const annotationRow = findFirstRenderedAnnotationRow(
      root,
      pendingAnnotationScrollPath,
    );
    if (!annotationRow) {
      return;
    }
    annotationRow.scrollIntoView({
      block: "center",
      behavior: "auto",
      inline: "nearest",
    });
    setPendingAnnotationScrollPath(null);
  }, [diffByPath, pendingAnnotationScrollPath, visibleRange]);

  return (
    <div
      ref={inlineDiffsRootRef}
      data-testid="agents-publish-inline-diffs"
      className="flex min-h-0 flex-1 flex-col overflow-x-hidden"
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
          workspaceChangeCount={workspaceChangeCount}
          {...(workspaceChangeLabel !== undefined && { workspaceChangeLabel })}
          {...(conflictedCount !== undefined && { conflictedCount })}
          {...(stagedCount !== undefined && { stagedCount })}
          {...(unstagedCount !== undefined && { unstagedCount })}
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

          {onOpenInDialog !== undefined && (
            <Tooltip>
              <TooltipTrigger asChild>
                <button
                  type="button"
                  data-testid="agents-review-changes"
                  aria-label="Open changes in full diff dialog"
                  onClick={handleOpenDialog}
                  className="flex items-center justify-center rounded p-1 transition-colors hover:bg-[var(--bg-hover)]"
                  style={{ color: "var(--text-muted)" }}
                >
                  <Maximize2 className="h-3.5 w-3.5" aria-hidden="true" />
                </button>
              </TooltipTrigger>
              <TooltipContent side="top">
                <p>Open in full dialog</p>
              </TooltipContent>
            </Tooltip>
          )}
        </div>
      </div>

      {/* Body */}
      {isFileListLoading ? (
        <div data-testid="inline-diffs-loading" className="flex flex-col gap-2 p-3">
          {[1, 2, 3].map((i) => (
            <Skeleton key={i} className="h-10 w-full rounded-md" />
          ))}
        </div>
      ) : displayError ? (
        <div
          data-testid="inline-diffs-error"
          className="flex flex-col items-center justify-center px-4 py-12 text-center"
          style={{ color: "var(--text-muted)" }}
        >
          <p className="text-sm">Could not load workspace changes</p>
          <p className="mt-1 max-w-xl text-xs" style={{ color: "var(--text-muted)" }}>
            {displayError instanceof Error ? displayError.message : String(displayError)}
          </p>
        </div>
      ) : currentFiles.length === 0 ? (
        <div
          data-testid="inline-diffs-empty"
          className="flex flex-col items-center justify-center py-12 text-center"
          style={{ color: "var(--text-muted)" }}
        >
          <p className="text-sm">{emptyStateCopy.title}</p>
          <p className="mt-1 text-xs" style={{ color: "var(--text-muted)" }}>
            {emptyStateCopy.detail}
          </p>
        </div>
      ) : (
        <Virtuoso
          ref={virtuosoRef}
          data={currentFiles}
          data-testid="inline-diffs-virtual-list"
          className="min-h-0 flex-1 overflow-x-hidden"
          style={{ height: "100%", overflowX: "hidden", scrollbarGutter: "stable" }}
          computeItemKey={computeFileKey}
          rangeChanged={hydrateVisibleRange}
          increaseViewportBy={{ top: 240, bottom: 480 }}
          itemContent={renderFileRow}
        />
      )}
    </div>
  );
}
