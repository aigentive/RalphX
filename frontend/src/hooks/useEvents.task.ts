/**
 * Task event hooks - Tauri task event listeners with type-safe validation
 *
 * Uses EventBus abstraction for browser/Tauri compatibility.
 */

import { useEffect } from "react";
import { useQueryClient } from "@tanstack/react-query";
import { useEventBus } from "@/providers/EventProvider";
import { TaskEventSchema, TaskStatusChangedEventSchema } from "@/types/events";
import { useTaskStore } from "@/stores/taskStore";
import { taskKeys } from "@/hooks/useTasks";
import { infiniteTaskKeys } from "@/hooks/useInfiniteTasksQuery";
import { stateTransitionKeys } from "@/hooks/useTaskStateTransitions";
import { reviewKeys } from "@/hooks/useReviews";
import { ideationKeys } from "@/hooks/useIdeation";
import { transformTask, type Task } from "@/types/task";

const REVIEW_STATUSES = new Set([
  "pending_review",
  "reviewing",
  "review_passed",
  "escalated",
  "revision_needed",
]);

/**
 * Hook to listen for task events from the backend
 *
 * Listens to 'task:event' events and updates the task store accordingly.
 * Validates incoming events using TaskEventSchema before processing.
 *
 * @example
 * ```tsx
 * function App() {
 *   useTaskEvents(); // Sets up listener automatically
 *   return <TaskBoard />;
 * }
 * ```
 */
export function useTaskEvents() {
  const bus = useEventBus();
  const addTask = useTaskStore((s) => s.addTask);
  const updateTask = useTaskStore((s) => s.updateTask);
  const removeTask = useTaskStore((s) => s.removeTask);
  const queryClient = useQueryClient();

  useEffect(() => {
    const handleStatusChange = (
      taskId: string,
      from: Task["internalStatus"] | undefined,
      to: Task["internalStatus"],
    ) => {
      updateTask(taskId, { internalStatus: to });

      // Invalidate both regular and infinite task queries so Kanban refetches
      queryClient.invalidateQueries({ queryKey: taskKeys.lists() });
      queryClient.invalidateQueries({ queryKey: infiniteTaskKeys.all });
      // Invalidate state transitions so StateTimelineNav updates
      queryClient.invalidateQueries({ queryKey: stateTransitionKeys.task(taskId) });
      // Refetch full task data on every status change so detail view updates in real-time
      queryClient.invalidateQueries({ queryKey: taskKeys.detail(taskId) });
      // Invalidate review queries when transitioning to/from review stages
      if ((from !== undefined && REVIEW_STATUSES.has(from)) || REVIEW_STATUSES.has(to)) {
        queryClient.invalidateQueries({ queryKey: reviewKeys.all });
      }
      // Invalidate ideation sidebar counts and per-plan progress
      queryClient.invalidateQueries({ queryKey: ideationKeys.sessions() });
      // Bridge to graph hooks that listen for task:updated
      bus.emit("task:updated", { taskId });
    };

    const unsubscribeTaskEvent = bus.subscribe<unknown>("task:event", (payload) => {
      // Runtime validation of backend events
      const parsed = TaskEventSchema.safeParse(payload);

      if (!parsed.success) {
        return;
      }

      const taskEvent = parsed.data;

      switch (taskEvent.type) {
        case "created":
          addTask(transformTask(taskEvent.task));
          // Invalidate both regular and infinite task queries
          queryClient.invalidateQueries({ queryKey: taskKeys.lists() });
          queryClient.invalidateQueries({ queryKey: infiniteTaskKeys.all });
          // Invalidate ideation sidebar counts and per-plan progress
          queryClient.invalidateQueries({ queryKey: ideationKeys.sessions() });
          // Bridge to graph hooks that listen for task:updated
          bus.emit("task:updated", { taskId: taskEvent.task.id });
          break;
        case "updated": {
          // Transform snake_case changes to camelCase
          const transformedChanges: Partial<Task> = {};
          const changes = taskEvent.changes;
          if (changes.id !== undefined) transformedChanges.id = changes.id;
          if (changes.project_id !== undefined) transformedChanges.projectId = changes.project_id;
          if (changes.category !== undefined) transformedChanges.category = changes.category;
          if (changes.title !== undefined) transformedChanges.title = changes.title;
          if (changes.description !== undefined) transformedChanges.description = changes.description;
          if (changes.priority !== undefined) transformedChanges.priority = changes.priority;
          if (changes.internal_status !== undefined) transformedChanges.internalStatus = changes.internal_status;
          if (changes.needs_review_point !== undefined) transformedChanges.needsReviewPoint = changes.needs_review_point;
          if (changes.created_at !== undefined) transformedChanges.createdAt = changes.created_at;
          if (changes.updated_at !== undefined) transformedChanges.updatedAt = changes.updated_at;
          if (changes.started_at !== undefined) transformedChanges.startedAt = changes.started_at;
          if (changes.completed_at !== undefined) transformedChanges.completedAt = changes.completed_at;
          if (changes.archived_at !== undefined) transformedChanges.archivedAt = changes.archived_at;

          updateTask(taskEvent.taskId, transformedChanges);
          // Invalidate both regular and infinite task queries
          queryClient.invalidateQueries({ queryKey: taskKeys.lists() });
          queryClient.invalidateQueries({ queryKey: infiniteTaskKeys.all });
          // Invalidate review queries when internal_status changes (may be review-related)
          if (changes.internal_status !== undefined) {
            queryClient.invalidateQueries({ queryKey: reviewKeys.all });
          }
          // Invalidate ideation sidebar counts and per-plan progress
          queryClient.invalidateQueries({ queryKey: ideationKeys.sessions() });
          // Bridge to graph hooks that listen for task:updated
          bus.emit("task:updated", { taskId: taskEvent.taskId });
          break;
        }
        case "deleted":
          removeTask(taskEvent.taskId);
          // Invalidate both regular and infinite task queries
          queryClient.invalidateQueries({ queryKey: taskKeys.lists() });
          queryClient.invalidateQueries({ queryKey: infiniteTaskKeys.all });
          // Invalidate review queries — deleted task may have been in a review stage
          queryClient.invalidateQueries({ queryKey: reviewKeys.all });
          // Invalidate ideation sidebar counts and per-plan progress
          queryClient.invalidateQueries({ queryKey: ideationKeys.sessions() });
          break;
        case "status_changed":
          handleStatusChange(taskEvent.taskId, taskEvent.from, taskEvent.to);
          break;
      }
    });

    const unsubscribeLegacyStatus = bus.subscribe<unknown>("task:status_changed", (payload) => {
      const parsed = TaskStatusChangedEventSchema.safeParse(payload);
      if (!parsed.success) {
        return;
      }
      handleStatusChange(parsed.data.task_id, parsed.data.old_status, parsed.data.new_status);
    });

    return () => {
      unsubscribeTaskEvent();
      unsubscribeLegacyStatus();
    };
  }, [bus, addTask, updateTask, removeTask, queryClient]);
}
