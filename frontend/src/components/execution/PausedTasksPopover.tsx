/**
 * PausedTasksPopover - Compact popover showing all paused tasks
 *
 * Follows MergePipelinePopover pattern: glass panel, scrollable list,
 * header with total count, footer with explanation.
 *
 * Supports two pause types:
 * - provider_error: auto-resumable provider errors (rate limit, server, etc.)
 * - user_initiated: manually paused by user
 */

import { useState } from "react";
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "@/components/ui/popover";
import { PausedTaskCard, type PauseReason } from "./PausedTaskCard";
import { shouldPreserveExecutionPopoverForTarget } from "./executionPopoverDismissal";
import {
  taskRowNavigationTarget,
  type ExecutionBarTaskNavigationTarget,
} from "./executionTaskNavigation";
import type { Task } from "@/types/task";
import { api } from "@/lib/tauri";

interface PausedTasksPopoverProps {
  /** Paused tasks to display */
  pausedTasks: Task[];
  /** Children to use as trigger (e.g., "Paused: 3" text) */
  children: React.ReactNode;
  /** Optional horizontal alignment offset for popover content */
  alignOffset?: number;
  /** Optional controlled open state */
  open?: boolean;
  /** Optional controlled open-state handler */
  onOpenChange?: (open: boolean) => void;
  /** Called when a paused task should open its Agent conversation task detail */
  onNavigateToTask?: (target: ExecutionBarTaskNavigationTarget) => void;
}

/** Parse pause_reason from task.metadata JSON string, with legacy fallback */
// eslint-disable-next-line react-refresh/only-export-components -- exported for tests
export function parsePauseReason(task: Task): PauseReason | null {
  if (!task.metadata) return null;
  try {
    const parsed = JSON.parse(task.metadata);

    // New format: pause_reason with type discriminator
    if (parsed?.pause_reason) {
      const pr = parsed.pause_reason;
      if (pr.type === "user_initiated") {
        return {
          type: "user_initiated",
          previous_status: pr.previous_status ?? "executing",
          paused_at: pr.paused_at ?? new Date().toISOString(),
          scope: pr.scope ?? "global",
        };
      }
      if (pr.type === "provider_error") {
        return {
          type: "provider_error",
          category: pr.category ?? "server_error",
          message: pr.message ?? "Unknown error",
          retry_after: pr.retry_after ?? null,
          previous_status: pr.previous_status ?? "executing",
          paused_at: pr.paused_at ?? new Date().toISOString(),
          auto_resumable: pr.auto_resumable ?? false,
          resume_attempts: pr.resume_attempts ?? 0,
        };
      }
    }

    // Legacy format: provider_error at top level
    if (parsed?.provider_error) {
      const pe = parsed.provider_error;
      return {
        type: "provider_error",
        category: pe.category ?? "server_error",
        message: pe.message ?? "Unknown error",
        retry_after: pe.retry_after ?? null,
        previous_status: pe.previous_status ?? "executing",
        paused_at: pe.paused_at ?? new Date().toISOString(),
        auto_resumable: pe.auto_resumable ?? false,
        resume_attempts: pe.resume_attempts ?? 0,
      };
    }
  } catch {
    // Invalid JSON - skip
  }
  return null;
}

export function PausedTasksPopover({
  pausedTasks,
  children,
  alignOffset = -24,
  open,
  onOpenChange,
  onNavigateToTask,
}: PausedTasksPopoverProps) {
  const [uncontrolledOpen, setUncontrolledOpen] = useState(false);
  const popoverOpen = open ?? uncontrolledOpen;
  const handleOpenChange = onOpenChange ?? setUncontrolledOpen;
  const handleResume = async (taskId: string) => {
    try {
      await api.tasks.resume(taskId);
    } catch (error) {
      console.error("Failed to resume paused task:", error);
    }
  };

  const handleViewDetails = (taskId: string) => {
    const task = pausedTasks.find((item) => item.id === taskId);
    handleOpenChange(false);
    if (task) {
      onNavigateToTask?.(taskRowNavigationTarget(task, "paused"));
    }
  };

  // Parse all paused tasks with their reasons
  const tasksWithReasons = pausedTasks
    .map((task) => ({ task, reason: parsePauseReason(task) }))
    .filter((entry): entry is { task: Task; reason: PauseReason } => entry.reason !== null);

  // Split into groups
  const providerErrors = tasksWithReasons.filter((e) => e.reason.type === "provider_error");
  const userPaused = tasksWithReasons.filter((e) => e.reason.type === "user_initiated");
  // Tasks with no parseable reason still show (generic paused)
  const unparsed = pausedTasks.filter(
    (t) => !tasksWithReasons.some((e) => e.task.id === t.id)
  );

  const hasBothGroups = providerErrors.length > 0 && (userPaused.length > 0 || unparsed.length > 0);

  return (
    <Popover open={popoverOpen} onOpenChange={handleOpenChange}>
      <PopoverTrigger asChild>{children}</PopoverTrigger>
      <PopoverContent
        side="top"
        align="start"
        alignOffset={alignOffset}
        sideOffset={24}
        className="w-[420px] p-0"
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
            Paused Tasks ({pausedTasks.length})
          </h3>
          <span
            className="text-[0.6875rem] tabular-nums"
            style={{ color: "var(--text-muted)" }}
          >
            {providerErrors.length > 0 && userPaused.length > 0
              ? `${providerErrors.length} errors · ${userPaused.length} user`
              : providerErrors.length > 0
                ? "provider errors"
                : "user paused"}
          </span>
        </div>

        {/* Scrollable content */}
        <div
          className="max-h-[320px] overflow-y-auto p-1.5"
          style={{
            scrollbarWidth: "thin",
            scrollbarColor: "var(--overlay-moderate) transparent",
          }}
        >
          {pausedTasks.length === 0 ? (
            <div
              className="py-6 text-center text-xs"
              style={{ color: "var(--text-muted)" }}
            >
              No paused tasks
            </div>
          ) : (
            <>
              {/* Provider Errors section */}
              {providerErrors.length > 0 && hasBothGroups && (
                <div
                  className="px-2 py-1 text-[0.625rem] font-semibold uppercase tracking-wider"
                  style={{ color: "var(--text-muted)" }}
                >
                  Provider Errors
                </div>
              )}
              {providerErrors.map((entry) => (
                <PausedTaskCard
                  key={entry.task.id}
                  task={entry.task}
                  pauseReason={entry.reason}
                  onResume={handleResume}
                  onViewDetails={handleViewDetails}
                />
              ))}

              {/* User Paused section */}
              {userPaused.length > 0 && hasBothGroups && (
                <div
                  className="px-2 py-1 mt-1 text-[0.625rem] font-semibold uppercase tracking-wider"
                  style={{ color: "var(--text-muted)" }}
                >
                  User Paused
                </div>
              )}
              {userPaused.map((entry) => (
                <PausedTaskCard
                  key={entry.task.id}
                  task={entry.task}
                  pauseReason={entry.reason}
                  onResume={handleResume}
                  onViewDetails={handleViewDetails}
                />
              ))}

              {/* Unparsed paused tasks - show as user-initiated with fallback */}
              {unparsed.map((task) => (
                <PausedTaskCard
                  key={task.id}
                  task={task}
                  pauseReason={{
                    type: "user_initiated",
                    previous_status: "executing",
                    paused_at: new Date().toISOString(),
                    scope: "unknown",
                  }}
                  onResume={handleResume}
                  onViewDetails={handleViewDetails}
                />
              ))}
            </>
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
          Click Resume to restart individual tasks. Auto-resumable tasks retry automatically.
        </div>
      </PopoverContent>
    </Popover>
  );
}
