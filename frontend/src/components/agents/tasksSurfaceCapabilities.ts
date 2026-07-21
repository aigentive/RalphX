import type { TasksFeatureState } from "@/types/ideation-config";

export interface TasksSurfaceCapabilities {
  hasHistory: boolean;
  isReadOnly: boolean;
  canProgress: boolean;
  canQuiesce: boolean;
  reason: "tasks_disabled" | "tasks_draining" | "history_unavailable" | null;
}

export function deriveTasksSurfaceCapabilities({
  featureState,
  hasHistory,
  historyUnavailable = false,
}: {
  featureState: TasksFeatureState;
  hasHistory: boolean;
  historyUnavailable?: boolean;
}): TasksSurfaceCapabilities {
  const featureEnabled = featureState === "enabled";
  return {
    hasHistory: historyUnavailable || hasHistory,
    isReadOnly: !featureEnabled || historyUnavailable,
    canProgress: featureEnabled && !historyUnavailable,
    canQuiesce: true,
    reason: historyUnavailable
      ? "history_unavailable"
      : featureState === "draining"
        ? "tasks_draining"
        : featureState === "disabled"
          ? "tasks_disabled"
          : null,
  };
}
