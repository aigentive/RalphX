import { type MouseEvent, useEffect, useMemo, useRef, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { Check, ChevronDown, ChevronRight, Loader2 } from "lucide-react";

import { agentTaskApi } from "@/api/agent-tasks";
import type {
  AgentTaskListSummary,
  AgentTaskState,
  AgentTaskSummary,
} from "@/api/agent-tasks";
import { diffApi } from "@/api/diff";
import type { FileChange } from "@/api/diff";
import type { AgentConversationWorkspace } from "@/api/chat";
import { cn } from "@/lib/utils";

import { canInspectAgentWorkspacePublishDiffs } from "./agentWorkspacePublishState";
import type { DiffFilterMode } from "./AgentsPublishDiffFilter";
import {
  AGENT_WORKSPACE_STALE_MS,
  agentWorkspaceKeys,
} from "./agentWorkspaceQueries";
import { useDeferredAgentHydration } from "./useDeferredAgentHydration";
import { useAgentWorkspaceChangeSummary } from "./useAgentWorkspaceChangeSummary";

const ACTIVE_AGENT_CHANGE_SUMMARY_REFRESH_MS = 2_500;
const ACTIVE_AGENT_TASK_REFRESH_MS = 2_500;
const EMPTY_AGENT_TASKS: AgentTaskSummary[] = [];
const EMPTY_AGENT_TASK_LISTS: AgentTaskListSummary[] = [];
const VISIBLE_TASK_COUNT = 3;
const TASK_ROW_HEIGHT_PX = 36;

type ComposerContextPanel = "tasks" | "changes";

interface VisibleTaskWindow {
  tasks: AgentTaskSummary[];
  hiddenBefore: number;
  hiddenAfter: number;
}

interface AgentTaskLedgerContext {
  contextType: string;
  contextId: string;
}

interface AgentsComposerWorkspaceChangesCardProps {
  conversationId: string;
  projectId?: string | null | undefined;
  workspace: AgentConversationWorkspace | null;
  isFocusedChildChat: boolean;
  taskLedgerContext: AgentTaskLedgerContext | null;
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

function taskSignature(task: AgentTaskSummary): string {
  return JSON.stringify([
    task.title,
    task.state,
    task.ownerAgent,
    task.availability,
    task.updatedAt,
    task.blockedBy,
    task.blocks,
  ]);
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

function formatTaskRef(taskId: string, taskNumberById: Map<string, number>): string {
  const taskNumber = taskNumberById.get(taskId);
  return taskNumber ? `#${taskNumber}` : `#${taskId}`;
}

function taskListStatusLabel(list: AgentTaskListSummary): string {
  if (list.activeCount > 0) {
    return "In progress";
  }
  if (list.openCount > 0) {
    return "Open";
  }
  if (list.taskCount > 0 && list.doneCount + list.droppedCount === list.taskCount) {
    return "Done";
  }
  return `${list.doneCount}/${list.taskCount}`;
}

function taskCountText(count: number): string {
  return `${count} ${count === 1 ? "task" : "tasks"}`;
}

function visibleTaskWindow(
  tasks: AgentTaskSummary[],
  showAllTasks: boolean,
  visibleTaskCount: number,
): VisibleTaskWindow {
  if (showAllTasks || tasks.length <= visibleTaskCount) {
    return {
      tasks,
      hiddenBefore: 0,
      hiddenAfter: 0,
    };
  }

  const firstActiveIndex = tasks.findIndex((task) => task.state === "active");
  const latestWindowStart = Math.max(tasks.length - visibleTaskCount, 0);
  const windowStart = firstActiveIndex >= 0
    ? Math.min(firstActiveIndex, latestWindowStart)
    : latestWindowStart;
  const windowEnd = Math.min(windowStart + visibleTaskCount, tasks.length);

  return {
    tasks: tasks.slice(windowStart, windowEnd),
    hiddenBefore: windowStart,
    hiddenAfter: tasks.length - windowEnd,
  };
}

function AgentTaskRowLine({
  task,
  taskNumberById,
  testId,
  highlighted = false,
  registerNode,
}: {
  task: AgentTaskSummary;
  taskNumberById: Map<string, number>;
  testId: string;
  highlighted?: boolean;
  registerNode?: (node: HTMLDivElement | null) => void;
}) {
  return (
    <div
      ref={registerNode}
      data-testid={testId}
      className="flex min-w-0 items-center gap-2 overflow-hidden px-2 py-1.5 transition-colors"
      style={{
        backgroundColor: highlighted ? "var(--bg-hover)" : "transparent",
        color: "var(--text-secondary)",
      }}
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
          blocked by {task.blockedBy.map((taskId) => formatTaskRef(taskId, taskNumberById)).join(", ")}
        </span>
      )}
    </div>
  );
}

export function AgentsComposerWorkspaceChangesCard({
  conversationId,
  projectId,
  workspace,
  isFocusedChildChat,
  taskLedgerContext,
  isAgentGenerating = false,
  pauseHydration = false,
  onOpenFile,
  onPreloadPublishPane,
}: AgentsComposerWorkspaceChangesCardProps) {
  if (!taskLedgerContext) {
    return null;
  }

  return (
    <AgentsComposerWorkspaceChangesCardContent
      conversationId={conversationId}
      projectId={projectId}
      workspace={workspace}
      isFocusedChildChat={isFocusedChildChat}
      taskLedgerContext={taskLedgerContext}
      isAgentGenerating={isAgentGenerating}
      pauseHydration={pauseHydration}
      onOpenFile={onOpenFile}
      onPreloadPublishPane={onPreloadPublishPane}
    />
  );
}

function PreviousTaskListDisclosure({
  taskLedgerContext,
  projectId,
  list,
  expanded,
  onToggle,
}: {
  taskLedgerContext: AgentTaskLedgerContext;
  projectId?: string | null | undefined;
  list: AgentTaskListSummary;
  expanded: boolean;
  onToggle: () => void;
}) {
  const tasksQuery = useQuery({
    queryKey: agentWorkspaceKeys.agentTaskListTasksForScope(
      taskLedgerContext.contextType,
      taskLedgerContext.contextId,
      list.listId,
    ),
    queryFn: () =>
      agentTaskApi.listAgentTasksForList({
        contextType: taskLedgerContext.contextType,
        contextId: taskLedgerContext.contextId,
        projectId,
        listId: list.listId,
        includeDone: true,
      }),
    enabled: expanded,
    staleTime: AGENT_WORKSPACE_STALE_MS,
  });
  const tasks = tasksQuery.data ?? EMPTY_AGENT_TASKS;
  const taskNumberById = useMemo(
    () => new Map(tasks.map((task) => [task.taskId, task.taskNumber])),
    [tasks],
  );

  return (
    <div data-testid={`agents-composer-task-list-slice-${list.listId}`}>
      <button
        type="button"
        aria-expanded={expanded}
        onClick={onToggle}
        className="flex w-full min-w-0 items-center gap-1.5 px-2 py-1.5 text-left transition-colors hover:bg-[var(--bg-hover)]"
        style={{ color: "var(--text-secondary)" }}
      >
        {expanded ? (
          <ChevronDown className="h-3 w-3 shrink-0" style={{ color: "var(--text-muted)" }} />
        ) : (
          <ChevronRight className="h-3 w-3 shrink-0" style={{ color: "var(--text-muted)" }} />
        )}
        <span className="min-w-0 flex-1 truncate text-[0.6875rem]">
          Task list #{list.listSequence}
        </span>
        <span
          className="shrink-0 text-[0.625rem]"
          style={{ color: "var(--text-muted)" }}
        >
          {taskCountText(list.taskCount)}
        </span>
        <span
          className="shrink-0 rounded border px-1.5 py-0.5 text-[0.625rem] font-medium"
          style={{
            borderColor: "var(--border-subtle)",
            color: list.activeCount > 0 ? "var(--accent-primary)" : "var(--text-muted)",
          }}
        >
          {taskListStatusLabel(list)}
        </span>
      </button>
      {expanded && (
        <div data-testid={`agents-composer-task-list-slice-${list.listId}-tasks`}>
          {tasksQuery.isLoading ? (
            <div
              className="px-8 py-1.5 text-[0.6875rem]"
              style={{ color: "var(--text-muted)" }}
            >
              Loading tasks...
            </div>
          ) : tasksQuery.isError ? (
            <div
              className="px-8 py-1.5 text-[0.6875rem]"
              style={{ color: "var(--text-muted)" }}
            >
              Could not load tasks
            </div>
          ) : (
            tasks.map((task) => (
              <AgentTaskRowLine
                key={task.taskId}
                task={task}
                taskNumberById={taskNumberById}
                testId={`agents-composer-task-list-${list.listId}-task-${task.taskNumber}`}
              />
            ))
          )}
        </div>
      )}
    </div>
  );
}

function AgentsComposerWorkspaceChangesCardContent({
  conversationId,
  projectId,
  workspace,
  isFocusedChildChat,
  taskLedgerContext,
  isAgentGenerating,
  pauseHydration,
  onOpenFile,
  onPreloadPublishPane,
}: {
  conversationId: string;
  projectId?: string | null | undefined;
  workspace: AgentConversationWorkspace | null;
  isFocusedChildChat: boolean;
  taskLedgerContext: AgentTaskLedgerContext;
  isAgentGenerating: boolean;
  pauseHydration: boolean;
  onOpenFile: (filePath: string, mode: DiffFilterMode) => void;
  onPreloadPublishPane: () => void;
}) {
  const [activePanel, setActivePanel] = useState<ComposerContextPanel | null>(null);
  const [highlightedTaskId, setHighlightedTaskId] = useState<string | null>(null);
  const wasAgentGenerating = useRef(false);
  const previousIsAgentGenerating = useRef(false);
  const userDismissedTaskPanel = useRef(false);
  const hasObservedTaskSnapshot = useRef(false);
  const previousTaskSignatures = useRef<Map<string, string>>(new Map());
  const taskRowRefs = useRef<Map<string, HTMLDivElement>>(new Map());
  const canInspectChanges =
    !isFocusedChildChat && canInspectAgentWorkspacePublishDiffs(workspace);
  const taskLedgerScopeKey = `${taskLedgerContext.contextType}:${taskLedgerContext.contextId}`;
  const canScheduleReviewHydration = useDeferredAgentHydration(
    `${conversationId}:${taskLedgerScopeKey}`,
  );
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
  const changeSummaryQuery = useQuery({
    queryKey: agentWorkspaceKeys.changeSummary(conversationId),
    queryFn: () => diffApi.getAgentConversationWorkspaceChangeSummary(conversationId),
    enabled: canInspectChanges && canHydrateReview,
    staleTime: AGENT_WORKSPACE_STALE_MS,
    refetchInterval:
      canInspectChanges && canHydrateReview && isAgentGenerating
        ? ACTIVE_AGENT_CHANGE_SUMMARY_REFRESH_MS
        : false,
  });
  const tasksQuery = useQuery({
    queryKey: agentWorkspaceKeys.agentTasksForScope(
      taskLedgerContext.contextType,
      taskLedgerContext.contextId,
    ),
    queryFn: () =>
      agentTaskApi.listAgentTasks({
        contextType: taskLedgerContext.contextType,
        contextId: taskLedgerContext.contextId,
        projectId,
        includeDone: true,
      }),
    enabled: canHydrateReview,
    staleTime: AGENT_WORKSPACE_STALE_MS,
    refetchInterval:
      canHydrateReview && isAgentGenerating ? ACTIVE_AGENT_TASK_REFRESH_MS : false,
  });
  const taskListsQuery = useQuery({
    queryKey: agentWorkspaceKeys.agentTaskListsForScope(
      taskLedgerContext.contextType,
      taskLedgerContext.contextId,
    ),
    queryFn: () =>
      agentTaskApi.listAgentTaskLists({
        contextType: taskLedgerContext.contextType,
        contextId: taskLedgerContext.contextId,
        projectId,
      }),
    enabled: canHydrateReview && activePanel === "tasks",
    staleTime: AGENT_WORKSPACE_STALE_MS,
    refetchInterval:
      canHydrateReview && activePanel === "tasks" && isAgentGenerating
        ? ACTIVE_AGENT_TASK_REFRESH_MS
        : false,
  });
  const refetchTasks = tasksQuery.refetch;
  const refetchTaskLists = taskListsQuery.refetch;
  useEffect(() => {
    if (isAgentGenerating) {
      wasAgentGenerating.current = true;
      return;
    }
    if (!canHydrateReview || !wasAgentGenerating.current) {
      return;
    }
    wasAgentGenerating.current = false;
    void refetchTasks();
    if (activePanel === "tasks") {
      void refetchTaskLists();
    }
  }, [activePanel, canHydrateReview, isAgentGenerating, refetchTaskLists, refetchTasks]);
  const liveSummary = changeSummaryQuery.data ?? null;
  const summary = useAgentWorkspaceChangeSummary({
    conversationId,
    review: null,
    liveSummary,
    hydrateWorktreeFileLists: activePanel === "changes",
  });
  const tasks = tasksQuery.data ?? EMPTY_AGENT_TASKS;
  const taskLists = taskListsQuery.data ?? EMPTY_AGENT_TASK_LISTS;
  const previousTaskLists = useMemo(() => taskLists.slice(1), [taskLists]);
  const taskNumberById = useMemo(
    () => new Map(tasks.map((task) => [task.taskId, task.taskNumber])),
    [tasks],
  );
  const [showAllTasks, setShowAllTasks] = useState(false);
  const [showPreviousTaskLists, setShowPreviousTaskLists] = useState(false);
  const [expandedTaskListIds, setExpandedTaskListIds] = useState<Set<string>>(
    () => new Set(),
  );
  const taskListRef = useRef<HTMLDivElement>(null);
  const taskProgress = useMemo(() => {
    const actionable = tasks.filter((t) => t.state !== "dropped");
    const done = actionable.filter((t) => t.state === "done").length;
    const active = actionable.filter((t) => t.state === "active").length;
    return { actionable: actionable.length, done, active, total: tasks.length };
  }, [tasks]);
  const shouldShowTasks = tasksQuery.isSuccess && tasks.length > 0;
  const shouldShowChanges =
    canInspectChanges &&
    changeSummaryQuery.isSuccess &&
    (summary.workspaceChangeCount > 0 ||
      summary.currentFiles.length > 0);
  const shouldShow = shouldShowTasks || shouldShowChanges;

  useEffect(() => {
    setActivePanel(null);
    setHighlightedTaskId(null);
    setShowAllTasks(false);
    setShowPreviousTaskLists(false);
    setExpandedTaskListIds(new Set());
    userDismissedTaskPanel.current = false;
    hasObservedTaskSnapshot.current = false;
    previousTaskSignatures.current = new Map();
    taskRowRefs.current.clear();
  }, [conversationId, taskLedgerScopeKey]);

  useEffect(() => {
    if (isAgentGenerating && !previousIsAgentGenerating.current) {
      userDismissedTaskPanel.current = false;
    }
    previousIsAgentGenerating.current = isAgentGenerating;
  }, [isAgentGenerating]);

  useEffect(() => {
    if (!tasksQuery.isSuccess) {
      return;
    }

    const currentSignatures = new Map(
      tasks.map((task) => [task.taskId, taskSignature(task)]),
    );
    const previousSignatures = previousTaskSignatures.current;
    const hadObservedSnapshot = hasObservedTaskSnapshot.current;

    previousTaskSignatures.current = currentSignatures;
    hasObservedTaskSnapshot.current = true;

    if (tasks.length === 0) {
      setHighlightedTaskId(null);
      return;
    }

    const changedTask = tasks.find(
      (task) => previousSignatures.get(task.taskId) !== currentSignatures.get(task.taskId),
    );
    if (!changedTask) {
      return;
    }

    if (!hadObservedSnapshot && !isAgentGenerating) {
      return;
    }

    setHighlightedTaskId(changedTask.taskId);
    if (!userDismissedTaskPanel.current) {
      setActivePanel("tasks");
    }
  }, [isAgentGenerating, tasks, tasksQuery.isSuccess]);

  useEffect(() => {
    if (activePanel !== "tasks" || !highlightedTaskId) {
      return;
    }

    const scrollTimer = window.setTimeout(() => {
      const node = taskRowRefs.current.get(highlightedTaskId);
      if (typeof node?.scrollIntoView === "function") {
        node.scrollIntoView({
          block: "nearest",
          inline: "nearest",
          behavior: "smooth",
        });
      }
    }, 0);
    const highlightTimer = window.setTimeout(() => {
      setHighlightedTaskId((current) =>
        current === highlightedTaskId ? null : current,
      );
    }, 2_200);

    return () => {
      window.clearTimeout(scrollTimer);
      window.clearTimeout(highlightTimer);
    };
  }, [activePanel, highlightedTaskId]);

  useEffect(() => {
    if (activePanel !== "tasks") {
      return;
    }
    const container = taskListRef.current;
    if (!container) {
      return;
    }
    const lastActive = [...tasks].reverse().find((t) => t.state === "active");
    if (lastActive) {
      const node = taskRowRefs.current.get(lastActive.taskId);
      if (typeof node?.scrollIntoView === "function") {
        node.scrollIntoView({ block: "nearest", behavior: "smooth" });
        return;
      }
    }
    container.scrollTop = container.scrollHeight;
  }, [activePanel, tasks]);

  useEffect(() => {
    if (activePanel === "tasks" && !shouldShowTasks) {
      setActivePanel(null);
    }
    if (activePanel === "changes" && !shouldShowChanges) {
      setActivePanel(null);
    }
  }, [activePanel, shouldShowChanges, shouldShowTasks]);

  const taskWindow = useMemo(
    () => visibleTaskWindow(tasks, showAllTasks, VISIBLE_TASK_COUNT),
    [showAllTasks, tasks],
  );
  const visibleTasks = taskWindow.tasks;
  const hiddenTaskRevealRows =
    (taskWindow.hiddenBefore > 0 ? 1 : 0) + (taskWindow.hiddenAfter > 0 ? 1 : 0);
  const taskHistoryRowReserve =
    previousTaskLists.length > 0 || taskListsQuery.isFetching || taskListsQuery.isError
      ? 30
      : 0;

  if (!shouldShow) {
    return null;
  }

  const changesLabel =
    summary.effectiveMode === "unstaged"
      ? "Unstaged"
      : summary.effectiveMode === "staged"
        ? "Staged"
        : "Workspace changes";
  const fileLabel = `${summary.currentFileCount} ${
    summary.currentFileCount === 1 ? "file" : "files"
  }`;
  const changesCountLabel = fileLabel;
  const allDone = taskProgress.actionable > 0 && taskProgress.done === taskProgress.actionable;
  const hasActive = taskProgress.active > 0;
  const taskCountLabel = allDone
    ? `${taskProgress.actionable}`
    : `${taskProgress.done}/${taskProgress.actionable}`;
  const togglePanel = (panel: ComposerContextPanel) =>
    setActivePanel((current) => {
      const nextPanel = current === panel ? null : panel;
      if (panel === "tasks") {
        userDismissedTaskPanel.current = nextPanel !== "tasks";
      } else {
        userDismissedTaskPanel.current = true;
      }
      return nextPanel;
    });
  const handleHeaderClick = (event: MouseEvent<HTMLDivElement>) => {
    if (event.target === event.currentTarget) {
      togglePanel(shouldShowChanges ? "changes" : "tasks");
    }
  };
  const togglePreviousTaskList = (listId: string) => {
    setExpandedTaskListIds((current) => {
      const next = new Set(current);
      if (next.has(listId)) {
        next.delete(listId);
      } else {
        next.add(listId);
      }
      return next;
    });
  };
  const handlePublishIntent = () => {
    if (shouldShowChanges) {
      onPreloadPublishPane();
    }
  };

  const TasksChevron = activePanel === "tasks" ? ChevronDown : ChevronRight;
  const ChangesChevron = activePanel === "changes" ? ChevronDown : ChevronRight;

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
          className={cn(
            "flex min-h-7 min-w-0 flex-wrap items-center gap-1.5",
            activePanel && "mb-0",
          )}
          onClick={handleHeaderClick}
        >
          {shouldShowTasks && (
            <button
              type="button"
              data-testid="agents-composer-tasks-toggle"
              aria-expanded={activePanel === "tasks"}
              onClick={() => togglePanel("tasks")}
              className={cn(
                "inline-flex h-7 max-w-full min-w-0 items-center gap-1 overflow-hidden px-2 text-[0.6875rem] font-medium transition-colors",
                activePanel === "tasks"
                  ? "rounded-t border border-b-0 bg-[var(--bg-base)]"
                  : "rounded hover:bg-[var(--bg-hover)]",
              )}
              style={{
                borderColor: activePanel === "tasks" ? "var(--border-subtle)" : "transparent",
                color: "var(--text-secondary)",
              }}
            >
              <TasksChevron className="h-3 w-3 shrink-0" style={{ color: "var(--text-muted)" }} />
              <span>Tasks</span>
              <span
                data-testid="agents-composer-tasks-count"
                className="font-mono"
                style={{ color: "var(--text-muted)" }}
              >
                {taskCountLabel}
              </span>
              {allDone && (
                <Check className="h-3 w-3 shrink-0" style={{ color: "var(--status-success)" }} />
              )}
              {hasActive && !allDone && (
                <Loader2 className="h-3 w-3 shrink-0 animate-spin" style={{ color: "var(--accent-primary)" }} />
              )}
            </button>
          )}
          {shouldShowChanges && (
            <button
              type="button"
              data-testid="diff-filter-trigger"
              aria-expanded={activePanel === "changes"}
              onClick={() => togglePanel("changes")}
              className={cn(
                "inline-flex h-7 max-w-full min-w-0 items-center gap-1 overflow-hidden px-2 text-[0.6875rem] font-medium transition-colors",
                activePanel === "changes"
                  ? "rounded-t border border-b-0 bg-[var(--bg-base)]"
                  : "rounded hover:bg-[var(--bg-hover)]",
              )}
              style={{
                borderColor: activePanel === "changes" ? "var(--border-subtle)" : "transparent",
                color: "var(--text-secondary)",
              }}
            >
              <ChangesChevron className="h-3 w-3 shrink-0" style={{ color: "var(--text-muted)" }} />
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
            ref={activePanel === "tasks" ? taskListRef : undefined}
            className="overflow-y-auto rounded-b rounded-tr border"
            data-testid="agents-composer-context-tray-body"
            style={{
              backgroundColor: "var(--bg-base)",
              borderColor: "var(--border-subtle)",
              borderStyle: "solid",
              borderWidth: "1px",
              maxHeight: activePanel === "tasks" && !showAllTasks
                ? `${
                    VISIBLE_TASK_COUNT * TASK_ROW_HEIGHT_PX +
                    hiddenTaskRevealRows * 30 +
                    taskHistoryRowReserve
                  }px`
                : "11rem",
            }}
          >
            {activePanel === "tasks" ? (
              <div data-testid="agents-composer-task-list">
                {taskWindow.hiddenBefore > 0 && (
                  <button
                    type="button"
                    data-testid="agents-composer-tasks-show-older"
                    onClick={() => setShowAllTasks(true)}
                    className="flex w-full items-center justify-center py-1 text-[0.625rem] font-medium transition-colors hover:bg-[var(--bg-hover)]"
                    style={{ color: "var(--text-muted)" }}
                  >
                    Show {taskWindow.hiddenBefore} earlier in this list
                  </button>
                )}
                {visibleTasks.map((task) => (
                  <AgentTaskRowLine
                    key={task.taskId}
                    task={task}
                    taskNumberById={taskNumberById}
                    testId={`agents-composer-task-${task.taskNumber}`}
                    highlighted={highlightedTaskId === task.taskId}
                    registerNode={(node) => {
                      if (node) {
                        taskRowRefs.current.set(task.taskId, node);
                      } else {
                        taskRowRefs.current.delete(task.taskId);
                      }
                    }}
                  />
                ))}
                {taskWindow.hiddenAfter > 0 && (
                  <button
                    type="button"
                    data-testid="agents-composer-tasks-show-more"
                    onClick={() => setShowAllTasks(true)}
                    className="flex w-full items-center justify-center py-1 text-[0.625rem] font-medium transition-colors hover:bg-[var(--bg-hover)]"
                    style={{ color: "var(--text-muted)" }}
                  >
                    Show {taskWindow.hiddenAfter} more in this list
                  </button>
                )}
                {taskListsQuery.isLoading && (
                  <div
                    className="px-2 py-1.5 text-[0.6875rem]"
                    style={{
                      borderTopColor: "var(--border-subtle)",
                      borderTopStyle: "solid",
                      borderTopWidth: "1px",
                      color: "var(--text-muted)",
                    }}
                  >
                    Loading previous task lists...
                  </div>
                )}
                {taskListsQuery.isError && (
                  <div
                    className="px-2 py-1.5 text-[0.6875rem]"
                    style={{
                      borderTopColor: "var(--border-subtle)",
                      borderTopStyle: "solid",
                      borderTopWidth: "1px",
                      color: "var(--text-muted)",
                    }}
                  >
                    Could not load previous task lists
                  </div>
                )}
                {!taskListsQuery.isError && previousTaskLists.length > 0 && (
                  <div
                    style={{
                      borderTopColor: "var(--border-subtle)",
                      borderTopStyle: "solid",
                      borderTopWidth: "1px",
                    }}
                  >
                    <button
                      type="button"
                      data-testid="agents-composer-task-lists-show-previous"
                      aria-expanded={showPreviousTaskLists}
                      onClick={() => setShowPreviousTaskLists((current) => !current)}
                      className="flex w-full min-w-0 items-center gap-1.5 px-2 py-1 text-left text-[0.625rem] font-medium transition-colors hover:bg-[var(--bg-hover)]"
                      style={{ color: "var(--text-muted)" }}
                    >
                      {showPreviousTaskLists ? (
                        <ChevronDown className="h-3 w-3 shrink-0" />
                      ) : (
                        <ChevronRight className="h-3 w-3 shrink-0" />
                      )}
                      <span className="min-w-0 flex-1 truncate">
                        Previous task lists
                      </span>
                      <span className="shrink-0 font-mono">
                        {previousTaskLists.length}
                      </span>
                    </button>
                    {showPreviousTaskLists &&
                      previousTaskLists.map((list) => (
                        <PreviousTaskListDisclosure
                          key={list.listId}
                          taskLedgerContext={taskLedgerContext}
                          projectId={projectId}
                          list={list}
                          expanded={expandedTaskListIds.has(list.listId)}
                          onToggle={() => togglePreviousTaskList(list.listId)}
                        />
                      ))}
                  </div>
                )}
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
