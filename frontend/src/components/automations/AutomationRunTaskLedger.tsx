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
import { automationRunTaskLedgerRefetchInterval } from "./automationRunTaskLedgerPolling";

const STATE_LABELS: Record<AgentTaskState, string> = {
  open: "Open",
  active: "In progress",
  done: "Done",
  dropped: "Dropped",
};

/** Status token for a task state badge — accent for live, muted/error otherwise. */
function stateColor(state: AgentTaskState): string {
  switch (state) {
    case "active":
      return "var(--accent-primary)";
    case "done":
      return "var(--status-success)";
    case "dropped":
      return "var(--status-error)";
    default:
      return "var(--text-secondary)";
  }
}

function TaskStateBadge({ state }: { state: AgentTaskState }) {
  return (
    <span
      className="inline-flex w-fit shrink-0 items-center rounded-full px-2 py-0.5 text-[10px] font-semibold uppercase tracking-normal"
      style={{
        color: stateColor(state),
        backgroundColor: "var(--bg-hover)",
        borderColor: "var(--border-default)",
        borderStyle: "solid",
        borderWidth: "1px",
      }}
    >
      {STATE_LABELS[state]}
    </span>
  );
}

function TaskRow({ task }: { task: AgentTaskSummary }) {
  return (
    <div
      className="flex items-center gap-2 text-sm"
      data-testid="automation-run-task-ledger-row"
    >
      <span
        className="w-10 shrink-0 text-right font-mono text-xs font-semibold"
        style={{ color: "var(--text-muted)" }}
      >
        #{task.taskNumber}
      </span>
      <TaskStateBadge state={task.state} />
      <span
        className="min-w-0 flex-1 truncate"
        style={{ color: "var(--text-secondary)" }}
      >
        {task.title}
      </span>
      {task.ownerAgent && (
        <span
          className="max-w-[7rem] shrink-0 truncate text-xs"
          style={{ color: "var(--text-muted)" }}
        >
          {task.ownerAgent}
        </span>
      )}
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
}: {
  conversationId: string;
  projectId: string | null;
  runStatus: AutomationRunStatus;
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
  const summaryParts: string[] = [];
  if (doneCount > 0) {
    summaryParts.push(`${doneCount} done`);
  }
  if (droppedCount > 0) {
    summaryParts.push(`${droppedCount} dropped`);
  }

  return (
    <div className="space-y-2" data-testid="automation-run-task-ledger">
      <div
        className="text-xs font-medium uppercase tracking-normal"
        style={{ color: "var(--text-muted)" }}
      >
        Task ledger
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
        <div className="space-y-1.5">
          {activeTasks.length > 0 ? (
            activeTasks.map((task) => <TaskRow key={task.taskId} task={task} />)
          ) : (
            <p className="text-sm" style={{ color: "var(--text-muted)" }}>
              No active tasks right now.
            </p>
          )}
          {summaryParts.length > 0 && (
            <p
              className="text-xs"
              style={{ color: "var(--text-muted)" }}
              data-testid="automation-run-task-ledger-summary"
            >
              {summaryParts.join(" · ")}
            </p>
          )}
        </div>
      )}
    </div>
  );
}
