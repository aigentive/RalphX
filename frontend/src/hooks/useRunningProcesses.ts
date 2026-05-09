/**
 * useRunningProcesses hook - Fetch and track currently running processes
 *
 * Provides real-time list of tasks in agent-active states with enriched data
 * (step progress, elapsed time, trigger origin, branch name).
 *
 * Automatically refetches on task status changes and step updates.
 */

import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect } from "react";
import { runningProcessesApi, type RunningProcessesResponse } from "@/api/running-processes";
import { useEventBus } from "@/providers/EventProvider";
import type { Unsubscribe } from "@/lib/event-bus";

/**
 * Query key factory for running processes
 */
export const runningProcessesKeys = {
  all: ["running-processes"] as const,
  list: (projectId?: string) =>
    projectId
      ? ([...runningProcessesKeys.all, "list", projectId] as const)
      : ([...runningProcessesKeys.all, "list"] as const),
};

/**
 * Hook to fetch and track running processes with real-time updates
 *
 * @returns TanStack Query result with running processes data
 *
 * @example
 * ```tsx
 * const { data, isLoading } = useRunningProcesses();
 *
 * if (isLoading) return <Loading />;
 * return (
 *   <RunningProcessPopover
 *     processes={data?.processes ?? []}
 *     maxConcurrent={3}
 *   />
 * );
 * ```
 */
interface UseRunningProcessesOptions {
  enabled?: boolean;
  refetchInterval?: number | false;
  refetchOnWindowFocus?: boolean;
  subscribeToEvents?: boolean;
}

export function useRunningProcesses(
  projectId?: string,
  options: UseRunningProcessesOptions = {}
) {
  const queryClient = useQueryClient();
  const bus = useEventBus();
  const enabled = options.enabled ?? true;
  const subscribeToEvents = options.subscribeToEvents ?? enabled;

  const query = useQuery<RunningProcessesResponse, Error>({
    queryKey: runningProcessesKeys.list(projectId),
    queryFn: async () => {
      return await runningProcessesApi.getRunningProcesses(projectId);
    },
    enabled,
    // Fallback poll every 10s (real-time updates come via events)
    refetchInterval: enabled ? options.refetchInterval ?? 10000 : false,
    // Also refetch on window focus
    refetchOnWindowFocus: options.refetchOnWindowFocus ?? enabled,
  });

  // Listen for task status changes and step updates to trigger refetch
  useEffect(() => {
    if (!subscribeToEvents) {
      return;
    }

    const unsubscribes: Unsubscribe[] = [];

    // Refetch when any task status changes (task might enter or leave agent-active state)
    unsubscribes.push(
      bus.subscribe("task:status_changed", () => {
        queryClient.invalidateQueries({ queryKey: runningProcessesKeys.list(projectId) });
      })
    );

    // Refetch when execution status changes (tasks starting/stopping)
    unsubscribes.push(
      bus.subscribe("execution:status_changed", () => {
        queryClient.invalidateQueries({ queryKey: runningProcessesKeys.list(projectId) });
      })
    );

    // Refetch when step status changes (step progress updates)
    unsubscribes.push(
      bus.subscribe("step:status_changed", () => {
        queryClient.invalidateQueries({ queryKey: runningProcessesKeys.list(projectId) });
      })
    );

    return () => {
      unsubscribes.forEach((unsub) => unsub());
    };
  }, [bus, projectId, queryClient, subscribeToEvents]);

  return query;
}
