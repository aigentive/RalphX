/**
 * useInfiniteTasksQuery hook - TanStack Query wrapper for infinite scroll pagination
 *
 * Implements cursor-based pagination using TanStack Query's useInfiniteQuery.
 * Supports filtering by status and archived state.
 */

import { useInfiniteQuery } from "@tanstack/react-query";
import { api } from "@/lib/tauri";
import { usePlanStore, selectActivePlanId } from "@/stores/planStore";
import type { Task, TaskListResponse } from "@/types/task";
import type { InternalStatus } from "@/types/status";

/**
 * Parameters for infinite tasks query
 */
export interface InfiniteTasksParams {
  /** The project ID to fetch tasks for */
  projectId: string;
  /** Optional status filter (single status or array of statuses) */
  statuses?: InternalStatus[] | undefined;
  /** Optional category filter — only return tasks matching these categories */
  categories?: string[] | undefined;
  /** Whether to include archived tasks (default false) */
  includeArchived?: boolean | undefined;
  /** Optional ideation session ID to filter tasks by plan */
  ideationSessionId?: string | null | undefined;
  /** Optional execution plan ID to filter tasks (mutually exclusive with ideationSessionId) */
  executionPlanId?: string | null | undefined;
}

/**
 * Query key factory for infinite tasks queries
 */
export const infiniteTaskKeys = {
  all: ["tasks", "infinite"] as const,
  list: (params: InfiniteTasksParams) =>
    [
      ...infiniteTaskKeys.all,
      params.projectId,
      params.statuses,
      params.categories,
      params.includeArchived,
      params.ideationSessionId,
      params.executionPlanId,
    ] as const,
};

/**
 * Hook for fetching tasks with infinite scroll pagination
 *
 * Uses TanStack Query's useInfiniteQuery for automatic pagination management.
 * Each page contains 20 tasks. The hook manages the page offset automatically
 * and provides helpers for loading more data.
 *
 * @param params - Query parameters
 * @param params.projectId - The project ID to fetch tasks for
 * @param params.status - Optional status filter
 * @param params.includeArchived - Whether to include archived tasks
 *
 * @returns TanStack Query infinite result with pagination helpers
 *
 * @example
 * ```tsx
 * const {
 *   data,
 *   fetchNextPage,
 *   hasNextPage,
 *   isFetchingNextPage,
 *   isLoading,
 * } = useInfiniteTasksQuery({
 *   projectId: "project-123",
 *   status: "backlog",
 *   includeArchived: false,
 * });
 *
 * const tasks = flattenPages(data);
 *
 * return (
 *   <div>
 *     {tasks.map(task => <TaskCard key={task.id} task={task} />)}
 *     {hasNextPage && (
 *       <button onClick={() => fetchNextPage()} disabled={isFetchingNextPage}>
 *         Load More
 *       </button>
 *     )}
 *   </div>
 * );
 * ```
 */
export function useInfiniteTasksQuery({
  projectId,
  statuses,
  categories,
  includeArchived = false,
  ideationSessionId,
  executionPlanId,
}: InfiniteTasksParams) {
  // Guard: if there's an active plan but executionPlanId hasn't resolved yet (loading gap),
  // disable the query to prevent fetching all tasks without a plan filter.
  const activePlanId = usePlanStore(selectActivePlanId(projectId));

  return useInfiniteQuery<TaskListResponse, Error>({
    queryKey: infiniteTaskKeys.list({ projectId, statuses, categories, includeArchived, ideationSessionId, executionPlanId }),
    queryFn: async ({ pageParam = 0 }) => {
      return api.tasks.list({
        projectId,
        ...(statuses !== undefined && statuses.length > 0 && { statuses }),
        ...(categories !== undefined && categories.length > 0 && { categories }),
        offset: pageParam as number,
        limit: 20,
        includeArchived,
        ...(ideationSessionId !== undefined && ideationSessionId !== null && { ideationSessionId }),
        ...(executionPlanId !== undefined && executionPlanId !== null && { executionPlanId }),
      });
    },
    getNextPageParam: (lastPage) => {
      // If there are more pages, return the next offset
      // Otherwise return undefined to signal no more pages
      return lastPage.hasMore ? lastPage.offset + 20 : undefined;
    },
    initialPageParam: 0,
    // Disable query during the plan loading gap: activePlanId is set (optimistic) but
    // executionPlanId hasn't been fetched yet → would fetch all tasks without a plan filter.
    // Allow when ideationSessionId is provided (Agents context filters by session, not execution plan).
    enabled: Boolean(projectId) && !(activePlanId && !executionPlanId && !ideationSessionId),
    // Local-first app with event-driven updates: longer cache is safe and performant
    staleTime: 10 * 60 * 1000, // 10 minutes
    gcTime: 30 * 60 * 1000, // 30 minutes cache retention
  });
}

/**
 * Helper function to flatten paginated data into a single array
 *
 * Extracts all tasks from all loaded pages into a flat array.
 * Returns empty array if data is undefined.
 *
 * @param data - The infinite query data object
 * @returns Flattened array of tasks from all pages
 *
 * @example
 * ```tsx
 * const { data } = useInfiniteTasksQuery({ projectId: "123" });
 * const allTasks = flattenPages(data);
 * ```
 */
export function flattenPages(
  data: { pages: TaskListResponse[] } | undefined
): Task[] {
  if (!data?.pages) return [];

  return data.pages.flatMap((page) => page.tasks);
}
