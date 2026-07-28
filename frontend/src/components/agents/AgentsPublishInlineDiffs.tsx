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
 *   - Normal diffs fetched for hydrated expanded fallback files only; page-capable diffs fetch row pages.
 *
 * Performance contract (frontend-interaction-performance.md):
 *   - Sticky bar always renders synchronously.
 *   - File cards receive fallback diff state as prop; page-capable diffs page their own rows.
 *
 * WKWebView CSS: explicit background-color / border-color with shallow-chain tokens.
 */

import { memo, useState, useCallback, useMemo, useRef, useEffect } from "react";
import { useQueries } from "@tanstack/react-query";
import { ArrowDownToLine, ChevronDown, ChevronUp, Info, Maximize2 } from "lucide-react";
import { Virtuoso, type ListRange, type VirtuosoHandle } from "react-virtuoso";
import { Popover, PopoverContent, PopoverTrigger } from "@/components/ui/popover";
import { Input } from "@/components/ui/input";
import { Skeleton } from "@/components/ui/skeleton";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/ui/tooltip";
import { diffApi } from "@/api/diff";
import { DIFF_ANNOTATION_LEVEL_LEGEND } from "@/components/diff/diffRenderHelpers";
import type {
  AgentWorkspaceChangeSummary,
  AgentWorkspaceReview,
  FileChange,
  DiffRefKind,
  PrDiffAnnotation,
  WorkspaceReviewHunkAnnotation,
} from "@/api/diff";
import type { Commit as DiffViewerCommit } from "@/components/diff";
import { cn } from "@/lib/utils";
import { AgentsPublishDiffFilter } from "./AgentsPublishDiffFilter";
import type { DiffFilterMode } from "./AgentsPublishDiffFilter";
import { AgentsPublishFileDiff } from "./AgentsPublishFileDiff";
import type {
  ConflictDiffState,
  DiffPageSummary,
  DiffState,
} from "./AgentsPublishFileDiff";
import { ReviewWalkthrough } from "./ReviewWalkthrough";
import { buildReviewWalkthroughFindings } from "./reviewWalkthroughFindings";
import {
  canUsePagedInlineDiff,
  requiresExplicitDiffHydration,
} from "./inlineDiffGuards";
import {
  AGENT_WORKSPACE_STALE_MS,
  agentWorkspaceKeys,
} from "./agentWorkspaceQueries";
import type { AgentPublishFocusRequest } from "./agentPublishFocus";
import { useAgentWorkspaceChangeSummary } from "./useAgentWorkspaceChangeSummary";

const EMPTY_PR_DIFF_ANNOTATIONS: PrDiffAnnotation[] = [];
const EMPTY_WORKSPACE_REVIEW_HUNK_ANNOTATIONS: WorkspaceReviewHunkAnnotation[] = [];
const DIFF_ROW_COUNT_SUMMARY_LIMIT = 1;
const VIRTUAL_RANGE_OVERSCAN_FILES = 0;
const PATCH_BACKED_HEAD_REF_PREFIX = "github-pr-diff/";
const FILE_JUMP_STABILIZE_FRAMES = 2;
const ANNOTATION_SCROLL_RETRY_DELAY_MS = 50;
const ANNOTATION_SCROLL_RETRY_LIMIT = 60;
type BulkExpansionPreference = "expanded" | "collapsed" | "custom";

interface HydrationPathState {
  generation: string;
  paths: Set<string>;
}

export interface AgentsPublishInlineDiffsProps {
  conversationId: string;
  review: AgentWorkspaceReview | null;
  commits: DiffViewerCommit[];
  isLoading: boolean;
  annotations?: PrDiffAnnotation[] | undefined;
  hunkAnnotations?: WorkspaceReviewHunkAnnotation[] | undefined;
  error?: unknown;
  onOpenInDialog?: ((filePath?: string) => void) | undefined;
  focusRequest?: AgentPublishFocusRequest | null | undefined;
  defaultMode?: DiffFilterMode | undefined;
  workspaceChangeLabel?: string | undefined;
  cumulativeModeLabel?: string | undefined;
  liveSummary?: AgentWorkspaceChangeSummary | null | undefined;
  repairMode?: boolean | undefined;
}

function getEmptyDiffStateCopy(
  mode: DiffFilterMode,
  workspaceChangeLabel: string | undefined,
  cumulativeModeLabel: string | undefined,
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
    if (cumulativeModeLabel === "Published changes") {
      return {
        title: "No published changes",
        detail: "No published file changes are available.",
      };
    }
    if (cumulativeModeLabel === "Pull request changes") {
      return {
        title: "No pull request changes",
        detail: "No pull request file changes are available.",
      };
    }
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
    return row.querySelector<HTMLElement>(
      '[data-testid="diff-annotation-row"], [data-testid="diff-hunk-annotation-row"]',
    );
  }
  return null;
}

function findRenderedFileRow(root: HTMLElement, filePath: string): HTMLElement | null {
  const fileRows = root.querySelectorAll<HTMLElement>("[data-publish-file-path]");
  for (const row of fileRows) {
    if (row.dataset.publishFilePath === filePath) {
      return row;
    }
  }
  return null;
}

function requestFileJumpFrame(callback: FrameRequestCallback): number {
  if (typeof window.requestAnimationFrame === "function") {
    return window.requestAnimationFrame(callback);
  }
  return window.setTimeout(() => callback(performance.now()), 16);
}

function cancelFileJumpFrame(frame: number) {
  if (typeof window.cancelAnimationFrame === "function") {
    window.cancelAnimationFrame(frame);
    return;
  }
  window.clearTimeout(frame);
}

function diffRefKindKey(refKind: DiffRefKind | undefined): string {
  if (!refKind) {
    return "none";
  }
  return refKind.kind === "commit" ? `${refKind.kind}:${refKind.sha}` : refKind.kind;
}

function resolveDiffPageRefKind({
  refKind,
  repairMode,
  isStagedMode,
  isUnstagedMode,
}: {
  refKind: DiffRefKind;
  repairMode: boolean;
  isStagedMode: boolean;
  isUnstagedMode: boolean;
}): DiffRefKind | undefined {
  return !repairMode || isStagedMode || isUnstagedMode ? refKind : undefined;
}

function workspaceReviewHunkAnnotationMatchesMode(
  annotation: WorkspaceReviewHunkAnnotation,
  mode: DiffFilterMode,
  repairMode: boolean,
): boolean {
  if (repairMode || mode === "conflicted") {
    return false;
  }
  if (mode === "staged") {
    return annotation.diffSource === "staged";
  }
  if (mode === "unstaged") {
    return annotation.diffSource === "unstaged";
  }
  if (mode === "uncommitted") {
    return (
      annotation.diffSource === "committed" ||
      annotation.diffSource === "selected_source" ||
      annotation.diffSource === "staged" ||
      annotation.diffSource === "unstaged"
    );
  }
  return annotation.diffSource === "committed" || annotation.diffSource === "selected_source";
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
  diffPageReloadKey?: string | undefined;
  inlineDiffScrollParent: HTMLElement | null;
  diffPageSummary?: DiffPageSummary | undefined;
  shouldHydrate: boolean;
  hydrationGeneration: string;
  onRegisterMountedPath: (path: string, generation: string) => void;
  onUnregisterMountedPath: (path: string, generation: string) => void;
  annotations: PrDiffAnnotation[];
  hunkAnnotations: WorkspaceReviewHunkAnnotation[];
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
  diffPageReloadKey,
  inlineDiffScrollParent,
  diffPageSummary,
  shouldHydrate,
  hydrationGeneration,
  onRegisterMountedPath,
  onUnregisterMountedPath,
  annotations,
  hunkAnnotations,
  isShowAnywayOverridden,
  onShowAnywayPath,
  isFocusTarget,
}: AgentsPublishVirtualFileRowProps) {
  useEffect(() => {
    onRegisterMountedPath(file.path, hydrationGeneration);
    return () => {
      onUnregisterMountedPath(file.path, hydrationGeneration);
    };
  }, [
    file.path,
    hydrationGeneration,
    onRegisterMountedPath,
    onUnregisterMountedPath,
  ]);

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
      diffPageReloadKey={diffPageReloadKey}
      inlineDiffScrollParent={inlineDiffScrollParent}
      diffPageSummary={diffPageSummary}
      shouldHydrate={shouldHydrate}
      annotations={annotations}
      hunkAnnotations={hunkAnnotations}
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
  hunkAnnotations = [],
  error,
  onOpenInDialog,
  focusRequest,
  defaultMode,
  workspaceChangeLabel,
  cumulativeModeLabel,
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
  const [inlineDiffScrollParent, setInlineDiffScrollParent] =
    useState<HTMLElement | null>(null);
  const [visibleRangeState, setVisibleRangeState] = useState<{
    generation: string;
    range: ListRange;
  } | null>(null);
  // Show-anyway overrides — paths where the user has dismissed a generated-file placeholder.
  const [userShowAnywayPaths, setUserShowAnywayPaths] = useState<Set<string>>(new Set());
  const [isReviewWalkthroughOpen, setIsReviewWalkthroughOpen] = useState(false);
  const [reviewWalkthroughFindingId, setReviewWalkthroughFindingId] = useState<string | null>(null);
  const [pendingFocusRequest, setPendingFocusRequest] =
    useState<AgentPublishFocusRequest | null>(null);
  const [focusTargetPath, setFocusTargetPath] = useState<string | null>(null);
  const [pendingFileScrollPath, setPendingFileScrollPath] = useState<string | null>(
    null,
  );
  const [pendingAnnotationScrollPath, setPendingAnnotationScrollPath] =
    useState<string | null>(null);
  const [annotationScrollAttempt, setAnnotationScrollAttempt] = useState(0);
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
    worktreeChangeSignature,
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
  const diffQuerySignature = repairMode
    ? (worktreeChangeSignature ?? "repair:none")
    : worktreeChangeSignature;
  const diffPageRefKind = useMemo(
    () =>
      resolveDiffPageRefKind({
        refKind,
        repairMode,
        isStagedMode,
        isUnstagedMode,
      }),
    [isStagedMode, isUnstagedMode, refKind, repairMode],
  );
  const diffPageReloadKey =
    isStagedMode || isUnstagedMode ? diffQuerySignature : undefined;
  const currentFilePathIdentity = useMemo(
    () => currentFiles.map((file) => file.path).join("\u0000"),
    [currentFiles],
  );
  const hydrationGeneration = useMemo(
    () =>
      [
        conversationId,
        diffRefKindKey(refKind),
        diffQuerySignature ?? "stable",
        currentFilePathIdentity,
      ].join("\u0001"),
    [
      conversationId,
      currentFilePathIdentity,
      refKind,
      diffQuerySignature,
    ],
  );
  const [hydrationState, setHydrationState] = useState<HydrationPathState>(
    () => ({
      generation: hydrationGeneration,
      paths: new Set(),
    }),
  );
  const [mountedState, setMountedState] = useState<HydrationPathState>(() => ({
    generation: hydrationGeneration,
    paths: new Set(),
  }));
  const hydratedPaths = useMemo(
    () =>
      hydrationState.generation === hydrationGeneration
        ? hydrationState.paths
        : new Set<string>(),
    [hydrationGeneration, hydrationState],
  );
  const mountedPaths = useMemo(
    () =>
      mountedState.generation === hydrationGeneration
        ? mountedState.paths
        : new Set<string>(),
    [hydrationGeneration, mountedState],
  );
  const visibleRange =
    visibleRangeState?.generation === hydrationGeneration
      ? visibleRangeState.range
      : null;
  const registerMountedPath = useCallback(
    (path: string, generation: string) => {
      if (generation !== hydrationGeneration) {
        return;
      }
      const register = (current: HydrationPathState): HydrationPathState => {
        const paths =
          current.generation === generation ? current.paths : new Set<string>();
        if (paths.has(path)) {
          return current.generation === generation
            ? current
            : { generation, paths };
        }
        const nextPaths = new Set(paths);
        nextPaths.add(path);
        return { generation, paths: nextPaths };
      };
      setHydrationState(register);
      setMountedState(register);
    },
    [hydrationGeneration],
  );
  const unregisterMountedPath = useCallback(
    (path: string, generation: string) => {
      setMountedState((current) => {
        if (current.generation !== generation || !current.paths.has(path)) {
          return current;
        }
        const nextPaths = new Set(current.paths);
        nextPaths.delete(path);
        return { generation, paths: nextPaths };
      });
    },
    [],
  );
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
  const hunkAnnotationsByPath = useMemo(() => {
    const map = new Map<string, WorkspaceReviewHunkAnnotation[]>();
    for (const annotation of hunkAnnotations) {
      if (
        !workspaceReviewHunkAnnotationMatchesMode(
          annotation,
          effectiveMode,
          repairMode,
        )
      ) {
        continue;
      }
      const existing = map.get(annotation.path);
      if (existing) {
        existing.push(annotation);
      } else {
        map.set(annotation.path, [annotation]);
      }
    }
    return map;
  }, [effectiveMode, hunkAnnotations, repairMode]);
  const activeAnnotationSummary = useMemo(() => {
    const signatureParts: string[] = [];
    let count = 0;
    for (const file of currentFiles) {
      const fileAnnotations = annotationsByPath.get(file.path) ?? EMPTY_PR_DIFF_ANNOTATIONS;
      const fileHunkAnnotations =
        hunkAnnotationsByPath.get(file.path) ?? EMPTY_WORKSPACE_REVIEW_HUNK_ANNOTATIONS;
      if (fileAnnotations.length === 0 && fileHunkAnnotations.length === 0) {
        continue;
      }
      count += fileAnnotations.length + fileHunkAnnotations.length;
      signatureParts.push(
        [
          file.path,
          ...fileAnnotations.map((annotation) => annotation.id),
          ...fileHunkAnnotations.map((annotation) => annotation.id),
        ].join(":"),
      );
    }
    return {
      count,
      signature: signatureParts.join("|"),
    };
  }, [annotationsByPath, currentFiles, hunkAnnotationsByPath]);
  const hasActiveReviewAnnotations = activeAnnotationSummary.count > 0;
  const reviewWalkthroughPath = useMemo(() => {
    if (reviewWalkthroughFindingId === null) return null;
    const isWorkspaceFinding = reviewWalkthroughFindingId.startsWith("workspace:");
    const id = reviewWalkthroughFindingId.slice(
      isWorkspaceFinding ? "workspace:".length : "pr:".length,
    );
    if (!id) return null;
    for (const file of currentFiles) {
      const candidates =
        isWorkspaceFinding
          ? hunkAnnotationsByPath.get(file.path)
          : annotationsByPath.get(file.path);
      if (candidates?.some((annotation) => annotation.id === id)) {
        return file.path;
      }
    }
    return null;
  }, [annotationsByPath, currentFiles, hunkAnnotationsByPath, reviewWalkthroughFindingId]);
  const firstAnnotatedFilePath = useMemo(() => {
    if (annotationsByPath.size === 0 && hunkAnnotationsByPath.size === 0) {
      return null;
    }
    return (
      currentFiles.find(
        (file) =>
          (annotationsByPath.get(file.path)?.length ?? 0) > 0 ||
          (hunkAnnotationsByPath.get(file.path)?.length ?? 0) > 0,
      )?.path ?? null
    );
  }, [annotationsByPath, currentFiles, hunkAnnotationsByPath]);
  const annotationAutoScrollKey = useMemo(() => {
    if (!firstAnnotatedFilePath) {
      return null;
    }
    const prParts = annotations.flatMap((annotation) => {
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
    const hunkParts = hunkAnnotations.flatMap((annotation) => {
      if (!hunkAnnotationsByPath.has(annotation.path)) {
        return [];
      }
      return [
        [
          annotation.id,
          annotation.path,
          annotation.diffSource,
          annotation.oldStart,
          annotation.oldLines,
          annotation.newStart,
          annotation.newLines,
        ].join(":"),
      ];
    });
    const parts = [...prParts, ...hunkParts];
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
    hunkAnnotations,
    hunkAnnotationsByPath,
  ]);

  // ── Conversation/mode changes reset presentation state. Diff hydration is
  // generation-scoped so mounted rows can re-register without an effect race. ──
  useEffect(() => {
    setIsReviewWalkthroughOpen(false);
    setReviewWalkthroughFindingId(null);
    setFocusTargetPath(null);
    setPendingFileScrollPath(null);
    setPendingAnnotationScrollPath(null);
    setAnnotationScrollAttempt(0);
  }, [conversationId]);

  useEffect(() => {
    setIsReviewWalkthroughOpen(false);
    setReviewWalkthroughFindingId(null);
    setFocusTargetPath(null);
    setPendingFileScrollPath(null);
    setPendingAnnotationScrollPath(null);
    setAnnotationScrollAttempt(0);
  }, [activeAnnotationSummary.signature, conversationId, effectiveMode]);

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
      setVisibleRangeState({ generation: hydrationGeneration, range });
      setHydrationState((current) => {
        const previousPaths =
          current.generation === hydrationGeneration
            ? current.paths
            : new Set<string>();
        let changed = false;
        const next = new Set(previousPaths);
        const start = Math.max(
          0,
          range.startIndex - VIRTUAL_RANGE_OVERSCAN_FILES,
        );
        const end = Math.min(
          currentFiles.length - 1,
          range.endIndex + VIRTUAL_RANGE_OVERSCAN_FILES,
        );

        for (let index = start; index <= end; index += 1) {
          const path = currentFiles[index]?.path;
          if (path && !previousPaths.has(path)) {
            next.add(path);
            changed = true;
          }
        }

        if (!changed && current.generation === hydrationGeneration) {
          return current;
        }
        return { generation: hydrationGeneration, paths: next };
      });
    },
    [currentFiles, hydrationGeneration],
  );

  const handleInlineDiffScrollerRef = useCallback(
    (ref: HTMLElement | Window | null) => {
      setInlineDiffScrollParent((current) => {
        const next = ref instanceof HTMLElement ? ref : null;
        return current === next ? current : next;
      });
    },
    [],
  );

  useEffect(() => {
    if (!visibleRange || currentFiles.length === 0) {
      return;
    }
    hydrateVisibleRange(visibleRange);
  }, [currentFiles, hydrateVisibleRange, visibleRange]);

  // Only fetch fallback diffs for visible expanded files — collapsed/off-range/page-capable cards pay no query cost.
  const expandedFiles = useMemo(
    () => currentFiles.filter((f) => !effectiveCollapsedPaths.has(f.path)),
    [currentFiles, effectiveCollapsedPaths],
  );

  const liveFetchEligiblePathSet = useMemo(
    () => new Set([...mountedPaths, ...bufferedVisiblePathSet]),
    [bufferedVisiblePathSet, mountedPaths],
  );
  const fetchCandidateFiles = useMemo(
    () =>
      currentFiles.filter(
        (file) =>
          !effectiveCollapsedPaths.has(file.path) ||
          file.path === reviewWalkthroughPath,
      ),
    [currentFiles, effectiveCollapsedPaths, reviewWalkthroughPath],
  );

  const fetchableFiles = useMemo(
    () =>
      fetchCandidateFiles.filter((file) => {
        const isShowAnywayOverridden = userShowAnywayPaths.has(file.path);
        const isWalkthroughTarget = file.path === reviewWalkthroughPath;
        return (
          (liveFetchEligiblePathSet.has(file.path) || isWalkthroughTarget) &&
          // The walkthrough needs real hunk lines, so it opts out of paged
          // rendering — but the generated-file gate still requires explicit intent.
          (isWalkthroughTarget ||
            !canUsePagedInlineDiff({
              file,
              isConflictMode: isConflictedMode,
              conversationId,
              diffPageRefKind,
              isShowAnywayOverridden,
            })) &&
          (!requiresExplicitDiffHydration(file) || isShowAnywayOverridden)
        );
      }),
    [
      conversationId,
      diffPageRefKind,
      fetchCandidateFiles,
      isConflictedMode,
      liveFetchEligiblePathSet,
      reviewWalkthroughPath,
      userShowAnywayPaths,
    ],
  );

  const diffPageRefKindKey = diffRefKindKey(diffPageRefKind);
  const pageSummaryFiles = useMemo(
    () =>
      expandedFiles.filter((file) => {
        const isShowAnywayOverridden = userShowAnywayPaths.has(file.path);
        return (
          liveFetchEligiblePathSet.has(file.path) &&
          file.path !== reviewWalkthroughPath &&
          canUsePagedInlineDiff({
            file,
            isConflictMode: isConflictedMode,
            conversationId,
            diffPageRefKind,
            isShowAnywayOverridden,
          })
        );
      }),
    [
      conversationId,
      diffPageRefKind,
      expandedFiles,
      isConflictedMode,
      liveFetchEligiblePathSet,
      reviewWalkthroughPath,
      userShowAnywayPaths,
    ],
  );

  const pageSummaryQueries = useQueries({
    queries:
      diffPageRefKind === undefined
        ? []
        : pageSummaryFiles.map((file) => ({
            queryKey: [
              ...agentWorkspaceKeys.diff(conversationId),
              "page-summary",
              diffPageRefKindKey,
              diffPageReloadKey ?? "stable",
              file.path,
            ],
            queryFn: () =>
              diffApi.getAgentConversationWorkspaceFileDiffPage({
                conversationId,
                path: file.path,
                refKind: diffPageRefKind,
                offset: 0,
                limit: DIFF_ROW_COUNT_SUMMARY_LIMIT,
              }),
            staleTime: AGENT_WORKSPACE_STALE_MS,
          })),
  });

  const diffPageSummaryByPath = useMemo(() => {
    const map = new Map<string, DiffPageSummary>();
    pageSummaryFiles.forEach((file, idx) => {
      const page = pageSummaryQueries[idx]?.data;
      if (page !== undefined) {
        map.set(file.path, {
          totalRows: page.totalRows,
          isBinary: page.isBinary,
        });
      }
    });
    return map;
  }, [pageSummaryFiles, pageSummaryQueries]);

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
        ...(diffQuerySignature !== undefined ? [diffQuerySignature] : []),
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
        ...(diffQuerySignature !== undefined ? [diffQuerySignature] : []),
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
        ...(diffQuerySignature !== undefined ? [diffQuerySignature] : []),
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
  const activeDiffQueries = useMemo(
    () =>
      isCommitMode
        ? commitDiffQueries
        : isStagedMode
          ? stagedDiffQueries
          : isUnstagedMode
            ? unstagedDiffQueries
            : isCumulativeMode
              ? cumulativeDiffQueries
              : uncommittedDiffQueries,
    [
      isCommitMode,
      isStagedMode,
      isUnstagedMode,
      isCumulativeMode,
      uncommittedDiffQueries,
      commitDiffQueries,
      stagedDiffQueries,
      unstagedDiffQueries,
      cumulativeDiffQueries,
    ],
  );

  const diffByPath = useMemo(() => {
    const map = new Map<string, DiffState>();
    fetchableFiles.forEach((file, idx) => {
      const q = activeDiffQueries[idx];
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
  }, [activeDiffQueries, fetchableFiles]);

  // Retries the failed diff fetch behind the walkthrough's current finding so a
  // transient error is recoverable without leaving and re-entering the view.
  const handleRetryWalkthroughHunk = useCallback(
    (path: string) => {
      const idx = fetchableFiles.findIndex((file) => file.path === path);
      if (idx === -1) return;
      void activeDiffQueries[idx]?.refetch();
    },
    [activeDiffQueries, fetchableFiles],
  );

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

  const reviewWalkthroughFindings = useMemo(
    () =>
      buildReviewWalkthroughFindings({
        files: currentFiles,
        annotationsByPath,
        hunkAnnotationsByPath,
        diffByPath,
        showAnywayPaths: userShowAnywayPaths,
      }),
    [
      annotationsByPath,
      currentFiles,
      diffByPath,
      hunkAnnotationsByPath,
      userShowAnywayPaths,
    ],
  );

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
    setFocusTargetPath(path);
    setPendingFileScrollPath(path);
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
    setPendingFileScrollPath(pendingFocusRequest.filePath);
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

  useEffect(() => {
    if (!pendingFileScrollPath) {
      return;
    }
    const root = inlineDiffsRootRef.current;
    if (!root) {
      return;
    }
    const fileRow = findRenderedFileRow(root, pendingFileScrollPath);
    if (!fileRow) {
      return;
    }
    const targetFileRow = fileRow;

    let isCancelled = false;
    let frame: number | null = null;
    let resizeObserver: ResizeObserver | null = null;
    let remainingAlignmentFrames = FILE_JUMP_STABILIZE_FRAMES;
    const userInputOptions: AddEventListenerOptions = {
      capture: true,
      passive: true,
    };
    function scrollToFileRow() {
      if (isCancelled) {
        return;
      }
      targetFileRow.scrollIntoView({
        block: "start",
        behavior: "auto",
        inline: "nearest",
      });
    }
    function scheduleAlignmentFrame() {
      if (isCancelled || frame !== null) {
        return;
      }
      if (remainingAlignmentFrames <= 0) {
        stopStabilizing(true);
        return;
      }
      frame = requestFileJumpFrame(() => {
        frame = null;
        remainingAlignmentFrames -= 1;
        scrollToFileRow();
        if (!isCancelled && remainingAlignmentFrames > 0) {
          scheduleAlignmentFrame();
        } else {
          stopStabilizing(true);
        }
      });
    }
    function stopForUserInput() {
      stopStabilizing(true);
    }
    function stopStabilizing(clearPendingPath: boolean) {
      if (isCancelled) {
        return;
      }
      isCancelled = true;
      if (frame !== null) {
        cancelFileJumpFrame(frame);
        frame = null;
      }
      resizeObserver?.disconnect();
      window.removeEventListener("wheel", stopForUserInput, userInputOptions);
      window.removeEventListener("touchmove", stopForUserInput, userInputOptions);
      window.removeEventListener("pointerdown", stopForUserInput, userInputOptions);
      window.removeEventListener("keydown", stopForUserInput, userInputOptions);
      if (clearPendingPath) {
        setPendingFileScrollPath((current) =>
          current === pendingFileScrollPath ? null : current,
        );
      }
    }

    resizeObserver =
      typeof ResizeObserver !== "undefined"
        ? new ResizeObserver(() => {
            scrollToFileRow();
            scheduleAlignmentFrame();
          })
        : null;
    resizeObserver?.observe(targetFileRow);
    window.addEventListener("wheel", stopForUserInput, userInputOptions);
    window.addEventListener("touchmove", stopForUserInput, userInputOptions);
    window.addEventListener("pointerdown", stopForUserInput, userInputOptions);
    window.addEventListener("keydown", stopForUserInput, userInputOptions);
    scrollToFileRow();
    scheduleAlignmentFrame();

    return () => {
      stopStabilizing(false);
    };
  }, [pendingFileScrollPath, visibleRange]);

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
          diffPageRefKind={diffPageRefKind}
          diffPageReloadKey={diffPageReloadKey}
          inlineDiffScrollParent={inlineDiffScrollParent}
          diffPageSummary={diffPageSummaryByPath.get(fileChange.path)}
          shouldHydrate={hydratedPaths.has(fileChange.path)}
          hydrationGeneration={hydrationGeneration}
          onRegisterMountedPath={registerMountedPath}
          onUnregisterMountedPath={unregisterMountedPath}
          annotations={
            annotationsByPath.get(fileChange.path) ?? EMPTY_PR_DIFF_ANNOTATIONS
          }
          hunkAnnotations={
            hunkAnnotationsByPath.get(fileChange.path) ??
            EMPTY_WORKSPACE_REVIEW_HUNK_ANNOTATIONS
          }
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
      hunkAnnotationsByPath,
      hydratedPaths,
      hydrationGeneration,
      inlineDiffScrollParent,
      isConflictedMode,
      diffPageRefKind,
      diffPageReloadKey,
      diffPageSummaryByPath,
      focusTargetPath,
      rangeRefKind,
      registerMountedPath,
      repairMode,
      unregisterMountedPath,
      userShowAnywayPaths,
    ],
  );

  const displayError = error ?? currentFilesError;
  const isFileListLoading = isLoading || isCurrentFilesLoading;
  const emptyStateCopy = getEmptyDiffStateCopy(
    effectiveMode,
    workspaceChangeLabel,
    cumulativeModeLabel,
  );
  const errorSubject =
    effectiveMode === "cumulative" && cumulativeModeLabel
      ? cumulativeModeLabel.toLowerCase()
      : "workspace changes";

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
    setAnnotationScrollAttempt(0);
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
      if (annotationScrollAttempt >= ANNOTATION_SCROLL_RETRY_LIMIT) {
        setPendingAnnotationScrollPath(null);
        setAnnotationScrollAttempt(0);
        return;
      }
      const retryTimer = window.setTimeout(() => {
        setAnnotationScrollAttempt((attempt) => attempt + 1);
      }, ANNOTATION_SCROLL_RETRY_DELAY_MS);
      return () => window.clearTimeout(retryTimer);
    }
    annotationRow.scrollIntoView({
      block: "center",
      behavior: "auto",
      inline: "nearest",
    });
    setPendingAnnotationScrollPath(null);
    setAnnotationScrollAttempt(0);
    return undefined;
  }, [annotationScrollAttempt, diffByPath, pendingAnnotationScrollPath, visibleRange]);

  // The walkthrough reads its hunks from `diffByPath`/`activeDiffQueries`, which
  // are empty in conflicted mode (conflicts use `conflictDiffQueries`). Today the
  // annotation gates already keep findings empty there, but keeping the surface
  // itself out of conflicted mode means a future gate change cannot produce a
  // walkthrough whose hunks never load and whose retry silently does nothing.
  if (isReviewWalkthroughOpen && !isConflictedMode) {
    return (
      <div
        ref={inlineDiffsRootRef}
        data-testid="agents-publish-inline-diffs"
        className="flex min-h-0 flex-1 flex-col overflow-x-hidden"
      >
        <ReviewWalkthrough
          findings={reviewWalkthroughFindings}
          onExit={() => {
            setIsReviewWalkthroughOpen(false);
            setReviewWalkthroughFindingId(null);
          }}
          onOpenFile={handleOpenFullscreen}
          onCurrentFindingChange={setReviewWalkthroughFindingId}
          onRetryHunk={handleRetryWalkthroughHunk}
          onLoadHunkAnyway={handleShowAnyway}
        />
      </div>
    );
  }

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
          {...(cumulativeModeLabel !== undefined && { cumulativeModeLabel })}
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

          {hasActiveReviewAnnotations && (
            <>
              <button
                type="button"
                data-testid="publish-review-walkthrough-enter"
                aria-label="Start review walkthrough"
                onClick={() => setIsReviewWalkthroughOpen(true)}
                className="rounded border px-2 py-1 text-[0.6875rem] font-semibold transition-colors hover:bg-[var(--bg-hover)]"
                style={{
                  backgroundColor: "transparent",
                  borderColor: "var(--border-subtle)",
                  borderStyle: "solid",
                  borderWidth: "1px",
                  color: "var(--text-secondary)",
                }}
              >
                ▶ Walkthrough {activeAnnotationSummary.count}
              </button>
              <Popover modal={false}>
                <PopoverTrigger asChild>
                  <button
                    type="button"
                    data-testid="inline-diffs-review-legend"
                    aria-label="Show review annotation legend"
                    className="inline-flex items-center gap-1 rounded border px-2 py-1 text-[0.6875rem] transition-colors hover:bg-[var(--bg-hover)]"
                    style={{
                      borderColor: "var(--border-subtle)",
                      borderStyle: "solid",
                      borderWidth: "1px",
                      color: "var(--text-secondary)",
                    }}
                  >
                    <Info className="h-3 w-3" aria-hidden="true" />
                    Legend
                  </button>
                </PopoverTrigger>
                <PopoverContent
                  align="end"
                  data-testid="inline-diffs-review-legend-popover"
                  className="w-80 p-3"
                  style={{
                    backgroundColor: "var(--bg-elevated)",
                    borderColor: "var(--border-subtle)",
                    borderStyle: "solid",
                    borderWidth: "1px",
                    boxShadow: "var(--shadow-sm)",
                  }}
                >
                  <div className="space-y-2">
                    <p
                      className="text-xs font-semibold"
                      style={{ color: "var(--text-primary)" }}
                    >
                      Review annotation colors
                    </p>
                    <div className="space-y-1.5">
                      {DIFF_ANNOTATION_LEVEL_LEGEND.map((item) => (
                        <div key={item.label} className="flex items-start gap-2">
                          <span
                            aria-hidden="true"
                            className="mt-1 h-2.5 w-2.5 shrink-0 rounded-full"
                            style={{ backgroundColor: item.color }}
                          />
                          <div className="min-w-0">
                            <div className="text-[0.6875rem] font-medium" style={{ color: "var(--text-primary)" }}>
                              {item.label}
                              <span className="ml-1 font-normal" style={{ color: "var(--text-muted)" }}>
                                {item.levels}
                              </span>
                            </div>
                            <p className="text-[0.6875rem]" style={{ color: "var(--text-muted)" }}>
                              {item.description}
                            </p>
                          </div>
                        </div>
                      ))}
                    </div>
                    <p
                      className="pt-2 text-[0.6875rem]"
                      style={{
                        borderTopColor: "var(--border-subtle)",
                        borderTopStyle: "solid",
                        borderTopWidth: "1px",
                        color: "var(--text-muted)",
                      }}
                    >
                      GitHub rows come from checks, code scanning, and review comments.
                      Workspace review rows come from the RalphX reviewer attached to a diff hunk.
                    </p>
                  </div>
                </PopoverContent>
              </Popover>
            </>
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
          <p className="text-sm">Could not load {errorSubject}</p>
          <p
            className="mt-1 max-w-xl text-xs"
            style={{ color: "var(--text-muted)" }}
          >
            {displayError instanceof Error
              ? displayError.message
              : String(displayError)}
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
          scrollerRef={handleInlineDiffScrollerRef}
          increaseViewportBy={{ top: 240, bottom: 480 }}
          itemContent={renderFileRow}
        />
      )}
    </div>
  );
}
