/**
 * Execution control hooks - manage pause/resume/stop execution state
 *
 * Provides hooks for:
 * - useExecutionStatus: Query execution status (running/queued counts, pause state)
 * - usePauseExecution: Toggle pause/resume execution for resumable work
 * - useStopExecution: Stop all running tasks and require manual restart
 *
 * Phase 82: All hooks now accept optional projectId for per-project scoping
 */

import { useEffect } from "react";
import { useQuery, useMutation, useQueryClient } from "@tanstack/react-query";
import { api, type ExecutionStatusResponse } from "@/lib/tauri";
import { useUiStore } from "@/stores/uiStore";

/**
 * Query key factory for execution status
 * Phase 82: Keys now include projectId for per-project caching
 */
export const executionKeys = {
  all: ["execution"] as const,
  status: (projectId?: string) => [...executionKeys.all, "status", projectId ?? "all"] as const,
};

/**
 * Hook to fetch and track execution status
 * Phase 82: Now accepts optional projectId for per-project status
 *
 * @param projectId - Optional project ID to scope status to (uses active project if omitted)
 * @returns TanStack Query result with execution status data
 * Also returns convenience accessors: isPaused, runningCount, queuedCount, etc.
 *
 * @example
 * ```tsx
 * const { isPaused, runningCount, queuedCount, isLoading } = useExecutionStatus(projectId);
 *
 * if (isLoading) return <Loading />;
 * return <ExecutionControlBar isPaused={isPaused} running={runningCount} />;
 * ```
 */
interface UseExecutionStatusOptions {
  enabled?: boolean;
  refetchInterval?: number | false;
  refetchOnWindowFocus?: boolean;
  staleTime?: number;
}

export function useExecutionStatus(
  projectId?: string,
  options: UseExecutionStatusOptions = {}
) {
  const setExecutionStatus = useUiStore((state) => state.setExecutionStatus);
  const setExecutionStatusUnknown = useUiStore(
    (state) => state.setExecutionStatusUnknown,
  );
  const enabled = options.enabled ?? true;

  const query = useQuery<ExecutionStatusResponse, Error>({
    queryKey: executionKeys.status(projectId),
    queryFn: async () => {
      const status = await api.execution.getStatus(projectId);
      // Update uiStore with fresh status
      setExecutionStatus(status);
      return status;
    },
    enabled,
    // Fallback poll every 30s - real-time updates come via useExecutionEvents
    refetchInterval: enabled ? options.refetchInterval ?? 30000 : false,
    // Also refetch on window focus
    refetchOnWindowFocus: options.refetchOnWindowFocus ?? enabled,
    ...(options.staleTime !== undefined ? { staleTime: options.staleTime } : {}),
  });

  useEffect(() => {
    if (enabled && query.isError) {
      setExecutionStatusUnknown();
    }
  }, [enabled, query.isError, setExecutionStatusUnknown]);

  return {
    ...query,
    status: query.data ? query.status : "unknown" as const,
    isStatusKnown: query.data !== undefined,
    // Convenience accessors
    isPaused: query.data?.isPaused ?? false,
    haltMode: query.data?.haltMode ?? "running",
    runningCount: query.data?.runningCount ?? 0,
    queuedCount: query.data?.queuedCount ?? 0,
    queuedMessageCount: query.data?.queuedMessageCount ?? 0,
    maxConcurrent: query.data?.maxConcurrent ?? 10,
    globalMaxConcurrent: query.data?.globalMaxConcurrent ?? 20,
    canStartTask: query.data?.canStartTask ?? false,
  };
}

/**
 * Hook to pause/start/resume execution
 * Phase 82: Now accepts optional projectId for per-project pause/resume
 *
 * @param projectId - Optional project ID to scope pause/resume to
 * @returns Mutation for toggling pause state, plus convenience methods
 *
 * @example
 * ```tsx
 * const { toggle, isPending } = usePauseExecution(projectId);
 *
 * return (
 *   <button onClick={toggle} disabled={isPending}>
 *     {uiStore.executionStatus.isPaused ? 'Resume' : 'Pause'}
 *   </button>
 * );
 * ```
 */
export function usePauseExecution(projectId?: string) {
  const queryClient = useQueryClient();
  const executionStatus = useUiStore((state) => state.executionStatus);
  const setExecutionStatus = useUiStore((state) => state.setExecutionStatus);
  const haltMode = executionStatus.haltMode ?? (executionStatus.isPaused ? "paused" : "running");

  const pauseMutation = useMutation({
    mutationFn: async () => {
      const response = await api.execution.pause(projectId);
      return response;
    },
    onSuccess: (data) => {
      setExecutionStatus(data.status);
      queryClient.invalidateQueries({ queryKey: executionKeys.status(projectId) });
    },
  });

  const resumeMutation = useMutation({
    mutationFn: async () => {
      const response = await api.execution.resume(projectId);
      return response;
    },
    onSuccess: (data) => {
      setExecutionStatus(data.status);
      queryClient.invalidateQueries({ queryKey: executionKeys.status(projectId) });
    },
  });

  const toggle = () => {
    if (executionStatus.isPaused || haltMode === "stopped") {
      resumeMutation.mutate();
    } else {
      pauseMutation.mutate();
    }
  };

  return {
    toggle,
    pause: () => {
      if (haltMode === "running") {
        pauseMutation.mutate();
      }
    },
    resume: () => {
      if (haltMode !== "running") {
        resumeMutation.mutate();
      }
    },
    isPending: pauseMutation.isPending || resumeMutation.isPending,
    isError: pauseMutation.isError || resumeMutation.isError,
    error: pauseMutation.error || resumeMutation.error,
    canPauseToggle: !(pauseMutation.isPending || resumeMutation.isPending),
  };
}

/**
 * Hook to stop all running tasks
 * Phase 82: Now accepts optional projectId for per-project stop
 *
 * @param projectId - Optional project ID to scope stop to
 * @returns Mutation for stopping execution
 *
 * @example
 * ```tsx
 * const { stop, isPending, canStop } = useStopExecution(projectId);
 *
 * return (
 *   <button onClick={stop} disabled={!canStop || isPending}>
 *     Stop
 *   </button>
 * );
 * ```
 */
export function useStopExecution(projectId?: string) {
  const queryClient = useQueryClient();
  const executionStatus = useUiStore((state) => state.executionStatus);
  const setExecutionStatus = useUiStore((state) => state.setExecutionStatus);

  const mutation = useMutation({
    mutationFn: async () => {
      const response = await api.execution.stop(projectId);
      return response;
    },
    onSuccess: (data) => {
      setExecutionStatus(data.status);
      queryClient.invalidateQueries({ queryKey: executionKeys.status(projectId) });
    },
  });

  return {
    stop: () => mutation.mutate(),
    isPending: mutation.isPending,
    isError: mutation.isError,
    error: mutation.error,
    // Can only stop if there are running tasks
    canStop: executionStatus.runningCount > 0,
  };
}
