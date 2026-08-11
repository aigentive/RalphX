/**
 * QueuedTasksPopover - Compact queued tasks list
 *
 * Dense row-based layout showing tasks waiting for execution slots.
 */

import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "@/components/ui/popover";
import { QueuedTaskRow } from "./QueuedTaskRow";
import { useQueuedTasks } from "@/hooks/useQueuedTasks";
import { shouldPreserveExecutionPopoverForTarget } from "./executionPopoverDismissal";
import type { ExecutionBarTaskNavigationTarget } from "./executionTaskNavigation";

interface QueuedTasksPopoverProps {
  /** The project ID */
  projectId: string;
  /** Number of queued tasks (for display) */
  queuedCount: number;
  /** Children to use as trigger (e.g., "Queued: 5" text) */
  children: React.ReactNode;
  /** Optional horizontal alignment offset for popover content */
  alignOffset?: number;
  /** Optional controlled open state */
  open?: boolean;
  /** Optional controlled open-state handler */
  onOpenChange?: (open: boolean) => void;
  /** Called when a queued task should open its Agent conversation task detail */
  onNavigateToTask?: (target: ExecutionBarTaskNavigationTarget) => void;
}

export function QueuedTasksPopover({
  projectId,
  queuedCount,
  children,
  alignOffset = -24,
  open,
  onOpenChange,
  onNavigateToTask,
}: QueuedTasksPopoverProps) {
  const { data: queuedTasks, isLoading } = useQueuedTasks(projectId);
  const controlledProps =
    open !== undefined
      ? {
          open,
          ...(onOpenChange !== undefined && { onOpenChange }),
        }
      : {};

  return (
    <Popover {...controlledProps}>
      <PopoverTrigger asChild>{children}</PopoverTrigger>
      <PopoverContent
        side="top"
        align="start"
        alignOffset={alignOffset}
        sideOffset={24}
        className="w-[400px] p-0"
        style={{
          backgroundColor: "var(--bg-surface)",
          border: "1px solid var(--overlay-weak)",
          borderRadius: "10px",
          boxShadow:
            "0 4px 16px var(--overlay-scrim), 0 12px 32px var(--overlay-scrim)",
        }}
        onInteractOutside={(event) => {
          if (shouldPreserveExecutionPopoverForTarget(event.target)) {
            event.preventDefault();
          }
        }}
      >
        {/* Header */}
        <div
          className="flex items-center justify-between px-3 py-2.5"
          style={{
            borderBottom: "1px solid var(--overlay-weak)",
          }}
        >
          <h3
            className="text-xs font-semibold"
            style={{ color: "var(--text-secondary)" }}
          >
            Queued Tasks ({queuedCount})
          </h3>
        </div>

        {/* Task List */}
        <div
          className="max-h-[320px] overflow-y-auto p-1.5"
          style={{
            scrollbarWidth: "thin",
            scrollbarColor: "var(--overlay-moderate) transparent",
          }}
        >
          {isLoading ? (
            <div
              className="py-6 text-center text-xs"
              style={{ color: "var(--text-muted)" }}
            >
              Loading...
            </div>
          ) : !queuedTasks || queuedTasks.length === 0 ? (
            <div
              className="py-6 text-center text-xs"
              style={{ color: "var(--text-muted)" }}
            >
              No tasks queued
            </div>
          ) : (
            queuedTasks.map((task, index) => (
              <QueuedTaskRow
                key={task.id}
                position={index + 1}
                task={task}
                onNavigateToTask={(target) => {
                  onOpenChange?.(false);
                  onNavigateToTask?.(target);
                }}
              />
            ))
          )}
        </div>

        {/* Footer */}
        <div
          className="px-3 py-2 text-[0.6875rem]"
          style={{
            borderTop: "1px solid var(--overlay-weak)",
            color: "var(--text-muted)",
          }}
        >
          Ready tasks queued by priority, oldest first within same priority.
        </div>
      </PopoverContent>
    </Popover>
  );
}
