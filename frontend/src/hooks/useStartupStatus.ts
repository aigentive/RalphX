import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";

import { startupApi, type StartupStage, type StartupStatus } from "@/api/startup";

const STARTUP_STATUS_POLL_INTERVAL_MS = 500;

const STARTUP_STAGE_ORDER: Record<StartupStage, number> = {
  creating_window: 0,
  opening_database: 1,
  migrating: 2,
  loading_settings: 3,
  startup_cleanup: 4,
  registering_state: 5,
  app_state_ready: 6,
  binding_local_runtime: 7,
  safety_recovery: 8,
  runtime_ready: 9,
  background_recovery: 10,
  ready: 11,
  degraded: 12,
  failed: 13,
};

export const startupStatusKeys = {
  all: ["startup-status"] as const,
  snapshot: () => [...startupStatusKeys.all, "snapshot"] as const,
};

function isTerminalStartupStatus(status: StartupStatus | undefined): boolean {
  return (
    status?.stage === "ready" ||
    status?.stage === "degraded" ||
    status?.stage === "failed"
  );
}

export function reconcileStartupStatus(
  previous: StartupStatus | undefined,
  incoming: StartupStatus,
): StartupStatus {
  if (!previous || previous.bootId !== incoming.bootId) {
    return incoming;
  }

  if (incoming.attemptId > previous.attemptId) {
    return incoming;
  }

  if (incoming.attemptId < previous.attemptId) {
    return previous;
  }

  if (isTerminalStartupStatus(previous)) {
    return previous;
  }

  if (STARTUP_STAGE_ORDER[incoming.stage] < STARTUP_STAGE_ORDER[previous.stage]) {
    return previous;
  }

  return {
    ...incoming,
    appStateReady: previous.appStateReady || incoming.appStateReady,
    runtimeReady: previous.runtimeReady || incoming.runtimeReady,
    backgroundComplete: previous.backgroundComplete || incoming.backgroundComplete,
    completedAt: incoming.completedAt ?? previous.completedAt,
  };
}

export function useStartupStatus() {
  const queryClient = useQueryClient();
  const statusQuery = useQuery({
    queryKey: startupStatusKeys.snapshot(),
    queryFn: async () => {
      const incoming = await startupApi.getStatus();
      return reconcileStartupStatus(
        queryClient.getQueryData<StartupStatus>(startupStatusKeys.snapshot()),
        incoming,
      );
    },
    retry: false,
    refetchInterval: (query) =>
      isTerminalStartupStatus(query.state.data)
        ? false
        : STARTUP_STATUS_POLL_INTERVAL_MS,
  });
  const retryMutation = useMutation({
    mutationFn: () => startupApi.retry(),
    onSuccess: (incoming) => {
      queryClient.setQueryData<StartupStatus>(
        startupStatusKeys.snapshot(),
        (previous) => reconcileStartupStatus(previous, incoming),
      );
    },
  });

  const status = statusQuery.data;

  return {
    status,
    canMountApp: status?.runtimeReady === true,
    isLoading: statusQuery.isLoading,
    isTerminalFailure: status?.stage === "failed",
    isBackgroundSettled:
      status?.backgroundComplete === true ||
      status?.stage === "degraded" ||
      status?.stage === "failed",
    statusError: statusQuery.error,
    isStatusError: statusQuery.isError,
    refetch: statusQuery.refetch,
    retry: retryMutation.mutateAsync,
    isRetrying: retryMutation.isPending,
    retryError: retryMutation.error,
  };
}
