import { useEffect, useMemo, useRef } from "react";
import { useQuery } from "@tanstack/react-query";

import {
  agentTaskApi,
  type AgentTaskState,
  type AgentTaskSummary,
} from "@/api/agent-tasks";
import type { AutomationRunStatus } from "@/api/automations";
import {
  AGENT_WORKSPACE_STALE_MS,
  agentWorkspaceKeys,
} from "@/components/agents/agentWorkspaceQueries";
import { FieldLabel } from "./automationDetailShared";
import { automationRunTaskLedgerRefetchInterval } from "./automationRunTaskLedgerPolling";

const STATE_LABELS: Record<AgentTaskState, string> = {
  open: "Open",
  active: "In progress",
  done: "Done",
  dropped: "Dropped",
};

const TERMINAL_SUCCESS_RUN_STATUSES: AutomationRunStatus[] = [
  "merged",
  "completed",
  "published",
];

/**
 * Keep the ledger in sync with the run's terminal state: once a run has merged /
 * completed / published, there is no live work left, so a task the agent left as
 * `active` is shown as `done` rather than a misleading in-progress dot.
 */
function effectiveTaskState(
  state: AgentTaskState,
  runStatus: AutomationRunStatus,
): AgentTaskState {
  if (state === "active" && TERMINAL_SUCCESS_RUN_STATUSES.includes(runStatus)) {
    return "done";
  }
  return state;
}

/** Small status dot per task state — animated for the live/in-progress task. */
function stateDotColor(state: AgentTaskState): string {
  switch (state) {
    case "active":
      return "var(--accent-primary, #ff6a35)";
    case "done":
      return "var(--status-success, #3fbf7f)";
    case "dropped":
      return "var(--status-error, #d55e00)";
    default:
      return "var(--text-subtle, #6b6b73)";
  }
}

function TaskStateDot({ state }: { state: AgentTaskState }) {
  return (
    <span
      role="img"
      aria-label={STATE_LABELS[state]}
      title={STATE_LABELS[state]}
      className={`h-2 w-2 shrink-0 rounded-full${state === "active" ? " animate-pulse" : ""}`}
      style={{ backgroundColor: stateDotColor(state) }}
    />
  );
}

function TaskRow({
  task,
  state,
  isFirst,
}: {
  task: AgentTaskSummary;
  state: AgentTaskState;
  isFirst: boolean;
}) {
  return (
    <div
      className="grid grid-cols-[1.75rem_0.5rem_minmax(0,1fr)_auto] items-center gap-x-2.5 px-3 py-2 text-sm"
      style={
        isFirst
          ? undefined
          : {
              borderTopColor: "var(--border-subtle, #2e2e36)",
              borderTopStyle: "solid",
              borderTopWidth: "1px",
            }
      }
      data-testid="automation-run-task-ledger-row"
    >
      <span
        className="text-right font-mono text-xs font-semibold tabular-nums"
        style={{ color: "var(--text-muted)" }}
      >
        #{task.taskNumber}
      </span>
      <TaskStateDot state={state} />
      <span
        className="min-w-0 truncate"
        style={{ color: "var(--text-secondary)" }}
      >
        {task.title}
      </span>
      <span
        className="shrink-0 text-[0.6875rem] font-semibold uppercase tracking-wide"
        style={{ color: stateDotColor(state) }}
        data-testid="automation-run-task-ledger-row-state"
      >
        {STATE_LABELS[state]}
      </span>
    </div>
  );
}

function taskLedgerFingerprint(tasks: AgentTaskSummary[]): string {
  return tasks
    .map(
      (task) =>
        `${task.taskId}:${task.taskNumber}:${task.state}:${task.updatedAt}`,
    )
    .join("|");
}

/**
 * Live "what the agent is doing now" ledger for a single automation run's
 * conversation. Reuses the shared agent-task query surface and keeps terminal
 * runs on a static snapshot. Mounted lazily by the run card only after it
 * expands so the timeline first paint is not blocked by this fetch.
 */
export function AutomationRunTaskLedger({
  conversationId,
  projectId,
  runStatus,
  onOwnerAgentChange,
}: {
  conversationId: string;
  projectId: string | null;
  runStatus: AutomationRunStatus;
  onOwnerAgentChange?: (ownerAgent: string | null) => void;
}) {
  const lastFingerprintRef = useRef<string | null>(null);
  const unchangedResponsesRef = useRef(0);
  const query = useQuery({
    queryKey: agentWorkspaceKeys.agentTasksForScope("conversation", conversationId),
    queryFn: () =>
      agentTaskApi.listConversationTasks({
        conversationId,
        projectId,
        includeDone: true,
      }),
    staleTime: AGENT_WORKSPACE_STALE_MS,
    refetchInterval: () =>
      automationRunTaskLedgerRefetchInterval(
        runStatus,
        unchangedResponsesRef.current,
      ),
    refetchIntervalInBackground: false,
  });

  useEffect(() => {
    lastFingerprintRef.current = null;
    unchangedResponsesRef.current = 0;
  }, [conversationId, runStatus]);

  useEffect(() => {
    if (!query.data) {
      return;
    }
    const fingerprint = taskLedgerFingerprint(query.data);
    if (fingerprint === lastFingerprintRef.current) {
      unchangedResponsesRef.current += 1;
      return;
    }
    lastFingerprintRef.current = fingerprint;
    unchangedResponsesRef.current = 0;
  }, [query.data, query.dataUpdatedAt]);

  useEffect(() => {
    if (query.isSuccess) {
      onOwnerAgentChange?.(query.data[0]?.ownerAgent ?? null);
    }
  }, [onOwnerAgentChange, query.data, query.isSuccess]);

  const { taskCount, activeTasks, doneCount, droppedCount } = useMemo(() => {
    const tasks = query.data ?? [];
    const active = tasks.filter(
      (task) => task.state === "active" || task.state === "open",
    );
    // Live/actionable work first, then in-progress before merely open.
    active.sort((a, b) => {
      if (a.state === b.state) {
        return a.taskNumber - b.taskNumber;
      }
      return a.state === "active" ? -1 : 1;
    });
    return {
      taskCount: tasks.length,
      activeTasks: active,
      doneCount: tasks.filter((task) => task.state === "done").length,
      droppedCount: tasks.filter((task) => task.state === "dropped").length,
    };
  }, [query.data]);

  const hasSnapshot = query.isSuccess;

  return (
    <div className="space-y-2" data-testid="automation-run-task-ledger">
      <div
        className="flex items-baseline justify-between gap-3"
        data-testid="automation-run-task-ledger-label-row"
      >
        <FieldLabel>Task ledger</FieldLabel>
        {hasSnapshot ? (
          <span
            className="shrink-0 text-right text-xs"
            style={{ color: "var(--text-muted, #8e8e96)" }}
            data-testid="automation-run-task-ledger-summary"
          >
            {doneCount} done · {droppedCount} dropped
          </span>
        ) : null}
      </div>
      {!hasSnapshot ? (
        <p className="text-sm" style={{ color: "var(--text-muted)" }}>
          {query.isError ? "Could not load agent tasks." : "Loading agent tasks..."}
        </p>
      ) : taskCount === 0 ? (
        <p className="text-sm" style={{ color: "var(--text-muted)" }}>
          No agent tasks yet.
        </p>
      ) : (
        activeTasks.length > 0 ? (
          <div
            className="overflow-hidden rounded-md"
            style={{
              backgroundColor: "var(--bg-elevated, #232329)",
              borderColor: "var(--border-default, #393940)",
              borderStyle: "solid",
              borderWidth: "1px",
            }}
          >
            {activeTasks.map((task, index) => (
              <TaskRow
                key={task.taskId}
                task={task}
                state={effectiveTaskState(task.state, runStatus)}
                isFirst={index === 0}
              />
            ))}
          </div>
        ) : (
          <p className="text-sm" style={{ color: "var(--text-muted)" }}>
            No active tasks right now.
          </p>
        )
      )}
    </div>
  );
}
