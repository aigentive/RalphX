import { ArrowRight, CheckCircle2, CircleCheck, Clock, Loader2, RotateCcw, Zap } from "lucide-react";

import { withAlpha } from "@/lib/theme-colors";
import type { StatusCounts } from "@/types/status";

interface AcceptedPlanProgressBannerProps {
  counts: StatusCounts;
  convertedAt: string | null;
  onViewWork: () => void;
  onRestartImplementation?: () => void;
  isRestartingImplementation?: boolean;
}

function formatTimestamp(iso: string): string {
  try {
    const date = new Date(iso);
    return date.toLocaleDateString(undefined, {
      month: "short",
      day: "numeric",
      hour: "numeric",
      minute: "2-digit",
    });
  } catch {
    return "";
  }
}

export function AcceptedPlanProgressBanner({
  counts,
  convertedAt,
  onViewWork,
  onRestartImplementation,
  isRestartingImplementation = false,
}: AcceptedPlanProgressBannerProps) {
  if (counts.total === 0) return null;

  return (
    <div
      data-testid="accepted-session-banner"
      className="mb-4 rounded-lg overflow-hidden"
      style={{
        backgroundColor: withAlpha("var(--status-success)", 10),
        borderColor: withAlpha("var(--status-success)", 35),
        borderStyle: "solid",
        borderWidth: 1,
        boxShadow: `0 0 32px ${withAlpha("var(--status-success)", 8)}, inset 0 1px 0 ${withAlpha("var(--status-success)", 15)}`,
      }}
    >
      <div className="px-5 py-4">
        <div className="flex items-center justify-between gap-3 mb-3">
          <div className="flex items-center gap-2.5">
            <div
              className="w-7 h-7 rounded-full flex items-center justify-center"
              style={{
                backgroundColor: withAlpha("var(--status-success)", 18),
                borderColor: withAlpha("var(--status-success)", 40),
                borderStyle: "solid",
                borderWidth: 1,
              }}
            >
              <CheckCircle2 className="w-4 h-4" style={{ color: "var(--status-success)" }} />
            </div>
            <div className="flex flex-col leading-tight">
              <span className="text-[0.9375rem] font-semibold" style={{ color: "var(--text-primary)" }}>
                Plan accepted
              </span>
              {convertedAt && (
                <span className="text-[0.6875rem]" style={{ color: "var(--text-muted)" }}>
                  {formatTimestamp(convertedAt)}
                </span>
              )}
            </div>
          </div>

          <div className="flex items-center gap-2">
            {onRestartImplementation && (
              <button
                data-testid="restart-implementation-button"
                type="button"
                onClick={onRestartImplementation}
                disabled={isRestartingImplementation}
                className="flex items-center gap-1.5 px-3 py-2 rounded-lg text-[0.8125rem] font-semibold transition-colors duration-150 disabled:cursor-not-allowed disabled:opacity-60"
                style={{
                  backgroundColor: withAlpha("var(--status-error)", 9),
                  borderColor: withAlpha("var(--status-error)", 28),
                  borderStyle: "solid",
                  borderWidth: 1,
                  color: "var(--status-error)",
                }}
              >
                {isRestartingImplementation ? (
                  <Loader2 className="w-3.5 h-3.5 animate-spin" />
                ) : (
                  <RotateCcw className="w-3.5 h-3.5" />
                )}
                {isRestartingImplementation ? "Restarting..." : "Restart Implementation"}
              </button>
            )}

            <button
              data-testid="view-work-button"
              type="button"
              onClick={onViewWork}
              className="flex items-center gap-1.5 px-4 py-2 rounded-lg text-[0.8125rem] font-semibold transition-colors duration-150"
              style={{
                backgroundColor: "var(--status-success)",
                color: "var(--text-inverse)",
                boxShadow: `0 1px 4px ${withAlpha("var(--status-success)", 30)}`,
              }}
              onMouseEnter={(e) => {
                e.currentTarget.style.backgroundColor = withAlpha("var(--status-success)", 90);
                e.currentTarget.style.boxShadow = `0 2px 8px ${withAlpha("var(--status-success)", 40)}`;
              }}
              onMouseLeave={(e) => {
                e.currentTarget.style.backgroundColor = "var(--status-success)";
                e.currentTarget.style.boxShadow = `0 1px 4px ${withAlpha("var(--status-success)", 30)}`;
              }}
            >
              View Work
              <ArrowRight className="w-3.5 h-3.5" />
            </button>
          </div>
        </div>

        <div
          className="flex flex-wrap items-center gap-4 pt-3"
          style={{ borderTop: `1px solid ${withAlpha("var(--status-success)", 15)}` }}
        >
          <span className="text-[0.8125rem] font-medium" style={{ color: "var(--text-secondary)" }}>
            {counts.total} {counts.total === 1 ? "task" : "tasks"}
          </span>

          {counts.active > 0 && (
            <div className="flex items-center gap-1.5">
              <Zap className="w-3.5 h-3.5" style={{ color: "var(--accent-primary)" }} />
              <span className="text-[0.75rem] font-medium" style={{ color: "var(--accent-primary)" }}>
                {counts.active} in progress
              </span>
            </div>
          )}

          {counts.done > 0 && (
            <div className="flex items-center gap-1.5">
              <CircleCheck className="w-3.5 h-3.5" style={{ color: "var(--status-success)" }} />
              <span className="text-[0.75rem] font-medium" style={{ color: "var(--status-success)" }}>
                {counts.done} completed
              </span>
            </div>
          )}

          {counts.idle > 0 && (
            <div className="flex items-center gap-1.5">
              <Clock className="w-3.5 h-3.5" style={{ color: "var(--text-muted)" }} />
              <span className="text-[0.75rem]" style={{ color: "var(--text-muted)" }}>
                {counts.idle} queued
              </span>
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
