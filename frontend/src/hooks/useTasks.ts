/**
 * useTasks hook - TanStack Query wrapper for task fetching
 *
 * Fetches tasks for a project using the Tauri API with
 * automatic caching, refetching, and error handling.
 */

import { useQuery } from "@tanstack/react-query";
import { api } from "@/lib/tauri";
import type { Task } from "@/types/task";

const DEFAULT_ALL_TASKS_PAGE_SIZE = 100;
const MAX_TASK_PAGES = 1_000;

type TaskListParams = Parameters<typeof api.tasks.list>[0];

export interface UseTasksOptions {
  enabled?: boolean;
  executionPlanId?: string | null;
  ideationSessionId?: string | null;
  includeArchived?: boolean;
  allPages?: boolean;
  pageSize?: number;
}

/**
 * Query key factory for tasks
 * @param projectId - The project ID to fetch tasks for
 * @returns Query key array for TanStack Query
 */
export const taskKeys = {
  all: ["tasks"] as const,
  lists: () => [...taskKeys.all, "list"] as const,
  list: (projectId: string) => [...taskKeys.lists(), projectId] as const,
  scopedList: (
    projectId: string,
    scope: {
      executionPlanId: string | null;
      ideationSessionId: string | null;
      includeArchived: boolean;
      allPages: boolean;
      pageSize: number | null;
    },
  ) => [...taskKeys.list(projectId), scope] as const,
  details: () => [...taskKeys.all, "detail"] as const,
  detail: (taskId: string) => [...taskKeys.details(), taskId] as const,
};

function hasScopedOptions(options: UseTasksOptions): boolean {
  return Boolean(
    options.executionPlanId ||
      options.ideationSessionId ||
      options.includeArchived ||
      options.allPages,
  );
}

function getTasksQueryKey(projectId: string, options: UseTasksOptions) {
  if (!hasScopedOptions(options)) {
    return taskKeys.list(projectId);
  }

  return taskKeys.scopedList(projectId, {
    executionPlanId: options.executionPlanId ?? null,
    ideationSessionId: options.ideationSessionId ?? null,
    includeArchived: options.includeArchived ?? false,
    allPages: options.allPages ?? false,
    pageSize: options.pageSize ?? null,
  });
}

function buildTaskListParams(
  projectId: string,
  options: UseTasksOptions,
  pagination?: { offset: number; limit: number },
): TaskListParams {
  return {
    projectId,
    ...(options.executionPlanId && { executionPlanId: options.executionPlanId }),
    ...(options.ideationSessionId && { ideationSessionId: options.ideationSessionId }),
    ...(options.includeArchived !== undefined && {
      includeArchived: options.includeArchived,
    }),
    ...(pagination && pagination),
  };
}

async function fetchAllTaskPages(
  baseParams: TaskListParams,
  pageSize: number,
): Promise<Task[]> {
  const tasks: Task[] = [];
  let offset = 0;

  for (let pageCount = 0; pageCount < MAX_TASK_PAGES; pageCount += 1) {
    const page = await api.tasks.list({
      ...baseParams,
      offset,
      limit: pageSize,
    });
    tasks.push(...page.tasks);

    if (!page.hasMore) {
      return tasks;
    }

    const nextOffset = page.offset + page.tasks.length;
    if (nextOffset <= offset) {
      throw new Error("Task pagination did not advance");
    }
    offset = nextOffset;
  }

  throw new Error("Task pagination exceeded the maximum page limit");
}

/**
 * Hook to fetch tasks for a project
 *
 * @param projectId - The project ID to fetch tasks for
 * @returns TanStack Query result with tasks data
 *
 * @example
 * ```tsx
 * const { data: tasks, isLoading, error } = useTasks("project-123");
 *
 * if (isLoading) return <Loading />;
 * if (error) return <Error message={error.message} />;
 * return <TaskList tasks={tasks} />;
 * ```
 */
export function useTasks(
  projectId: string,
  options: UseTasksOptions = {}
) {
  return useQuery<Task[], Error>({
    queryKey: getTasksQueryKey(projectId, options),
    queryFn: async () => {
      const params = buildTaskListParams(projectId, options);
      if (options.allPages) {
        const pageSize = Math.max(
          1,
          Math.floor(options.pageSize ?? DEFAULT_ALL_TASKS_PAGE_SIZE),
        );
        return fetchAllTaskPages(params, pageSize);
      }

      const response = await api.tasks.list(params);
      return response.tasks;
    },
    enabled: Boolean(projectId) && (options.enabled ?? true),
  });
}
