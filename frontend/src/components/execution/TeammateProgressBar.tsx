/**
 * TeammateProgressBar - Per-teammate step completion progress bar
 *
 * Shows a thin progress bar with warm orange fill and percentage label.
 * Used inside TeamProcessGroup for each teammate with step progress data.
 */

interface TeammateProgressBarProps {
  completed: number;
  total: number;
}

export function TeammateProgressBar({ completed, total }: TeammateProgressBarProps) {
  const percent = total > 0 ? Math.round((completed / total) * 100) : 0;

  return (
    <div className="flex items-center gap-1.5 flex-1 min-w-0">
      {/* Track */}
      <div
        className="flex-1 h-1 rounded-full overflow-hidden"
        style={{ backgroundColor: "var(--overlay-weak)" }}
      >
        {/* Fill */}
        <div
          className="h-full rounded-full transition-[width] duration-300 ease-out"
          style={{
            width: `${percent}%`,
            backgroundColor: "var(--accent-primary)",
          }}
        />
      </div>
      {/* Percentage */}
      <span
        className="text-[0.5625rem] font-medium tabular-nums shrink-0 w-7 text-right"
        style={{ color: "var(--text-muted)" }}
      >
        {percent}%
      </span>
    </div>
  );
}
