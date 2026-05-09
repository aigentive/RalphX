/**
 * ProgressIndicator - macOS Tahoe-inspired progress bar
 *
 * Minimal design:
 * - No container/card wrapper
 * - Thin progress track (4px)
 * - Subtle status fill, no gradients or borders
 */

interface ProgressIndicatorProps {
  percentComplete: number;
  completedSteps?: number;
  totalSteps?: number;
  variant?: "accent" | "success" | "info";
}

const VARIANT_COLORS = {
  accent: "color-mix(in srgb, var(--accent-primary) 82%, var(--text-muted))",
  success: "color-mix(in srgb, var(--status-success) 78%, var(--text-muted))",
  info: "color-mix(in srgb, var(--status-info) 72%, var(--text-muted))",
};

export function ProgressIndicator({
  percentComplete,
  completedSteps,
  totalSteps,
  variant = "accent",
}: ProgressIndicatorProps) {
  const color = VARIANT_COLORS[variant];
  const showSteps = completedSteps !== undefined && totalSteps !== undefined && totalSteps > 0;

  return (
    <div className="space-y-2.5">
      {/* Labels row */}
      <div className="flex items-center justify-between">
        {showSteps && (
          <span
            className="text-[0.75rem]"
            style={{ color: "var(--text-muted)" }}
          >
            Step{" "}
            <span
              className="font-medium tabular-nums"
              style={{ color: "var(--text-secondary)" }}
            >
              {completedSteps}
            </span>
            {" "}of{" "}
            <span
              className="font-medium tabular-nums"
              style={{ color: "var(--text-secondary)" }}
            >
              {totalSteps}
            </span>
          </span>
        )}
        <span
          className="text-[0.75rem] font-medium ml-auto tabular-nums"
          style={{ color }}
        >
          {Math.round(percentComplete)}%
        </span>
      </div>

      {/* Progress track - thin, minimal */}
      <div
        className="relative h-1 rounded-full overflow-hidden"
        style={{ backgroundColor: "var(--bg-elevated)" }}
      >
        {/* Progress fill */}
        <div
          className="absolute inset-y-0 left-0 rounded-full transition-all duration-500 ease-out"
          style={{
            width: `${Math.max(0, Math.min(100, percentComplete))}%`,
            backgroundColor: color,
          }}
        />
      </div>
    </div>
  );
}
