/**
 * DurationDisplay - Shows task duration in human-readable format
 *
 * Two modes:
 * - static: shows completed_at - started_at as formatted duration
 * - live: runs 1-second interval ticker counting up from started_at
 */

import { useState, useEffect } from "react";
import { Clock } from "lucide-react";

// ============================================================================
// Duration formatting
// ============================================================================

/**
 * Format elapsed seconds as human-readable duration.
 * Examples: "34s", "2m 34s", "1h 12m 34s"
 */
// eslint-disable-next-line react-refresh/only-export-components -- utility shared across components
export function formatDuration(seconds: number): string {
  if (seconds < 0) return "0s";

  const h = Math.floor(seconds / 3600);
  const m = Math.floor((seconds % 3600) / 60);
  const s = seconds % 60;

  if (h > 0) return `${h}h ${m}m ${s}s`;
  if (m > 0) return `${m}m ${s}s`;
  return `${s}s`;
}

/**
 * Calculate elapsed seconds between two ISO timestamps (or now).
 * Returns null if startedAt is null/invalid.
 */
// eslint-disable-next-line react-refresh/only-export-components -- utility shared across components
export function calcElapsedSeconds(
  startedAt: string | null,
  endedAt?: string | null
): number | null {
  if (!startedAt) return null;
  const start = new Date(startedAt).getTime();
  if (isNaN(start)) return null;
  const end = endedAt ? new Date(endedAt).getTime() : Date.now();
  if (endedAt && isNaN(end)) return null;
  return Math.floor(Math.max(0, end - start) / 1000);
}

// ============================================================================
// Component props
// ============================================================================

interface DurationDisplayStaticProps {
  mode: "static";
  startedAt: string | null;
  completedAt: string | null;
  className?: string;
}

interface DurationDisplayLiveProps {
  mode: "live";
  startedAt: string | null;
  className?: string;
}

type DurationDisplayProps = DurationDisplayStaticProps | DurationDisplayLiveProps;

// ============================================================================
// Component
// ============================================================================

/**
 * DurationDisplay renders a human-readable task duration.
 *
 * Static mode: shows the fixed duration between startedAt and completedAt.
 * Live mode: counts up from startedAt using a 1-second interval.
 *
 * Returns null when startedAt is missing (nothing to display).
 */
export function DurationDisplay(props: DurationDisplayProps) {
  const { mode, startedAt, className } = props;
  const completedAt = mode === "static" ? props.completedAt : null;

  // For live mode, track current elapsed seconds in state
  const [liveSeconds, setLiveSeconds] = useState<number | null>(() => {
    if (mode === "live") return calcElapsedSeconds(startedAt);
    return null;
  });

  useEffect(() => {
    if (mode !== "live" || !startedAt) return;

    // Initialize immediately
    setLiveSeconds(calcElapsedSeconds(startedAt));

    const id = setInterval(() => {
      setLiveSeconds(calcElapsedSeconds(startedAt));
    }, 1000);

    return () => clearInterval(id);
  }, [mode, startedAt]);

  const seconds =
    mode === "static"
      ? calcElapsedSeconds(startedAt, completedAt)
      : liveSeconds;

  // Nothing to display if we couldn't compute duration
  if (seconds === null) return null;

  const label = formatDuration(seconds);

  return (
    <span
      data-testid="duration-display"
      className={`inline-flex items-center gap-1 tabular-nums ${className ?? ""}`}
      style={{ color: "var(--text-muted)" }}
    >
      <Clock className="w-3 h-3 shrink-0" />
      <span className="text-[0.75rem]">{label}</span>
    </span>
  );
}
