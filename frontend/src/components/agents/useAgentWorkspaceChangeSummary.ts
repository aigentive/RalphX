import { useMemo, useState } from "react";
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
  uncommittedCount: number;
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
  const [mode, setMode] = useState<DiffFilterMode>("uncommitted");
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

  const stagedFilesQuery = useQuery({
    queryKey: [...agentWorkspaceKeys.diff(conversationId), "staged-files"],
    queryFn: () => diffApi.getAgentConversationWorkspaceStagedFileChanges(conversationId),
    enabled: supportsWorktreeModes && isStagedMode,
    staleTime: AGENT_WORKSPACE_STALE_MS,
  });

  const unstagedFilesQuery = useQuery({
    queryKey: [...agentWorkspaceKeys.diff(conversationId), "unstaged-files"],
    queryFn: () => diffApi.getAgentConversationWorkspaceUnstagedFileChanges(conversationId),
    enabled: supportsWorktreeModes && isUnstagedMode,
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
          ? cumulativeFilesQuery.isLoading
          : false;

  const totalAdditions = currentFiles.reduce((sum, file) => sum + file.additions, 0);
  const totalDeletions = currentFiles.reduce((sum, file) => sum + file.deletions, 0);

  return {
    mode,
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
    uncommittedCount: review?.changes.length ?? 0,
    stagedCount: stagedFilesQuery.data?.length,
    unstagedCount: unstagedFilesQuery.data?.length,
    totalAdditions,
    totalDeletions,
  };
}
