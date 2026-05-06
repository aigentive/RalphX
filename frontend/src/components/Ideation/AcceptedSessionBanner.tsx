/**
 * AcceptedSessionBanner - Shows acceptance status, live task counts, and "View Work" CTA
 *
 * Rendered at the top of PlanningView when session.status === "accepted".
 * Task counts are live/reactive via the existing useTasks query.
 */

import { useMemo } from "react";
import { CheckCircle2, ArrowRight, Clock, Zap, CircleCheck } from "lucide-react";
import { useTasks } from "@/hooks/useTasks";
import { withAlpha } from "@/lib/theme-colors";
import type { TaskProposal } from "@/types/ideation";
import { getStatusCounts } from "@/types/status";

interface AcceptedSessionBannerProps {
  projectId: string;
  proposals: TaskProposal[];
  convertedAt: string | null;
  onViewWork: () => void;
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

export function AcceptedSessionBanner({
  projectId,
  proposals,
  convertedAt,
  onViewWork,
}: AcceptedSessionBannerProps) {
  const { data: allTasks } = useTasks(projectId);

  const createdTaskIds = useMemo(
    () => new Set(proposals.filter((p) => p.createdTaskId != null).map((p) => p.createdTaskId!)),
    [proposals]
  );

  const sessionTasks = useMemo(
    () => (allTasks ?? []).filter((t) => createdTaskIds.has(t.id)),
    [allTasks, createdTaskIds]
  );

  const counts = useMemo(() => getStatusCounts(sessionTasks), [sessionTasks]);

  if (createdTaskIds.size === 0) return null;

  return (
    <div
      data-testid="accepted-session-banner"
      className="mb-4 rounded-xl overflow-hidden"
      style={{
        background: `linear-gradient(135deg, ${withAlpha("var(--status-success)", 12)} 0%, ${withAlpha("var(--status-success)", 4)} 100%)`,
        border: `1px solid ${withAlpha("var(--status-success)", 35)}`,
        boxShadow: `0 0 32px ${withAlpha("var(--status-success)", 8)}, inset 0 1px 0 ${withAlpha("var(--status-success)", 15)}`,
      }}
    >
      <div className="px-5 py-4">
        {/* Header row */}
        <div className="flex items-center justify-between mb-3">
          <div className="flex items-center gap-2.5">
            <div
              className="w-7 h-7 rounded-full flex items-center justify-center"
              style={{
                background: withAlpha("var(--status-success)", 18),
                border: `1px solid ${withAlpha("var(--status-success)", 40)}`,
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

          <button
            data-testid="view-work-button"
            onClick={onViewWork}
            className="flex items-center gap-1.5 px-4 py-2 rounded-lg text-[0.8125rem] font-semibold transition-all duration-150"
            style={{
              background: "var(--status-success)",
              color: "var(--text-inverse)",
              boxShadow: `0 1px 4px ${withAlpha("var(--status-success)", 30)}`,
            }}
            onMouseEnter={(e) => {
              e.currentTarget.style.background = withAlpha("var(--status-success)", 90);
              e.currentTarget.style.boxShadow = `0 2px 8px ${withAlpha("var(--status-success)", 40)}`;
            }}
            onMouseLeave={(e) => {
              e.currentTarget.style.background = "var(--status-success)";
              e.currentTarget.style.boxShadow = `0 1px 4px ${withAlpha("var(--status-success)", 30)}`;
            }}
          >
            View Work
            <ArrowRight className="w-3.5 h-3.5" />
          </button>
        </div>

        {/* Status summary */}
        <div
          className="flex items-center gap-4 pt-3"
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

          {counts.idle > 0 && counts.active === 0 && counts.done === 0 && (
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
