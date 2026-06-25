/**
 * useExecutionEvents hook - Real-time execution status event listener
 *
 * Listens to execution events from the backend and updates the UI store
 * immediately to provide instant feedback for execution state changes.
 *
 * Uses EventBus abstraction for browser/Tauri compatibility.
 *
 * Events:
 * - execution:status_changed: Running count, pause state, etc.
 * - execution:queue_changed: Queued task count
 *
 * Phase 82: Events now include projectId for per-project status updates.
 * Only updates UI store if the event is for the currently active project.
 */

import { useEffect } from "react";
import { useEventBus } from "@/providers/EventProvider";
import { useUiStore } from "@/stores/uiStore";
import { useTaskStore } from "@/stores/taskStore";
import { useProjectStore } from "@/stores/projectStore";
import type { Unsubscribe } from "@/lib/event-bus";
import type { InternalStatus } from "@/types/status";

/**
 * Event payload for execution:status_changed
 * Phase 82: Added projectId and globalMaxConcurrent
 */
interface ExecutionStatusEvent {
  isPaused: boolean;
  haltMode?: "running" | "paused" | "stopped";
  runningCount: number;
  maxConcurrent: number;
  globalMaxConcurrent?: number;
  queuedMessageCount?: number;
  projectId?: string;
  reason: string;
  timestamp: string;
}

/**
 * Event payload for execution:queue_changed
 * Phase 82: Added projectId
 */
interface ExecutionQueueEvent {
  queuedCount: number;
  queuedMessageCount?: number;
  projectId?: string;
  timestamp: string;
}

/**
 * Event payload for task:provider_error_paused
 * Emitted when a task is paused due to a provider error
 */
interface ProviderErrorPausedEvent {
  task_id: string;
  category: string;
  message: string;
  retry_after: string | null;
}

/**
 * Hook to listen for execution events from the backend
 *
 * Listens to 'execution:status_changed' and 'execution:queue_changed' events
 * and updates the UI store immediately for real-time feedback.
 *
 * Phase 82: Only updates UI store if the event's projectId matches the
 * currently active project (or if projectId is not specified in the event).
 *
 * This hook should be called once in the App component to enable
 * real-time execution status updates throughout the application.
 *
 * @example
 * ```tsx
 * function App() {
 *   useExecutionEvents(); // Enables real-time execution status
 *   return <AppContent />;
 * }
 * ```
 */
export function useExecutionEvents(projectIdOverride?: string | null) {
  const bus = useEventBus();
  const setExecutionStatus = useUiStore((state) => state.setExecutionStatus);
  const setExecutionQueuedCount = useUiStore(
    (state) => state.setExecutionQueuedCount
  );
  const updateTask = useTaskStore((state) => state.updateTask);

  useEffect(() => {
    const unsubscribes: Unsubscribe[] = [];

    // Listen for execution:status_changed events
    unsubscribes.push(
      bus.subscribe<ExecutionStatusEvent>("execution:status_changed", (payload) => {
        const {
          isPaused,
          haltMode,
          runningCount,
          maxConcurrent,
          globalMaxConcurrent,
          queuedMessageCount,
          projectId,
        } = payload;

        // Phase 82: Only update if event is for the active project (or unscoped)
        const activeProjectId = projectIdOverride ?? useProjectStore.getState().activeProjectId;
        if (projectId && activeProjectId && projectId !== activeProjectId) {
          // Event is for a different project - ignore
          return;
        }

        const currentStatus = useUiStore.getState().executionStatus;
        setExecutionStatus({
          isPaused,
          haltMode: haltMode ?? currentStatus.haltMode,
          runningCount,
          maxConcurrent,
          globalMaxConcurrent: globalMaxConcurrent ?? currentStatus.globalMaxConcurrent,
          // Preserve current queuedCount - will be updated by queue_changed event
          queuedCount: currentStatus.queuedCount,
          queuedMessageCount: queuedMessageCount ?? currentStatus.queuedMessageCount ?? 0,
          canStartTask: !isPaused && runningCount < maxConcurrent,
          // Preserve ideation stats - updated by getExecutionStatus polling
          ideationActive: currentStatus.ideationActive,
          ideationIdle: currentStatus.ideationIdle,
          ideationWaiting: currentStatus.ideationWaiting,
          ideationMaxProject: currentStatus.ideationMaxProject,
          ideationMaxGlobal: currentStatus.ideationMaxGlobal,
        });
      })
    );

    // Listen for execution:queue_changed events
    unsubscribes.push(
      bus.subscribe<ExecutionQueueEvent>("execution:queue_changed", (payload) => {
        const { queuedCount, queuedMessageCount, projectId } = payload;

        // Phase 82: Only update if event is for the active project (or unscoped)
        const activeProjectId = projectIdOverride ?? useProjectStore.getState().activeProjectId;
        if (projectId && activeProjectId && projectId !== activeProjectId) {
          // Event is for a different project - ignore
          return;
        }

        setExecutionQueuedCount(queuedCount, queuedMessageCount);
      })
    );

    // Listen for task:provider_error_paused events
    // Updates the task in the store to reflect the paused status immediately
    // Stores in new pause_reason format for unified parsing
    unsubscribes.push(
      bus.subscribe<ProviderErrorPausedEvent>("task:provider_error_paused", (payload) => {
        const { task_id } = payload;
        const task = useTaskStore.getState().tasks[task_id];
        if (task) {
          const existingMeta = task.metadata ? JSON.parse(task.metadata) : {};
          updateTask(task_id, {
            internalStatus: "paused" as InternalStatus,
            metadata: JSON.stringify({
              ...existingMeta,
              pause_reason: {
                type: "provider_error",
                category: payload.category,
                message: payload.message,
                retry_after: payload.retry_after,
                previous_status: task.internalStatus,
                paused_at: new Date().toISOString(),
                auto_resumable: payload.retry_after !== null,
                resume_attempts: 0,
              },
              // Keep legacy key for backward compat
              provider_error: {
                category: payload.category,
                message: payload.message,
                retry_after: payload.retry_after,
                previous_status: task.internalStatus,
                paused_at: new Date().toISOString(),
                auto_resumable: payload.retry_after !== null,
                resume_attempts: 0,
              },
            }),
          });
        }
      })
    );

    return () => {
      unsubscribes.forEach((unsub) => unsub());
    };
  }, [bus, projectIdOverride, setExecutionStatus, setExecutionQueuedCount, updateTask]);
}
