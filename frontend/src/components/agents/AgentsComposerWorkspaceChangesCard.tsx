import { type MouseEvent, useEffect, useMemo, useRef, useState } from "react";
import { useQuery } from "@tanstack/react-query";

import { agentTaskApi } from "@/api/agent-tasks";
import type { AgentTaskState, AgentTaskSummary } from "@/api/agent-tasks";
import { diffApi } from "@/api/diff";
import type { FileChange } from "@/api/diff";
import type { AgentConversationWorkspace } from "@/api/chat";
import { cn } from "@/lib/utils";

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

const ACTIVE_AGENT_REVIEW_REFRESH_MS = 2_500;
const ACTIVE_AGENT_TASK_REFRESH_MS = 2_500;
const EMPTY_AGENT_TASKS: AgentTaskSummary[] = [];

type ComposerContextPanel = "tasks" | "changes";

interface AgentsComposerWorkspaceChangesCardProps {
  conversationId: string;
  projectId?: string | null | undefined;
  workspace: AgentConversationWorkspace | null;
  isFocusedChildChat: boolean;
  isAgentGenerating?: boolean;
  pauseHydration?: boolean;
  onOpenFile: (filePath: string, mode: DiffFilterMode) => void;
  onPreloadPublishPane: () => void;
}

function taskStateLabel(state: AgentTaskState): string {
  switch (state) {
    case "active":
      return "In progress";
    case "done":
      return "Done";
    case "dropped":
      return "Dropped";
    default:
      return "Open";
  }
}

function taskStateColor(state: AgentTaskState): string {
  switch (state) {
    case "active":
      return "var(--accent-primary)";
    case "done":
      return "var(--status-success)";
    case "dropped":
      return "var(--text-muted)";
    default:
      return "var(--text-secondary)";
  }
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
  projectId,
  workspace,
  isFocusedChildChat,
  isAgentGenerating = false,
  pauseHydration = false,
  onOpenFile,
  onPreloadPublishPane,
}: AgentsComposerWorkspaceChangesCardProps) {
  const canRender = !isFocusedChildChat;
  if (!canRender) {
    return null;
  }

  return (
    <AgentsComposerWorkspaceChangesCardContent
      conversationId={conversationId}
      projectId={projectId}
      workspace={workspace}
      isAgentGenerating={isAgentGenerating}
      pauseHydration={pauseHydration}
      onOpenFile={onOpenFile}
      onPreloadPublishPane={onPreloadPublishPane}
    />
  );
}

function AgentsComposerWorkspaceChangesCardContent({
  conversationId,
  projectId,
  workspace,
  isAgentGenerating,
  pauseHydration,
  onOpenFile,
  onPreloadPublishPane,
}: {
  conversationId: string;
  projectId?: string | null | undefined;
  workspace: AgentConversationWorkspace | null;
  isAgentGenerating: boolean;
  pauseHydration: boolean;
  onOpenFile: (filePath: string, mode: DiffFilterMode) => void;
  onPreloadPublishPane: () => void;
}) {
  const [activePanel, setActivePanel] = useState<ComposerContextPanel | null>(null);
  const isReviewRefreshInFlight = useRef(false);
  const reviewRefetchRef = useRef<(() => Promise<unknown>) | null>(null);
  const canInspectChanges =
    workspace?.mode === "edit" && workspace.status !== "missing";
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
    enabled: canInspectChanges && canHydrateReview,
    staleTime: AGENT_WORKSPACE_STALE_MS,
  });
  const tasksQuery = useQuery({
    queryKey: agentWorkspaceKeys.agentTasks(conversationId),
    queryFn: () =>
      agentTaskApi.listConversationTasks({
        conversationId,
        projectId,
        includeDone: true,
      }),
    enabled: canHydrateReview,
    staleTime: AGENT_WORKSPACE_STALE_MS,
    refetchInterval:
      canHydrateReview && isAgentGenerating ? ACTIVE_AGENT_TASK_REFRESH_MS : false,
  });
  useEffect(() => {
    reviewRefetchRef.current = reviewQuery.refetch;
  }, [reviewQuery.refetch]);
  useEffect(() => {
    if (!canInspectChanges || !canHydrateReview || !isAgentGenerating) {
      isReviewRefreshInFlight.current = false;
      return;
    }

    const timer = window.setInterval(() => {
      if (isReviewRefreshInFlight.current) {
        return;
      }
      const refetchReview = reviewRefetchRef.current;
      if (!refetchReview) {
        return;
      }
      isReviewRefreshInFlight.current = true;
      void refetchReview().finally(() => {
        isReviewRefreshInFlight.current = false;
      });
    }, ACTIVE_AGENT_REVIEW_REFRESH_MS);

    return () => window.clearInterval(timer);
  }, [canHydrateReview, canInspectChanges, conversationId, isAgentGenerating]);
  const review = reviewQuery.data ?? null;
  const commits = useMemo(() => mapReviewCommitsToDiffViewerCommits(review), [review]);
  const summary = useAgentWorkspaceChangeSummary({ conversationId, review });
  const hasCommitSummary = summary.workspaceChangeCount === 0 && commits.length > 0;
  const summaryMode = summary.mode;
  const setSummaryMode = summary.setMode;
  useEffect(() => {
    if (
      !canInspectChanges ||
      !reviewQuery.isSuccess ||
      !hasCommitSummary ||
      summaryMode === "cumulative"
    ) {
      return;
    }
    setSummaryMode("cumulative");
  }, [
    canInspectChanges,
    hasCommitSummary,
    reviewQuery.isSuccess,
    setSummaryMode,
    summaryMode,
  ]);
  const tasks = tasksQuery.data ?? EMPTY_AGENT_TASKS;
  const taskNumberById = useMemo(
    () => new Map(tasks.map((task) => [task.taskId, task.taskNumber])),
    [tasks],
  );
  const shouldShowTasks = tasksQuery.isSuccess && tasks.length > 0;
  const shouldShowChanges =
    canInspectChanges &&
    reviewQuery.isSuccess &&
    (summary.workspaceChangeCount > 0 ||
      summary.currentFiles.length > 0 ||
      hasCommitSummary);
  const shouldShow = shouldShowTasks || shouldShowChanges;

  useEffect(() => {
    if (activePanel === "tasks" && !shouldShowTasks) {
      setActivePanel(null);
    }
    if (activePanel === "changes" && !shouldShowChanges) {
      setActivePanel(null);
    }
  }, [activePanel, shouldShowChanges, shouldShowTasks]);

  if (!shouldShow) {
    return null;
  }

  const changesLabel = hasCommitSummary ? "All commits" : "Workspace changes";
  const fileLabel = `${summary.currentFiles.length} ${
    summary.currentFiles.length === 1 ? "file" : "files"
  }`;
  const changesCountLabel = hasCommitSummary
    ? `${commits.length} ${commits.length === 1 ? "commit" : "commits"}`
    : fileLabel;
  const taskLabel = `${tasks.length}`;
  const togglePanel = (panel: ComposerContextPanel) =>
    setActivePanel((current) => (current === panel ? null : panel));
  const handleHeaderClick = (event: MouseEvent<HTMLDivElement>) => {
    if (event.target === event.currentTarget) {
      togglePanel(shouldShowChanges ? "changes" : "tasks");
    }
  };
  const formatTaskRef = (taskId: string) => {
    const taskNumber = taskNumberById.get(taskId);
    return taskNumber ? `#${taskNumber}` : `#${taskId}`;
  };
  const handlePublishIntent = () => {
    if (shouldShowChanges) {
      onPreloadPublishPane();
    }
  };

  return (
    <div
      data-testid="agents-composer-context-tray"
      className="mb-1.5 px-1"
      onPointerEnter={handlePublishIntent}
      onFocusCapture={handlePublishIntent}
    >
      <div data-testid="agents-composer-workspace-changes">
        <div
          data-testid="agents-composer-workspace-changes-header"
          className="flex min-h-7 min-w-0 flex-wrap items-center gap-1.5"
          onClick={handleHeaderClick}
        >
          {shouldShowTasks && (
            <button
              type="button"
              data-testid="agents-composer-tasks-toggle"
              aria-expanded={activePanel === "tasks"}
              onClick={() => togglePanel("tasks")}
              className={cn(
                "inline-flex h-7 max-w-full min-w-0 items-center gap-1.5 overflow-hidden rounded border px-2 text-[0.6875rem] font-medium transition-colors hover:bg-[var(--bg-hover)]",
                activePanel === "tasks" && "bg-[var(--bg-hover)]",
              )}
              style={{
                borderColor: "var(--border-subtle)",
                color: "var(--text-secondary)",
              }}
            >
              <span>Tasks</span>
              <span
                data-testid="agents-composer-tasks-count"
                className="font-mono"
                style={{ color: "var(--text-muted)" }}
              >
                {taskLabel}
              </span>
            </button>
          )}
          {shouldShowChanges && (
            <button
              type="button"
              data-testid="diff-filter-trigger"
              aria-expanded={activePanel === "changes"}
              onClick={() => togglePanel("changes")}
              className={cn(
                "inline-flex h-7 max-w-full min-w-0 items-center gap-1.5 overflow-hidden rounded border px-2 text-[0.6875rem] font-medium transition-colors hover:bg-[var(--bg-hover)]",
                activePanel === "changes" && "bg-[var(--bg-hover)]",
              )}
              style={{
                borderColor: "var(--border-subtle)",
                color: "var(--text-secondary)",
              }}
            >
              <span className="truncate">{changesLabel}</span>
              <span
                data-testid="agents-composer-workspace-changes-count"
                className="shrink-0"
                style={{ color: "var(--text-muted)" }}
              >
                {changesCountLabel}
              </span>
              <span
                data-testid="agents-composer-workspace-changes-additions"
                className={cn(
                  "shrink-0 font-mono",
                  summary.totalAdditions === 0 && "opacity-60",
                )}
                style={{ color: "var(--status-success)" }}
              >
                +{summary.totalAdditions}
              </span>
              <span
                data-testid="agents-composer-workspace-changes-deletions"
                className={cn(
                  "shrink-0 font-mono",
                  summary.totalDeletions === 0 && "opacity-60",
                )}
                style={{ color: "var(--status-error)" }}
              >
                −{summary.totalDeletions}
              </span>
            </button>
          )}
        </div>

        {activePanel && (
          <div
            className="mt-1.5 max-h-44 overflow-y-auto rounded border"
            data-testid="agents-composer-context-tray-body"
            style={{
              backgroundColor: "var(--bg-base)",
              borderColor: "var(--border-subtle)",
              borderStyle: "solid",
              borderWidth: "1px",
            }}
          >
            {activePanel === "tasks" ? (
              <div data-testid="agents-composer-task-list">
                {tasks.map((task) => (
                  <div
                    key={task.taskId}
                    data-testid={`agents-composer-task-${task.taskNumber}`}
                    className="flex min-w-0 items-center gap-2 overflow-hidden px-2 py-1.5"
                    style={{ color: "var(--text-secondary)" }}
                  >
                    <span
                      className="w-8 shrink-0 font-mono text-[0.6875rem] font-semibold"
                      style={{ color: "var(--text-muted)" }}
                    >
                      #{task.taskNumber}
                    </span>
                    <span
                      className="shrink-0 rounded border px-1.5 py-0.5 text-[0.625rem] font-medium"
                      style={{
                        borderColor: "var(--border-subtle)",
                        color: taskStateColor(task.state),
                      }}
                    >
                      {taskStateLabel(task.state)}
                    </span>
                    <span className="min-w-0 flex-1 truncate text-[0.7188rem]">
                      {task.title}
                    </span>
                    {task.ownerAgent && (
                      <span
                        className="hidden shrink-0 text-[0.6875rem] sm:inline"
                        style={{ color: "var(--text-muted)" }}
                      >
                        {task.ownerAgent}
                      </span>
                    )}
                    {task.blockedBy.length > 0 && (
                      <span
                        className="hidden max-w-[9rem] shrink-0 truncate text-[0.6875rem] sm:inline"
                        style={{ color: "var(--text-muted)" }}
                      >
                        blocked by {task.blockedBy.map(formatTaskRef).join(", ")}
                      </span>
                    )}
                  </div>
                ))}
              </div>
            ) : (
              <div data-testid="agents-composer-workspace-changes-list">
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
        )}
      </div>
    </div>
  );
}
