import { type MouseEvent, useEffect, useMemo, useState } from "react";
import { useQuery } from "@tanstack/react-query";

import { diffApi } from "@/api/diff";
import type { FileChange } from "@/api/diff";
import type { AgentConversationWorkspace } from "@/api/chat";
import { cn } from "@/lib/utils";

import { AgentsPublishDiffFilter } from "./AgentsPublishDiffFilter";
import type { DiffFilterMode } from "./AgentsPublishDiffFilter";
import {
  AGENT_WORKSPACE_STALE_MS,
  agentWorkspaceKeys,
} from "./agentWorkspaceQueries";
import { useDeferredAgentHydration } from "./useDeferredAgentHydration";
import {
  mapReviewCommitsToDiffViewerCommits,
  useAgentWorkspaceChangeSummary,
} from "./useAgentWorkspaceChangeSummary";

interface AgentsComposerWorkspaceChangesCardProps {
  conversationId: string;
  workspace: AgentConversationWorkspace | null;
  isFocusedChildChat: boolean;
  pauseHydration?: boolean;
  onOpenFile: (filePath: string, mode: DiffFilterMode) => void;
  onPreloadPublishPane: () => void;
}

function statusLabel(status: FileChange["status"]): string {
  switch (status) {
    case "added":
      return "Added";
    case "deleted":
      return "Deleted";
    default:
      return "Modified";
  }
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

export function AgentsComposerWorkspaceChangesCard({
  conversationId,
  workspace,
  isFocusedChildChat,
  pauseHydration = false,
  onOpenFile,
  onPreloadPublishPane,
}: AgentsComposerWorkspaceChangesCardProps) {
  const canRender =
    !isFocusedChildChat && workspace?.mode === "edit" && workspace.status !== "missing";
  if (!canRender) {
    return null;
  }

  return (
    <AgentsComposerWorkspaceChangesCardContent
      conversationId={conversationId}
      pauseHydration={pauseHydration}
      onOpenFile={onOpenFile}
      onPreloadPublishPane={onPreloadPublishPane}
    />
  );
}

function AgentsComposerWorkspaceChangesCardContent({
  conversationId,
  pauseHydration,
  onOpenFile,
  onPreloadPublishPane,
}: {
  conversationId: string;
  pauseHydration: boolean;
  onOpenFile: (filePath: string, mode: DiffFilterMode) => void;
  onPreloadPublishPane: () => void;
}) {
  const [isExpanded, setIsExpanded] = useState(false);
  const canScheduleReviewHydration = useDeferredAgentHydration(conversationId);
  const [canHydrateReview, setCanHydrateReview] = useState(false);
  useEffect(() => {
    setCanHydrateReview(false);
    if (!canScheduleReviewHydration || pauseHydration) {
      return;
    }

    const timer = window.setTimeout(() => {
      setCanHydrateReview(true);
    }, 900);

    return () => window.clearTimeout(timer);
  }, [canScheduleReviewHydration, conversationId, pauseHydration]);
  const reviewQuery = useQuery({
    queryKey: agentWorkspaceKeys.review(conversationId),
    queryFn: () => diffApi.getAgentConversationWorkspaceReview(conversationId),
    enabled: canHydrateReview,
    staleTime: AGENT_WORKSPACE_STALE_MS,
  });
  const review = reviewQuery.data ?? null;
  const commits = useMemo(() => mapReviewCommitsToDiffViewerCommits(review), [review]);
  const summary = useAgentWorkspaceChangeSummary({ conversationId, review });
  const shouldShow =
    reviewQuery.isSuccess &&
    (summary.workspaceChangeCount > 0 || summary.currentFiles.length > 0);

  if (!shouldShow) {
    return null;
  }

  const fileLabel = `${summary.currentFiles.length} ${
    summary.currentFiles.length === 1 ? "file" : "files"
  }`;
  const toggleExpanded = () => setIsExpanded((value) => !value);
  const handleHeaderClick = (event: MouseEvent<HTMLDivElement>) => {
    if (event.target === event.currentTarget) {
      toggleExpanded();
    }
  };

  return (
    <div
      data-testid="agents-composer-workspace-changes"
      className="mb-1.5 px-1"
      onPointerEnter={onPreloadPublishPane}
      onFocusCapture={onPreloadPublishPane}
    >
      <div
        data-testid="agents-composer-workspace-changes-header"
        className="flex min-h-7 min-w-0 flex-wrap items-center gap-2"
        onClick={handleHeaderClick}
      >
        <AgentsPublishDiffFilter
          mode={summary.effectiveMode}
          workspaceChangeCount={summary.workspaceChangeCount}
          {...(summary.stagedCount !== undefined && { stagedCount: summary.stagedCount })}
          {...(summary.unstagedCount !== undefined && { unstagedCount: summary.unstagedCount })}
          commits={commits}
          supportsWorktreeModes={summary.supportsWorktreeModes}
          onModeChange={summary.setMode}
        />
        <div className="ml-auto flex min-w-0 items-center gap-1.5">
          <button
            type="button"
            data-testid="agents-composer-workspace-changes-count"
            aria-expanded={isExpanded}
            onClick={toggleExpanded}
            className="rounded px-1.5 py-1 text-[0.6875rem] font-medium transition-colors hover:bg-[var(--bg-hover)]"
            style={{ color: "var(--text-secondary)" }}
          >
            {fileLabel}
          </button>
          <button
            type="button"
            data-testid="agents-composer-workspace-changes-additions"
            aria-expanded={isExpanded}
            onClick={toggleExpanded}
            className={cn(
              "rounded px-1 py-1 font-mono text-[0.6875rem] font-medium transition-colors hover:bg-[var(--bg-hover)]",
              summary.totalAdditions === 0 && "opacity-60",
            )}
            style={{ color: "var(--status-success)" }}
          >
            +{summary.totalAdditions}
          </button>
          <button
            type="button"
            data-testid="agents-composer-workspace-changes-deletions"
            aria-expanded={isExpanded}
            onClick={toggleExpanded}
            className={cn(
              "rounded px-1 py-1 font-mono text-[0.6875rem] font-medium transition-colors hover:bg-[var(--bg-hover)]",
              summary.totalDeletions === 0 && "opacity-60",
            )}
            style={{ color: "var(--status-error)" }}
          >
            −{summary.totalDeletions}
          </button>
        </div>
      </div>

      {isExpanded && (
        <div
          className="mt-1.5 max-h-44 overflow-y-auto rounded border"
          data-testid="agents-composer-workspace-changes-list"
          style={{
            backgroundColor: "var(--bg-base)",
            borderColor: "var(--border-subtle)",
            borderStyle: "solid",
            borderWidth: "1px",
          }}
        >
          {summary.isCurrentFilesLoading ? (
            <div
              className="px-2 py-2 text-xs"
              style={{ color: "var(--text-muted)" }}
            >
              Loading files...
            </div>
          ) : summary.currentFilesError ? (
            <div
              className="px-2 py-2 text-xs"
              style={{ color: "var(--text-muted)" }}
            >
              Could not load files
            </div>
          ) : (
            summary.currentFiles.map((file) => (
              <button
                key={file.path}
                type="button"
                data-testid={`agents-composer-workspace-file-${file.path}`}
                aria-label={`Open ${file.path} in Commit & Publish`}
                onClick={() => onOpenFile(file.path, summary.effectiveMode)}
                className="flex w-full min-w-0 items-center gap-2 px-2 py-1.5 text-left transition-colors hover:bg-[var(--bg-hover)] focus-visible:[outline:1px_solid_var(--accent-border)] focus-visible:[outline-offset:-1px]"
                style={{ color: "var(--text-secondary)" }}
              >
                <span
                  className="w-4 shrink-0 text-center text-[0.6875rem] font-semibold"
                  style={{ color: statusColor(file.status) }}
                >
                  {statusLetter(file.status)}
                </span>
                <span className="min-w-0 flex-1 truncate font-mono text-[0.7188rem]">
                  {file.path}
                </span>
                <span
                  className="hidden shrink-0 text-[0.6875rem] sm:inline"
                  style={{ color: "var(--text-muted)" }}
                >
                  {statusLabel(file.status)}
                </span>
                {file.isGenerated && (
                  <span
                    className="shrink-0 rounded border px-1 py-0.5 text-[0.625rem]"
                    style={{
                      borderColor: "var(--border-subtle)",
                      color: "var(--text-muted)",
                    }}
                  >
                    Generated
                  </span>
                )}
                <span
                  className="shrink-0 font-mono text-[0.6875rem]"
                  style={{ color: "var(--status-success)" }}
                >
                  +{file.additions}
                </span>
                <span
                  className="shrink-0 font-mono text-[0.6875rem]"
                  style={{ color: "var(--status-error)" }}
                >
                  −{file.deletions}
                </span>
              </button>
            ))
          )}
        </div>
      )}
    </div>
  );
}
