import type { InfiniteData } from "@tanstack/react-query";
import type { AgentSidebarConversationGroup } from "@/api/chat";
import type { ExecutionTaskAgentWorkspace } from "@/api/execution-task-agent-workspace";
import type { MergePipelineTask } from "@/api/merge-pipeline";
import type { RunningProcess } from "@/api/running-processes";
import { getQueryClient } from "@/lib/queryClient";
import type { Task } from "@/types/task";

const AGENT_SIDEBAR_CONVERSATION_QUERY_KEY = [
  "agents",
  "sidebar-conversations",
] as const;

export type ExecutionBarTaskNavigationSource =
  | "running"
  | "queued"
  | "paused"
  | "merge";

export interface ExecutionBarTaskNavigationTarget {
  taskId: string;
  source: ExecutionBarTaskNavigationSource;
  projectId?: string | null;
  ideationSessionId?: string | null;
  executionPlanId?: string | null;
  agentWorkspace?: ExecutionTaskAgentWorkspace | null;
}

export function runningProcessTaskTarget(
  process: RunningProcess,
): ExecutionBarTaskNavigationTarget {
  return {
    taskId: process.taskId,
    source: "running",
    projectId: process.agentWorkspace?.projectId ?? null,
    agentWorkspace: process.agentWorkspace ?? null,
  };
}

export function mergePipelineTaskTarget(
  task: MergePipelineTask,
): ExecutionBarTaskNavigationTarget {
  return {
    taskId: task.taskId,
    source: "merge",
    projectId: task.agentWorkspace?.projectId ?? null,
    agentWorkspace: task.agentWorkspace ?? null,
  };
}

export function taskRowNavigationTarget(
  task: Task,
  source: "queued" | "paused",
): ExecutionBarTaskNavigationTarget {
  return {
    taskId: task.id,
    source,
    projectId: task.projectId,
    ideationSessionId: task.ideationSessionId ?? null,
    executionPlanId: task.executionPlanId ?? null,
  };
}

export function resolveExecutionTaskAgentWorkspace(
  target: ExecutionBarTaskNavigationTarget,
): ExecutionTaskAgentWorkspace | null {
  if (target.agentWorkspace) {
    return target.agentWorkspace;
  }

  const executionPlanId = target.executionPlanId?.trim();
  const ideationSessionId = target.ideationSessionId?.trim();
  if (!executionPlanId && !ideationSessionId) {
    return null;
  }

  const queryClient = getQueryClient();
  const sidebarQueries = queryClient.getQueriesData<
    InfiniteData<AgentSidebarConversationGroup>
  >({ queryKey: AGENT_SIDEBAR_CONVERSATION_QUERY_KEY });
  const rows: Array<AgentSidebarConversationGroup["rows"][number]> = [];

  for (const [, data] of sidebarQueries) {
    for (const page of data?.pages ?? []) {
      for (const row of page.rows) {
        const workspace = row.workspace;
        if (!workspace || row.conversation.archivedAt) {
          continue;
        }
        if (target.projectId && workspace.projectId !== target.projectId) {
          continue;
        }
        rows.push(row);
      }
    }
  }

  if (executionPlanId) {
    const row = rows.find(
      (item) => item.workspace?.linkedPlanBranchId === executionPlanId,
    );
    if (row?.workspace) {
      return {
        conversationId: row.workspace.conversationId,
        projectId: row.workspace.projectId,
        title: row.conversation.title?.trim() || "Agent conversation",
      };
    }
  }

  if (ideationSessionId) {
    const row = rows.find(
      (item) => item.workspace?.linkedIdeationSessionId === ideationSessionId,
    );
    if (row?.workspace) {
      return {
        conversationId: row.workspace.conversationId,
        projectId: row.workspace.projectId,
        title: row.conversation.title?.trim() || "Agent conversation",
      };
    }
  }

  return null;
}
