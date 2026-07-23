import { useCallback, useEffect, useMemo, useRef, useState } from "react";
import { useQuery } from "@tanstack/react-query";

import { diffApi } from "@/api/diff";
import type {
  AgentWorkspaceChangeBucketSummary,
  AgentWorkspaceChangeSummary,
  AgentWorkspaceReview,
  DiffRefKind,
  FileChange,
} from "@/api/diff";
import type { Commit as DiffViewerCommit } from "@/components/diff";

import type { DiffFilterMode } from "./AgentsPublishDiffFilter";
import {
  AGENT_WORKSPACE_STALE_MS,
  agentWorkspaceKeys,
} from "./agentWorkspaceQueries";
import { buildRepairChangeSignature } from "./repairDiffSignature";

export interface AgentWorkspaceChangeSummaryState {
  mode: DiffFilterMode;
  setMode: (mode: DiffFilterMode) => void;
  effectiveMode: DiffFilterMode;
  refKind: DiffRefKind;
  currentFiles: FileChange[];
  currentFilesError: unknown;
  isCurrentFilesLoading: boolean;
  isConflictedMode: boolean;
  isCommitMode: boolean;
  isStagedMode: boolean;
  isUnstagedMode: boolean;
  isCumulativeMode: boolean;
  commitSha: string | undefined;
  supportsWorktreeModes: boolean;
  workspaceChangeCount: number;
  currentFileCount: number;
  conflictedCount: number | undefined;
  stagedCount: number | undefined;
  unstagedCount: number | undefined;
  totalAdditions: number;
  totalDeletions: number;
  worktreeChangeSignature?: string | undefined;
}

function liveBucketSignature(
  bucket: AgentWorkspaceChangeBucketSummary,
): string {
  return `live:${bucket.fileCount}:${bucket.additions}:${bucket.deletions}`;
}

export function mapReviewCommitsToDiffViewerCommits(
  review: AgentWorkspaceReview | null | undefined,
): DiffViewerCommit[] {
  return (review?.commits ?? [])
    .map((commit) => ({
      sha: commit.sha,
      shortSha: commit.shortSha,
      message: commit.message,
      author: commit.author,
      date: commit.date,
    }))
    .reverse();
}

export function useAgentWorkspaceChangeSummary({
  conversationId,
  review,
  defaultMode = "uncommitted",
  liveSummary = null,
  hydrateWorktreeFileLists = true,
  repairMode = false,
  enabled = true,
}: {
  conversationId: string;
  review: AgentWorkspaceReview | null;
  defaultMode?: DiffFilterMode | undefined;
  liveSummary?: AgentWorkspaceChangeSummary | null;
  hydrateWorktreeFileLists?: boolean;
  repairMode?: boolean;
  enabled?: boolean;
}): AgentWorkspaceChangeSummaryState {
  const [selectedMode, setSelectedMode] = useState<DiffFilterMode>(defaultMode);
  const [hasUserSelectedMode, setHasUserSelectedMode] = useState(false);
  const previousConversationIdRef = useRef(conversationId);
  const supportsWorktreeModes =
    liveSummary?.supportsWorktreeModes ?? review?.supportsWorktreeModes ?? true;
  const canQueryWorktreeFiles =
    enabled &&
    hydrateWorktreeFileLists &&
    supportsWorktreeModes &&
    (review != null || liveSummary != null);
  const hasLiveWorktreeSummary = liveSummary != null && supportsWorktreeModes;
  const repairChangeSignature = useMemo(
    () => (repairMode ? buildRepairChangeSignature(liveSummary) : undefined),
    [liveSummary, repairMode],
  );
  const stagedChangeSignature = repairChangeSignature ??
    (liveSummary ? liveBucketSignature(liveSummary.staged) : undefined);
  const unstagedChangeSignature = repairChangeSignature ??
    (liveSummary ? liveBucketSignature(liveSummary.unstaged) : undefined);
  const liveStagedCount = liveSummary?.staged.fileCount;
  const liveUnstagedCount = liveSummary?.unstaged.fileCount;
  const liveConflictedCount = liveSummary?.conflicted?.fileCount;
  const setMode = useCallback((nextMode: DiffFilterMode) => {
    setSelectedMode(nextMode);
    setHasUserSelectedMode(true);
  }, []);
  const shouldAutoPreferConflicted =
    repairMode &&
    !hasUserSelectedMode &&
    selectedMode === "uncommitted" &&
    liveConflictedCount !== undefined &&
    liveConflictedCount > 0;

  useEffect(() => {
    const conversationChanged = previousConversationIdRef.current !== conversationId;
    previousConversationIdRef.current = conversationId;

    if (conversationChanged) {
      setSelectedMode(defaultMode);
      setHasUserSelectedMode(false);
      return;
    }

    if (!hasUserSelectedMode) {
      setSelectedMode(defaultMode);
    }
  }, [conversationId, defaultMode, hasUserSelectedMode]);

  const stagedFilesQuery = useQuery({
    queryKey: [
      ...agentWorkspaceKeys.diff(conversationId),
      repairMode ? "repair-staged-files" : "staged-files",
      ...(stagedChangeSignature !== undefined ? [stagedChangeSignature] : []),
    ],
    queryFn: () =>
      repairMode
        ? diffApi.getAgentConversationWorkspaceRepairStagedFileChanges(conversationId)
        : diffApi.getAgentConversationWorkspaceStagedFileChanges(conversationId),
    enabled:
      canQueryWorktreeFiles &&
      (!hasLiveWorktreeSummary ||
        selectedMode === "staged" ||
        (!shouldAutoPreferConflicted &&
          !hasUserSelectedMode &&
          liveUnstagedCount === 0 &&
          liveStagedCount !== undefined &&
          liveStagedCount > 0)),
    staleTime: AGENT_WORKSPACE_STALE_MS,
  });

  const unstagedFilesQuery = useQuery({
    queryKey: [
      ...agentWorkspaceKeys.diff(conversationId),
      repairMode ? "repair-unstaged-files" : "unstaged-files",
      ...(unstagedChangeSignature !== undefined ? [unstagedChangeSignature] : []),
    ],
    queryFn: () =>
      repairMode
        ? diffApi.getAgentConversationWorkspaceRepairUnstagedFileChanges(conversationId)
        : diffApi.getAgentConversationWorkspaceUnstagedFileChanges(conversationId),
    enabled:
      canQueryWorktreeFiles &&
      (!hasLiveWorktreeSummary ||
        selectedMode === "unstaged" ||
        (!shouldAutoPreferConflicted &&
          !hasUserSelectedMode &&
          liveUnstagedCount !== undefined &&
          liveUnstagedCount > 0)),
    staleTime: AGENT_WORKSPACE_STALE_MS,
  });
  const stagedCount = enabled
    ? (liveSummary?.staged.fileCount ?? stagedFilesQuery.data?.length)
    : undefined;
  const unstagedCount = enabled
    ? (liveSummary?.unstaged.fileCount ?? unstagedFilesQuery.data?.length)
    : undefined;
  const conflictedCount = enabled
    ? liveSummary?.conflicted?.fileCount
    : undefined;
  const conflictedFilePaths = enabled
    ? liveSummary?.conflicted?.files
    : undefined;
  const preferredMode = useMemo<DiffFilterMode>(() => {
    if (!supportsWorktreeModes || hasUserSelectedMode || selectedMode !== "uncommitted") {
      return selectedMode;
    }
    if (shouldAutoPreferConflicted) {
      return "conflicted";
    }
    if (unstagedCount !== undefined && unstagedCount > 0) {
      return "unstaged";
    }
    const unstagedKnownEmpty = unstagedCount === 0 || unstagedFilesQuery.isError;
    if (unstagedKnownEmpty && stagedCount !== undefined && stagedCount > 0) {
      return "staged";
    }
    const stagedKnownEmpty = stagedCount === 0 || stagedFilesQuery.isError;
    if (unstagedKnownEmpty && stagedKnownEmpty) {
      return "uncommitted";
    }
    return selectedMode;
  }, [
    hasUserSelectedMode,
    selectedMode,
    shouldAutoPreferConflicted,
    stagedCount,
    stagedFilesQuery.isError,
    supportsWorktreeModes,
    unstagedCount,
    unstagedFilesQuery.isError,
  ]);
  const effectiveMode =
    !supportsWorktreeModes &&
    (preferredMode === "uncommitted" ||
      preferredMode === "staged" ||
      preferredMode === "unstaged")
      ? "cumulative"
      : preferredMode;
  const isStagedMode = effectiveMode === "staged";
  const isUnstagedMode = effectiveMode === "unstaged";
  const isConflictedMode = effectiveMode === "conflicted";
  const isCumulativeMode = effectiveMode === "cumulative";
  const worktreeChangeSignature = repairChangeSignature ??
    (isStagedMode
      ? stagedChangeSignature
      : isUnstagedMode
        ? unstagedChangeSignature
        : undefined);
  const isCommitMode =
    effectiveMode !== "uncommitted" &&
    !isConflictedMode &&
    !isStagedMode &&
    !isUnstagedMode &&
    !isCumulativeMode;
  const commitSha = isCommitMode ? effectiveMode : undefined;

  const refKind = useMemo<DiffRefKind>(() => {
    if (isStagedMode) return { kind: "staged" };
    if (isUnstagedMode) return { kind: "unstaged" };
    if (isCumulativeMode) return { kind: "cumulative_head" };
    if (isCommitMode && commitSha !== undefined) {
      return { kind: "commit", sha: commitSha };
    }
    return { kind: "head" };
  }, [commitSha, isCommitMode, isCumulativeMode, isStagedMode, isUnstagedMode]);

  const commitFilesQuery = useQuery({
    queryKey: [...agentWorkspaceKeys.diff(conversationId), "commit-files", commitSha],
    queryFn: () => {
      if (!commitSha) throw new Error("commitSha required");
      return diffApi.getAgentConversationWorkspaceCommitFileChanges(conversationId, commitSha);
    },
    enabled: enabled && isCommitMode && Boolean(commitSha),
    staleTime: AGENT_WORKSPACE_STALE_MS,
  });

  const cumulativeFilesQuery = useQuery({
    queryKey: [
      ...agentWorkspaceKeys.diff(conversationId),
      "cumulative-files",
      ...(!supportsWorktreeModes
        ? ["read-only", review?.baseRef ?? "", review?.headRef ?? ""]
        : []),
    ],
    queryFn: () =>
      diffApi.getAgentConversationWorkspaceCumulativeFileChanges(
        conversationId,
      ),
    enabled: enabled && isCumulativeMode,
    staleTime: AGENT_WORKSPACE_STALE_MS,
  });

  const conflictedFiles = useMemo<FileChange[]>(() => {
    if (!repairMode || !conflictedFilePaths) {
      return [];
    }
    return conflictedFilePaths.map((path) => ({
      path,
      status: "modified",
      additions: 0,
      deletions: 0,
      isGenerated: false,
    }));
  }, [conflictedFilePaths, repairMode]);

  const currentFiles = useMemo<FileChange[]>(() => {
    if (!enabled) return [];
    if (isCommitMode) return commitFilesQuery.data ?? [];
    if (isConflictedMode) return conflictedFiles;
    if (isStagedMode && supportsWorktreeModes) return stagedFilesQuery.data ?? [];
    if (isUnstagedMode && supportsWorktreeModes) return unstagedFilesQuery.data ?? [];
    if (isCumulativeMode) return cumulativeFilesQuery.data ?? review?.changes ?? [];
    return review?.changes ?? [];
  }, [
    commitFilesQuery.data,
    cumulativeFilesQuery.data,
    enabled,
    conflictedFiles,
    isCommitMode,
    isConflictedMode,
    isCumulativeMode,
    isStagedMode,
    isUnstagedMode,
    review,
    stagedFilesQuery.data,
    supportsWorktreeModes,
    unstagedFilesQuery.data,
  ]);

  const currentFilesError = !enabled
    ? null
    : isCommitMode
      ? commitFilesQuery.error
      : isConflictedMode
        ? null
        : isStagedMode
          ? stagedFilesQuery.error
          : isUnstagedMode
            ? unstagedFilesQuery.error
            : isCumulativeMode
              ? cumulativeFilesQuery.error
              : null;
  const isCurrentFilesLoading = !enabled
    ? false
    : isCommitMode
      ? commitFilesQuery.isLoading
      : isConflictedMode
        ? false
        : isStagedMode
          ? stagedFilesQuery.isLoading
          : isUnstagedMode
            ? unstagedFilesQuery.isLoading
            : isCumulativeMode
              ? cumulativeFilesQuery.isPending && !cumulativeFilesQuery.isError
              : false;

  const currentLiveBucket =
    useMemo<AgentWorkspaceChangeBucketSummary | null>(() => {
      if (!enabled || !liveSummary || !supportsWorktreeModes) return null;
      if (isStagedMode) return liveSummary.staged;
      if (isUnstagedMode) return liveSummary.unstaged;
      return null;
    }, [
      enabled,
      isStagedMode,
      isUnstagedMode,
      liveSummary,
      supportsWorktreeModes,
    ]);
  const currentFileCount = currentLiveBucket?.fileCount ?? currentFiles.length;
  const totalAdditions =
    currentLiveBucket?.additions ??
    currentFiles.reduce((sum, file) => sum + file.additions, 0);
  const totalDeletions =
    currentLiveBucket?.deletions ??
    currentFiles.reduce((sum, file) => sum + file.deletions, 0);

  return {
    mode: selectedMode,
    setMode,
    effectiveMode,
    refKind,
    currentFiles,
    currentFilesError,
    isCurrentFilesLoading,
    isConflictedMode,
    isCommitMode,
    isStagedMode,
    isUnstagedMode,
    isCumulativeMode,
    commitSha,
    supportsWorktreeModes,
    workspaceChangeCount: !enabled
      ? 0
      : review != null
        ? review.changes.length
        : liveSummary != null
        ? liveSummary.staged.fileCount +
          liveSummary.unstaged.fileCount +
          (liveSummary.conflicted?.fileCount ?? 0)
        : 0,
    currentFileCount,
    conflictedCount,
    stagedCount,
    unstagedCount,
    totalAdditions,
    totalDeletions,
    worktreeChangeSignature,
  };
}
