import { useEffect, useMemo, useRef } from "react";
import { useQuery } from "@tanstack/react-query";

import {
  agentTaskApi,
  type AgentTaskState,
  type AgentTaskSummary,
} from "@/api/agent-tasks";
import type { AutomationRunStatus } from "@/api/automations";
import { StatusPill, type StatusPillTone } from "@/components/ui/status-pill";
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

/** Tone for a task state badge — accent for live, success/error when settled. */
function stateTone(state: AgentTaskState): StatusPillTone {
  switch (state) {
    case "active":
      return "accent";
    case "done":
      return "success";
    case "dropped":
      return "error";
    default:
      return "neutral";
  }
}

function TaskStateBadge({ state }: { state: AgentTaskState }) {
  return (
    <StatusPill
      label={STATE_LABELS[state]}
      tone={stateTone(state)}
      live={state === "active"}
      className="shrink-0"
    />
  );
}

function TaskRow({ task }: { task: AgentTaskSummary }) {
  return (
    <div
      className="flex items-center gap-2 text-sm"
      data-testid="automation-run-task-ledger-row"
    >
      <span
        className="w-8 shrink-0 text-right font-mono text-xs font-semibold"
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
        <div className="space-y-1.5">
          {activeTasks.length > 0 ? (
            activeTasks.map((task) => <TaskRow key={task.taskId} task={task} />)
          ) : (
            <p className="text-sm" style={{ color: "var(--text-muted)" }}>
              No active tasks right now.
            </p>
          )}
        </div>
      )}
    </div>
  );
}
