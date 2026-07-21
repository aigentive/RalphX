/**
 * useTasks hook - TanStack Query wrapper for task fetching
 *
 * Fetches tasks for a project using the Tauri API with
 * automatic caching, refetching, and error handling.
 */

import { useQuery } from "@tanstack/react-query";
import { api } from "@/lib/tauri";
import type { Task } from "@/types/task";

/**
 * Query key factory for tasks
 * @param projectId - The project ID to fetch tasks for
 * @returns Query key array for TanStack Query
 */
export const taskKeys = {
  all: ["tasks"] as const,
  lists: () => [...taskKeys.all, "list"] as const,
  list: (projectId: string) => [...taskKeys.lists(), projectId] as const,
  details: () => [...taskKeys.all, "detail"] as const,
  detail: (taskId: string) => [...taskKeys.details(), taskId] as const,
  sessionHistory: (projectId: string, ideationSessionId: string) =>
    [...taskKeys.all, "session-history", projectId, ideationSessionId] as const,
};

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
  options: { enabled?: boolean } = {}
) {
  return useQuery<Task[], Error>({
    queryKey: taskKeys.list(projectId),
    queryFn: async () => {
      const response = await api.tasks.list({ projectId });
      return response.tasks;
    },
    enabled: Boolean(projectId) && (options.enabled ?? true),
  });
}

export function useSessionTaskHistoryAvailability(
  projectId: string,
  ideationSessionId: string | null | undefined
) {
  return useQuery({
    queryKey: taskKeys.sessionHistory(projectId, ideationSessionId ?? ""),
    queryFn: () => api.tasks.getSessionHistoryAvailability(projectId, ideationSessionId ?? ""),
    enabled: Boolean(projectId && ideationSessionId),
  });
}
