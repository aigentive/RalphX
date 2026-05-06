/**
 * QueuedTaskRow - Compact single-line row for queued task
 *
 * Layout: position | title | plan name | priority badge
 */

import { PriorityBadge } from "@/components/Ideation/PriorityBadge";
import { priorityFromScore } from "@/lib/priority";
import type { QueuedTask } from "@/hooks/useQueuedTasks";
import { useUiStore } from "@/stores/uiStore";

interface QueuedTaskRowProps {
  /** Queue position (1-indexed) */
  position: number;
  /** Task with plan title */
  task: QueuedTask;
}

export function QueuedTaskRow({ position, task }: QueuedTaskRowProps) {
  const priority = priorityFromScore(task.priority);
  const navigateToTask = useUiStore((s) => s.navigateToTask);

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
        onClick={() => navigateToTask(task.id)}
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
