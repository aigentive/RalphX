/**
 * useTaskSteps hook - TanStack Query wrappers for task step fetching
 *
 * Fetches task steps and progress summaries using the Tauri API with
 * automatic caching, refetching, and error handling.
 */

import { useQuery } from "@tanstack/react-query";
import { api } from "@/lib/tauri";
import type { TaskStep, StepProgressSummary } from "@/types/task-step";

/**
 * Query key factory for task steps
 * @param taskId - The task ID to fetch steps for
 * @returns Query key array for TanStack Query
 */
export const stepKeys = {
  all: ["steps"] as const,
  byTask: (taskId: string) => [...stepKeys.all, "task", taskId] as const,
  progress: (taskId: string) => [...stepKeys.all, "progress", taskId] as const,
};

/**
 * Hook to fetch steps for a task
 *
 * @param taskId - The task ID to fetch steps for
 * @returns TanStack Query result with steps data
 *
 * @example
 * ```tsx
 * const { data: steps, isLoading, isError } = useTaskSteps("task-123");
 *
 * if (isLoading) return <Loading />;
 * if (isError) return <Error />;
 * return <StepList steps={steps} />;
 * ```
 */
export function useTaskSteps(taskId: string) {
  return useQuery<TaskStep[], Error>({
    queryKey: stepKeys.byTask(taskId),
    queryFn: () => api.steps.getByTask(taskId),
    staleTime: 30_000, // 30 seconds
    enabled: Boolean(taskId),
  });
}

/**
 * Hook to fetch step progress summary for a task
 *
 * Automatically refetches every 5 seconds when there are steps in progress.
 *
 * @param taskId - The task ID to fetch progress for
 * @returns TanStack Query result with progress summary
 *
 * @example
 * ```tsx
 * const { data: progress, isLoading } = useStepProgress("task-123");
 *
 * if (isLoading) return <Loading />;
 * if (!progress) return null;
 * const display = getStepProgressDisplay(progress);
 * return (
 *   <div>
 *     Progress: {display.completed}/{display.total} steps
 *     ({display.completedPercent.toFixed(0)}%)
 *   </div>
 * );
 * ```
 */
export function useStepProgress(
  taskId: string,
  options?: { isExecuting?: boolean },
) {
  return useQuery<StepProgressSummary, Error>({
    queryKey: stepKeys.progress(taskId),
    queryFn: () => api.steps.getProgress(taskId),
    staleTime: 5_000, // 5 seconds
    enabled: Boolean(taskId),
    refetchInterval: (query) => {
      // Always poll during active execution (even when all steps are pending)
      if (options?.isExecuting) return 5_000;
      // Otherwise only poll when there are steps in progress
      const progress = query.state.data;
      return progress && progress.inProgress > 0 ? 5_000 : false;
    },
  });
}
