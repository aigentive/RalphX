import { useQuery } from "@tanstack/react-query";

import { agentTaskApi, type AgentTaskSummary } from "@/api/agent-tasks";
import { agentWorkspaceKeys } from "@/components/agents/agentWorkspaceQueries";

const COLUMNS = ["open", "active", "done"] as const;

function columnLabel(state: (typeof COLUMNS)[number]) {
  return state === "open" ? "Open" : state === "active" ? "Active" : "Done";
}

export function TeamTaskBoard({
  conversationId,
  projectId,
}: {
  conversationId: string;
  projectId: string | null;
}) {
  const tasks = useQuery({
    queryKey: agentWorkspaceKeys.agentTasksForScope("conversation", conversationId),
    queryFn: () =>
      agentTaskApi.listConversationTasks({ conversationId, projectId, includeDone: true }),
    staleTime: 5_000,
  });

  if (tasks.isLoading) {
    return <TeamTaskBoardPlaceholder label="Loading Team board…" />;
  }
  if (tasks.isError) {
    return <TeamTaskBoardPlaceholder label="Could not load Team board tasks." error />;
  }
  const taskList = tasks.data ?? [];

  return (
    <div className="grid grid-cols-3 gap-2" data-testid="team-task-board">
      {COLUMNS.map((state) => {
        const items = taskList.filter((task) => task.state === state);
        return (
          <section
            key={state}
            className="min-w-0 rounded-lg border p-2"
            style={{
              backgroundColor: "var(--bg-base)",
              borderColor: "var(--border-subtle)",
              borderStyle: "solid",
              borderWidth: 1,
            }}
          >
            <h3 className="text-xs font-semibold" style={{ color: "var(--text-secondary)" }}>
              {columnLabel(state)}
            </h3>
            <div className="mt-2 space-y-1.5">
              {items.length === 0 ? (
                <p className="text-xs" style={{ color: "var(--text-muted)" }}>—</p>
              ) : (
                items.map((task) => <TaskCard key={task.taskId} task={task} />)
              )}
            </div>
          </section>
        );
      })}
    </div>
  );
}

function TeamTaskBoardPlaceholder({
  label,
  error = false,
}: {
  label: string;
  error?: boolean;
}) {
  return (
    <div
      className="rounded-lg border px-3 py-2 text-xs"
      data-testid={error ? "team-task-board-error" : "team-task-board-loading"}
      style={{
        backgroundColor: "var(--bg-base)",
        borderColor: error ? "var(--status-error-border, #6e3737)" : "var(--border-subtle)",
        borderStyle: "solid",
        borderWidth: 1,
        color: error ? "var(--status-error-text, #f0a0a0)" : "var(--text-muted)",
      }}
    >
      {label}
    </div>
  );
}

function TaskCard({ task }: { task: AgentTaskSummary }) {
  return (
    <div className="rounded-md px-2 py-1.5 text-xs" style={{ backgroundColor: "var(--bg-surface)" }}>
      <p className="line-clamp-2" style={{ color: "var(--text-primary)" }}>
        #{task.taskNumber} {task.title}
      </p>
      {task.ownerAgent ? (
        <p className="mt-0.5 truncate" style={{ color: "var(--text-muted)" }}>{task.ownerAgent}</p>
      ) : null}
    </div>
  );
}
