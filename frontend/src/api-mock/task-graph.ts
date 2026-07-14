/**
 * Mock Task Graph API
 *
 * Mirrors src/api/task-graph.ts with mock implementations.
 */

import { z } from "zod";
import { getStore } from "./store";
import {
  TaskDependencyGraphResponseSchema,
  TimelineEventsResponseSchema,
} from "@/api/task-graph";
import type { InternalStatus } from "@/types/status";

type TaskDependencyGraphResponseRaw = z.infer<typeof TaskDependencyGraphResponseSchema>;
type TimelineEventsResponseRaw = z.infer<typeof TimelineEventsResponseSchema>;

function createEmptyStatusSummary() {
  return {
    backlog: 0,
    ready: 0,
    blocked: 0,
    executing: 0,
    qa: 0,
    review: 0,
    merge: 0,
    completed: 0,
    terminal: 0,
  };
}

function bucketStatus(status: InternalStatus): keyof ReturnType<typeof createEmptyStatusSummary> {
  switch (status) {
    case "backlog":
      return "backlog";
    case "ready":
      return "ready";
    case "blocked":
    case "paused":
      return "blocked";
    case "executing":
    case "re_executing":
      return "executing";
    case "qa_refining":
    case "qa_testing":
    case "qa_passed":
    case "qa_failed":
      return "qa";
    case "pending_review":
    case "reviewing":
    case "review_passed":
    case "escalated":
    case "revision_needed":
      return "review";
    case "pending_merge":
    case "merging":
    case "waiting_on_pr":
    case "merge_incomplete":
    case "merge_conflict":
    case "updating_plan_branch":
    case "updating_task_branch":
    case "branch_update_blocked":
      return "merge";
    case "approved":
    case "merged":
      return "completed";
    case "failed":
    case "cancelled":
    case "stopped":
      return "terminal";
  }
}

function formatPlanTitle(planArtifactId: string, index: number): string {
  const match = planArtifactId.match(/(\d+)$/);
  const suffix = match?.[1] ?? String(index + 1);
  return `Plan ${suffix}`;
}

function buildGraphResponse(
  projectId: string,
  ideationSessionId?: string | null,
  executionPlanId?: string | null
): TaskDependencyGraphResponseRaw {
  const store = getStore();
  let tasks = Array.from(store.tasks.values()).filter(
    (task) => task.projectId === projectId
  );

  // Filter by ideationSessionId (matches planArtifactId) if provided
  if (ideationSessionId !== undefined) {
    tasks = tasks.filter((task) => task.planArtifactId === ideationSessionId);
  } else if (executionPlanId !== undefined && executionPlanId !== null) {
    // In web-mode mocks, execution-plan-scoped tasks reuse planArtifactId as the stable filter key.
    tasks = tasks.filter((task) => task.planArtifactId === executionPlanId);
  }
  const planGroupMap = new Map<
    string,
    {
      sessionId: string;
      sessionTitle: string | null;
      taskIds: string[];
      statusSummary: ReturnType<typeof createEmptyStatusSummary>;
    }
  >();

  tasks.forEach((task) => {
    if (!task.planArtifactId) return;
    const existing = planGroupMap.get(task.planArtifactId);
    const bucket = bucketStatus(task.internalStatus);
    if (existing) {
      existing.taskIds.push(task.id);
      existing.statusSummary[bucket] += 1;
    } else {
      const statusSummary = createEmptyStatusSummary();
      statusSummary[bucket] = 1;
      planGroupMap.set(task.planArtifactId, {
        sessionId: task.planArtifactId,
        sessionTitle: null,
        taskIds: [task.id],
        statusSummary,
      });
    }
  });

  const planGroups = Array.from(planGroupMap.entries()).map(
    ([planArtifactId, group], index) => ({
      plan_artifact_id: planArtifactId,
      session_id: group.sessionId,
      session_title: group.sessionTitle ?? formatPlanTitle(planArtifactId, index),
      task_ids: group.taskIds,
      status_summary: group.statusSummary,
      execution_plan_id: null,
    })
  );

  return {
    nodes: tasks.map((task) => ({
      task_id: task.id,
      title: task.title,
      description: task.description ?? null,
      category: task.category,
      internal_status: task.internalStatus,
      priority: task.priority,
      in_degree: 0,
      out_degree: 0,
      tier: 0,
      plan_artifact_id: task.planArtifactId ?? null,
      source_proposal_id: task.sourceProposalId ?? null,
      execution_plan_id: null,
    })),
    edges: [],
    plan_groups: planGroups,
    critical_path: [],
    has_cycles: false,
  };
}

export const mockTaskGraphApi = {
  getDependencyGraph: async (
    projectId: string,
    _includeArchived: boolean = false,
    executionPlanId?: string | null,
    sessionId?: string | null
  ): Promise<TaskDependencyGraphResponseRaw> =>
    buildGraphResponse(projectId, sessionId ?? undefined, executionPlanId),

  getTimelineEvents: async (
    _projectId: string,
    _limit: number = 50,
    _offset: number = 0
  ): Promise<TimelineEventsResponseRaw> => ({
    events: [],
    total: 0,
    has_more: false,
  }),
} as const;
