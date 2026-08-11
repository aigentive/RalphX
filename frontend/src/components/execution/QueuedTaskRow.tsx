/**
 * QueuedTaskRow - Compact single-line row for queued task
 *
 * Layout: position | title | plan name | priority badge
 */

import { PriorityBadge } from "@/components/Ideation/PriorityBadge";
import { priorityFromScore } from "@/lib/priority";
import type { QueuedTask } from "@/hooks/useQueuedTasks";
import {
  taskRowNavigationTarget,
  type ExecutionBarTaskNavigationTarget,
} from "./executionTaskNavigation";

interface QueuedTaskRowProps {
  /** Queue position (1-indexed) */
  position: number;
  /** Task with plan title */
  task: QueuedTask;
  /** Called when the task title should open its Agent conversation task detail */
  onNavigateToTask?: (target: ExecutionBarTaskNavigationTarget) => void;
}

export function QueuedTaskRow({
  position,
  task,
  onNavigateToTask,
}: QueuedTaskRowProps) {
  const priority = priorityFromScore(task.priority);

  return (
    <div
      data-testid="queued-task-row"
      className="flex items-center gap-2 px-2 py-1.5 rounded-md hover:bg-white/[0.04] transition-colors"
    >
      <span
        className="text-[0.6875rem] tabular-nums shrink-0 w-4 text-right"
        style={{ color: "var(--text-muted)" }}
      >
        {position}
      </span>
      <button
        className="flex-1 text-xs font-medium truncate min-w-0 text-left cursor-pointer hover:opacity-75 transition-opacity"
        style={{ color: "var(--text-primary)" }}
        onClick={() => onNavigateToTask?.(taskRowNavigationTarget(task, "queued"))}
      >
        {task.title}
      </button>
      <span
        className="text-[0.6875rem] shrink-0 max-w-[100px] truncate"
        style={{ color: "var(--text-muted)" }}
      >
        {task.planTitle}
      </span>
      <PriorityBadge priority={priority} size="compact" />
    </div>
  );
}
