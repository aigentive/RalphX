/**
 * StepProgressBar component
 *
 * Displays visual progress dots for task steps with optional text summary.
 * Compact mode shows just dots and count, full mode includes text.
 */

import { useStepProgress } from "@/hooks/useTaskSteps";
import { getStepProgressDisplay } from "@/types/task-step";

interface StepProgressBarProps {
  taskId: string;
  compact?: boolean;
  internalStatus?: string;
}

const COMPACT_DOT_CAP = 19;

/**
 * Get background color class for step dot based on status
 * @param isTerminalComplete - whether the task is in a terminal state (merged/approved)
 */
function getStepDotColor(
  index: number,
  completed: number,
  skipped: number,
  failed: number,
  inProgress: number,
  isTerminalComplete: boolean = true
): string {
  const completedAndSkipped = completed + skipped;
  const failedStart = completedAndSkipped;
  const failedEnd = failedStart + failed;
  const inProgressStart = failedEnd;
  const inProgressEnd = inProgressStart + inProgress;

  if (index < completed) {
    // Completed steps - green only when terminal, muted otherwise
    return isTerminalComplete ? "bg-status-success" : "bg-text-muted";
  } else if (index < completedAndSkipped) {
    // Skipped steps
    return "bg-text-muted";
  } else if (index < failedEnd) {
    // Failed steps
    return "bg-status-error";
  } else if (index < inProgressEnd) {
    // In progress steps
    return "bg-accent-primary animate-pulse";
  } else {
    // Pending steps
    return "bg-border-default";
  }
}

/**
 * StepProgressBar Component
 *
 * Renders a visual progress indicator:
 * - Compact mode: thin progress bar with percentage (for TaskCard)
 * - Full mode: dots with count (for detail views)
 *
 * @example
 * ```tsx
 * // Compact mode for TaskCard - shows bar + percentage
 * <StepProgressBar taskId="task-123" compact />
 *
 * // Full mode with dots
 * <StepProgressBar taskId="task-123" />
 * ```
 */
export function StepProgressBar({ taskId, compact = false, internalStatus }: StepProgressBarProps) {
  const { data: progress, isLoading } = useStepProgress(taskId);

  // Don't render anything while loading, if no data, or if there are no steps
  if (isLoading || !progress || progress.total === 0) {
    return null;
  }

  const { total: rawTotal, completed, skipped, failed, inProgress } = progress;
  const progressDisplay = getStepProgressDisplay(progress);

  if (progressDisplay.total === 0) {
    return null;
  }

  const percentComplete = Math.round(progressDisplay.completedPercent);
  const activeSegmentStart = Math.max(0, Math.min(100, progressDisplay.completedPercent));
  const activeSegmentEnd = Math.max(
    activeSegmentStart,
    Math.min(100, progressDisplay.activePercent)
  );
  const activeSegmentWidth = activeSegmentEnd - activeSegmentStart;
  const showActiveSegment = inProgress > 0 && activeSegmentWidth > 0;

  // Determine if task is in terminal state (merged or approved)
  // Default to true for backward compatibility: show completed dots as green unless explicitly set to non-terminal state
  const isTerminalComplete =
    internalStatus === undefined || internalStatus === "merged" || internalStatus === "approved";

  // Compact mode: progress bar + percentage + dots for TaskCard
  if (compact) {
    const visibleDotCount = Math.min(rawTotal, COMPACT_DOT_CAP);
    const hiddenDotCount = Math.max(0, rawTotal - COMPACT_DOT_CAP);
    return (
      <div className="flex-1 space-y-1.5">
        {/* Progress bar row with percentage */}
        <div className="flex items-center gap-2">
          <div
            className="relative flex-1 h-1 rounded-full overflow-hidden"
            style={{ backgroundColor: "var(--kanban-progress-track)" }}
          >
            <div
              className="absolute inset-y-0 left-0 rounded-full transition-all duration-300"
              style={{
                width: `${activeSegmentStart}%`,
                backgroundColor: "var(--status-success)",
              }}
            />
            {showActiveSegment && (
              <div
                className="step-progress-active-segment absolute inset-y-0 rounded-full transition-all duration-300"
                data-animated="true"
                aria-hidden="true"
                style={{
                  left: `${activeSegmentStart}%`,
                  width: `${activeSegmentWidth}%`,
                  backgroundColor: "var(--status-success)",
                }}
              />
            )}
          </div>
          <span
            className="text-[0.625rem] tabular-nums shrink-0"
            style={{ color: "var(--text-muted)" }}
          >
            {percentComplete}%
          </span>
        </div>
        {/* Dots row */}
        <div className="flex items-center gap-1 min-w-0">
          {Array.from({ length: visibleDotCount }).map((_, index) => (
            <div
              key={index}
              className={`h-1.5 w-1.5 rounded-full transition-colors ${getStepDotColor(
                index,
                completed,
                skipped,
                failed,
                inProgress,
                isTerminalComplete
              )}`}
              aria-hidden="true"
            />
          ))}
          {hiddenDotCount > 0 && (
            <span
              className="text-[0.625rem] shrink-0"
              style={{ color: "var(--text-muted)" }}
              aria-label={`${hiddenDotCount} more steps`}
            >
              +{hiddenDotCount} more
            </span>
          )}
        </div>
      </div>
    );
  }

  // Full mode: dots with count
  return (
    <div className="flex items-center gap-2">
      {/* Progress dots */}
      <div className="flex items-center gap-1">
        {Array.from({ length: rawTotal }).map((_, index) => (
          <div
            key={index}
            className={`h-1.5 w-1.5 rounded-full transition-colors ${getStepDotColor(
              index,
              completed,
              skipped,
              failed,
              inProgress,
              isTerminalComplete
            )}`}
            aria-hidden="true"
          />
        ))}
      </div>

      {/* Text summary */}
      <span className="text-xs text-text-muted">
        {progressDisplay.completed}/{progressDisplay.total}
      </span>
    </div>
  );
}
