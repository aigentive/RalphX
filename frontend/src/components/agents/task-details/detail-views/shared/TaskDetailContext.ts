import { createContext, useContext, useMemo } from "react";
import { useQuery } from "@tanstack/react-query";
import { planBranchApi, type PlanBranch } from "@/api/plan-branch";
import { taskContextApi } from "@/api/task-context";
import type { Task, InternalStatus } from "@/types/task";
import type { TaskContext } from "@/types/task-context";

export type TaskDetailViewMode =
  | { kind: "current" }
  | {
      kind: "historical";
      status: InternalStatus;
      timestamp: string;
      conversationId?: string | undefined;
      agentRunId?: string | undefined;
    };

interface BranchSummary {
  label: string;
  source: string;
  target: string | null;
  status: string | null;
}

interface PullRequestSummary {
  number: number;
  url: string | null;
  status: PlanBranch["prStatus"];
}

interface MergeSummary {
  target: string | null;
  commitSha: string | null;
  mergedAt: string | null;
}

export interface TaskDetailContextModel {
  task: Task;
  viewMode: TaskDetailViewMode;
  taskContext: TaskContext | null;
  planBranch: PlanBranch | null;
  isLoading: boolean;
  planArtifactId: string | null;
  sessionId: string | null;
  branch: BranchSummary | null;
  pullRequest: PullRequestSummary | null;
  merge: MergeSummary | null;
}

export const TaskDetailContext = createContext<TaskDetailContextModel | null>(null);

export function useTaskDetailContextModel(): TaskDetailContextModel | null {
  return useContext(TaskDetailContext);
}

function getContextTaskValue(context: TaskContext | null | undefined, key: string): string | null {
  const contextTask = context?.task as Record<string, unknown> | undefined;
  const value = contextTask?.[key];
  return typeof value === "string" && value.trim().length > 0 ? value : null;
}

function resolvePlanBranch({
  task,
  branches,
  planArtifactId,
  sessionId,
}: {
  task: Task;
  branches: PlanBranch[];
  planArtifactId: string | null;
  sessionId: string | null;
}): PlanBranch | null {
  return (
    branches.find((branch) => branch.mergeTaskId === task.id) ??
    (sessionId ? branches.find((branch) => branch.sessionId === sessionId) : undefined) ??
    (planArtifactId ? branches.find((branch) => branch.planArtifactId === planArtifactId) : undefined) ??
    null
  );
}

function parseMetadataBranch(task: Task, key: "source_branch" | "target_branch"): string | null {
  if (!task.metadata) return null;
  try {
    const parsed = JSON.parse(task.metadata) as Record<string, unknown>;
    const value = parsed[key];
    return typeof value === "string" && value.trim().length > 0 ? value : null;
  } catch {
    return null;
  }
}

function buildBranchSummary(task: Task, planBranch: PlanBranch | null): BranchSummary | null {
  const source = parseMetadataBranch(task, "source_branch") ?? task.taskBranch ?? null;
  if (task.category !== "plan_merge" && source) {
    return {
      label: "Task branch",
      source,
      target: parseMetadataBranch(task, "target_branch"),
      status: null,
    };
  }

  if (planBranch) {
    const target = planBranch.baseBranchOverride?.trim() || planBranch.sourceBranch || null;
    return {
      label: "Plan branch",
      source: planBranch.branchName,
      target,
      status: planBranch.status,
    };
  }

  if (!source) return null;

  return {
    label: "Task branch",
    source,
    target: parseMetadataBranch(task, "target_branch"),
    status: null,
  };
}

function buildPullRequestSummary(planBranch: PlanBranch | null): PullRequestSummary | null {
  if (!planBranch || planBranch.prNumber == null) return null;
  const inferredStatus = planBranch.prStatus ?? (planBranch.status === "merged" ? "Merged" : null);
  return {
    number: planBranch.prNumber,
    url: planBranch.prUrl,
    status: inferredStatus,
  };
}

function buildMergeSummary(task: Task, planBranch: PlanBranch | null): MergeSummary | null {
  const commitSha = task.mergeCommitSha ?? planBranch?.mergeCommitSha ?? null;
  const target =
    planBranch?.baseBranchOverride?.trim() ||
    planBranch?.sourceBranch ||
    parseMetadataBranch(task, "target_branch") ||
    null;
  const mergedAt = planBranch?.mergedAt ?? task.completedAt ?? null;
  const isMerged =
    task.internalStatus === "merged" ||
    planBranch?.status === "merged" ||
    Boolean(commitSha) ||
    Boolean(mergedAt);

  if (!isMerged) return null;
  if (!commitSha && !target && !mergedAt) return null;
  return { target, commitSha, mergedAt };
}

export function useTaskDetailContextData(
  task: Task,
  viewMode: TaskDetailViewMode
): TaskDetailContextModel {
  const taskContextQuery = useQuery({
    queryKey: ["task-detail-context", "task-context", task.id] as const,
    queryFn: async () => taskContextApi.getTaskContext(task.id).catch(() => null),
    staleTime: 30_000,
  });

  const projectPlanBranchesQuery = useQuery({
    queryKey: ["task-detail-context", "project-plan-branches", task.projectId] as const,
    queryFn: async () => planBranchApi.getByProject(task.projectId).catch(() => []),
    staleTime: 10_000,
    refetchInterval: viewMode.kind === "current" ? 15_000 : false,
  });

  const taskContext = taskContextQuery.data ?? null;
  const branches = projectPlanBranchesQuery.data ?? [];
  const planArtifactId =
    task.planArtifactId ??
    taskContext?.planArtifact?.id ??
    getContextTaskValue(taskContext, "plan_artifact_id");
  const sessionId =
    task.ideationSessionId ?? getContextTaskValue(taskContext, "ideation_session_id");
  const planBranch = resolvePlanBranch({
    task,
    branches,
    planArtifactId: planArtifactId ?? null,
    sessionId: sessionId ?? null,
  });

  return useMemo(
    () => ({
      task,
      viewMode,
      taskContext,
      planBranch,
      isLoading: taskContextQuery.isLoading || projectPlanBranchesQuery.isLoading,
      planArtifactId: planArtifactId ?? null,
      sessionId: sessionId ?? null,
      branch: buildBranchSummary(task, planBranch),
      pullRequest: buildPullRequestSummary(planBranch),
      merge: buildMergeSummary(task, planBranch),
    }),
    [
      task,
      viewMode,
      taskContext,
      planBranch,
      taskContextQuery.isLoading,
      projectPlanBranchesQuery.isLoading,
      planArtifactId,
      sessionId,
    ]
  );
}
