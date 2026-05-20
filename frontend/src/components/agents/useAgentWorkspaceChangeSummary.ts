import { useCallback, useEffect, useMemo, useState } from "react";
import { useQuery } from "@tanstack/react-query";

import { diffApi } from "@/api/diff";
import type { AgentWorkspaceReview, DiffRefKind, FileChange } from "@/api/diff";
import type { Commit as DiffViewerCommit } from "@/components/diff";

import type { DiffFilterMode } from "./AgentsPublishDiffFilter";
import {
  AGENT_WORKSPACE_STALE_MS,
  agentWorkspaceKeys,
} from "./agentWorkspaceQueries";

export interface AgentWorkspaceChangeSummaryState {
  mode: DiffFilterMode;
  setMode: (mode: DiffFilterMode) => void;
  effectiveMode: DiffFilterMode;
  refKind: DiffRefKind;
  currentFiles: FileChange[];
  currentFilesError: unknown;
  isCurrentFilesLoading: boolean;
  isCommitMode: boolean;
  isStagedMode: boolean;
  isUnstagedMode: boolean;
  isCumulativeMode: boolean;
  commitSha: string | undefined;
  supportsWorktreeModes: boolean;
  workspaceChangeCount: number;
  stagedCount: number | undefined;
  unstagedCount: number | undefined;
  totalAdditions: number;
  totalDeletions: number;
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
}: {
  conversationId: string;
  review: AgentWorkspaceReview | null;
}): AgentWorkspaceChangeSummaryState {
  const [selectedMode, setSelectedMode] = useState<DiffFilterMode>("uncommitted");
  const [hasUserSelectedMode, setHasUserSelectedMode] = useState(false);
  const supportsWorktreeModes = review?.supportsWorktreeModes ?? true;
  const canQueryWorktreeFiles = review != null && supportsWorktreeModes;
  const setMode = useCallback((nextMode: DiffFilterMode) => {
    setSelectedMode(nextMode);
    setHasUserSelectedMode(true);
  }, []);

  useEffect(() => {
    setSelectedMode("uncommitted");
    setHasUserSelectedMode(false);
  }, [conversationId]);

  const stagedFilesQuery = useQuery({
    queryKey: [...agentWorkspaceKeys.diff(conversationId), "staged-files"],
    queryFn: () => diffApi.getAgentConversationWorkspaceStagedFileChanges(conversationId),
    enabled: canQueryWorktreeFiles,
    staleTime: AGENT_WORKSPACE_STALE_MS,
  });

  const unstagedFilesQuery = useQuery({
    queryKey: [...agentWorkspaceKeys.diff(conversationId), "unstaged-files"],
    queryFn: () => diffApi.getAgentConversationWorkspaceUnstagedFileChanges(conversationId),
    enabled: canQueryWorktreeFiles,
    staleTime: AGENT_WORKSPACE_STALE_MS,
  });
  const stagedCount = stagedFilesQuery.data?.length;
  const unstagedCount = unstagedFilesQuery.data?.length;
  const preferredMode = useMemo<DiffFilterMode>(() => {
    if (!supportsWorktreeModes || hasUserSelectedMode) {
      return selectedMode;
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
    stagedCount,
    stagedFilesQuery.isError,
    supportsWorktreeModes,
    unstagedCount,
    unstagedFilesQuery.isError,
  ]);
  const effectiveMode =
    !supportsWorktreeModes &&
    (preferredMode === "uncommitted" || preferredMode === "staged" || preferredMode === "unstaged")
      ? "cumulative"
      : preferredMode;
  const isStagedMode = effectiveMode === "staged";
  const isUnstagedMode = effectiveMode === "unstaged";
  const isCumulativeMode = effectiveMode === "cumulative";
  const isCommitMode =
    effectiveMode !== "uncommitted" &&
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
    enabled: isCommitMode && Boolean(commitSha),
    staleTime: AGENT_WORKSPACE_STALE_MS,
  });

  const cumulativeFilesQuery = useQuery({
    queryKey: [...agentWorkspaceKeys.diff(conversationId), "cumulative-files"],
    queryFn: () => diffApi.getAgentConversationWorkspaceCumulativeFileChanges(conversationId),
    enabled: isCumulativeMode,
    staleTime: AGENT_WORKSPACE_STALE_MS,
  });

  const currentFiles = useMemo<FileChange[]>(() => {
    if (isCommitMode) return commitFilesQuery.data ?? [];
    if (isStagedMode && supportsWorktreeModes) return stagedFilesQuery.data ?? [];
    if (isUnstagedMode && supportsWorktreeModes) return unstagedFilesQuery.data ?? [];
    if (isCumulativeMode) return cumulativeFilesQuery.data ?? review?.changes ?? [];
    return review?.changes ?? [];
  }, [
    commitFilesQuery.data,
    cumulativeFilesQuery.data,
    isCommitMode,
    isCumulativeMode,
    isStagedMode,
    isUnstagedMode,
    review,
    stagedFilesQuery.data,
    supportsWorktreeModes,
    unstagedFilesQuery.data,
  ]);

  const currentFilesError = isCommitMode
    ? commitFilesQuery.error
    : isStagedMode
      ? stagedFilesQuery.error
      : isUnstagedMode
        ? unstagedFilesQuery.error
        : isCumulativeMode
          ? cumulativeFilesQuery.error
          : null;
  const isCurrentFilesLoading = isCommitMode
    ? commitFilesQuery.isLoading
    : isStagedMode
      ? stagedFilesQuery.isLoading
      : isUnstagedMode
        ? unstagedFilesQuery.isLoading
        : isCumulativeMode
          ? (cumulativeFilesQuery.isPending && !cumulativeFilesQuery.isError)
          : false;

  const totalAdditions = currentFiles.reduce((sum, file) => sum + file.additions, 0);
  const totalDeletions = currentFiles.reduce((sum, file) => sum + file.deletions, 0);

  return {
    mode: selectedMode,
    setMode,
    effectiveMode,
    refKind,
    currentFiles,
    currentFilesError,
    isCurrentFilesLoading,
    isCommitMode,
    isStagedMode,
    isUnstagedMode,
    isCumulativeMode,
    commitSha,
    supportsWorktreeModes,
    workspaceChangeCount: review?.changes.length ?? 0,
    stagedCount,
    unstagedCount,
    totalAdditions,
    totalDeletions,
  };
}
