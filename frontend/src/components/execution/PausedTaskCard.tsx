/**
 * PausedTaskCard - Compact two-line row for a paused task
 *
 * Supports two pause types:
 * - provider_error: category badge, countdown, resume attempts
 * - user_initiated: "Paused by user" label, timestamp, previous status
 */

import { Clock, Play, ExternalLink, AlertTriangle, WifiOff, Server, ShieldAlert, PauseCircle } from "lucide-react";
import { useCallback, useEffect, useState } from "react";
import { getStatusIconConfig } from "@/types/status-icons";
import type { Task } from "@/types/task";

/** Provider error metadata stored in task.metadata JSON */
export interface ProviderErrorMetadata {
  category: "rate_limit" | "auth_error" | "server_error" | "network_error" | "overloaded";
  message: string;
  retry_after: string | null;
  previous_status: string;
  paused_at: string;
  auto_resumable: boolean;
  resume_attempts: number;
}

/** User-initiated pause metadata */
export interface UserPauseMetadata {
  previous_status: string;
  paused_at: string;
  scope: string;
}

/** Discriminated union for pause reasons */
export type PauseReason =
  | { type: "provider_error" } & ProviderErrorMetadata
  | { type: "user_initiated" } & UserPauseMetadata;

interface PausedTaskCardProps {
  task: Task;
  pauseReason: PauseReason;
  onResume: (taskId: string) => void;
  onViewDetails: (taskId: string) => void;
}

const MAX_RESUME_ATTEMPTS = 5;

/** Icon + color for each error category */
function getCategoryStyle(category: ProviderErrorMetadata["category"]): {
  icon: React.ComponentType<{ className?: string; style?: React.CSSProperties }>;
  color: string;
  bgColor: string;
  label: string;
} {
  switch (category) {
    case "rate_limit":
      return {
        icon: Clock,
        color: "var(--status-warning)",
        bgColor: "var(--status-warning-muted)",
        label: "Rate Limit",
      };
    case "auth_error":
      return {
        icon: ShieldAlert,
        color: "var(--status-error)",
        bgColor: "var(--status-error-muted)",
        label: "Auth Error",
      };
    case "server_error":
      return {
        icon: Server,
        color: "var(--status-error)",
        bgColor: "var(--status-error-muted)",
        label: "Server Error",
      };
    case "network_error":
      return {
        icon: WifiOff,
        color: "var(--accent-primary)",
        bgColor: "var(--accent-muted)",
        label: "Network",
      };
    case "overloaded":
      return {
        icon: AlertTriangle,
        color: "var(--status-warning)",
        bgColor: "var(--status-warning-muted)",
        label: "Overloaded",
      };
  }
}

/** Format remaining time until retry */
function formatCountdown(retryAfter: string | null): string | null {
  if (!retryAfter) return null;

  const target = new Date(retryAfter).getTime();
  const now = Date.now();
  const diff = Math.max(0, Math.floor((target - now) / 1000));

  if (diff <= 0) return "Retrying soon...";

  const hours = Math.floor(diff / 3600);
  const mins = Math.floor((diff % 3600) / 60);
  const secs = diff % 60;

  if (hours > 0) return `${hours}h ${mins}m`;
  if (mins > 0) return `${mins}m ${secs}s`;
  return `${secs}s`;
}

/** Format time since pause */
function formatTimeSince(pausedAt: string): string {
  const diff = Math.max(0, Math.floor((Date.now() - new Date(pausedAt).getTime()) / 1000));
  if (diff < 60) return "just now";
  const mins = Math.floor(diff / 60);
  if (mins < 60) return `${mins}m ago`;
  const hours = Math.floor(mins / 60);
  return `${hours}h ${mins % 60}m ago`;
}

export function PausedTaskCard({ task, pauseReason, onResume, onViewDetails }: PausedTaskCardProps) {
  const pausedStyle = getStatusIconConfig("paused");
  const handleResume = useCallback(() => onResume(task.id), [onResume, task.id]);
  const handleView = useCallback(() => onViewDetails(task.id), [onViewDetails, task.id]);

  if (pauseReason.type === "user_initiated") {
    return (
      <UserPauseCard
        task={task}
        meta={pauseReason}
        pausedStyle={pausedStyle}
        onResume={handleResume}
        onView={handleView}
      />
    );
  }

  return (
    <ProviderErrorCard
      task={task}
      meta={pauseReason}
      pausedStyle={pausedStyle}
      onResume={handleResume}
      onView={handleView}
    />
  );
}

/** User-initiated pause card */
function UserPauseCard({
  task,
  meta,
  pausedStyle,
  onResume,
  onView,
}: {
  task: Task;
  meta: UserPauseMetadata;
  pausedStyle: { color: string };
  onResume: () => void;
  onView: () => void;
}) {
  const [timeSince, setTimeSince] = useState(() => formatTimeSince(meta.paused_at));

  useEffect(() => {
    const interval = setInterval(() => {
      setTimeSince(formatTimeSince(meta.paused_at));
    }, 30000);
    return () => clearInterval(interval);
  }, [meta.paused_at]);

  return (
    <div
      data-testid={`paused-task-card-${task.id}`}
      className="px-2 py-1.5 rounded-md hover:bg-white/[0.04] transition-colors"
    >
      {/* Line 1: Icon | Title | Badge | Actions */}
      <div className="flex items-center gap-2">
        <PauseCircle
          className="w-3.5 h-3.5 shrink-0"
          style={{ color: "var(--status-warning)" }}
        />
        <button
          className="flex-1 text-xs font-medium truncate min-w-0 text-left cursor-pointer hover:opacity-75 transition-opacity"
          style={{ color: "var(--text-primary)" }}
          title={task.title}
          onClick={onView}
        >
          {task.title}
        </button>
        <span
          className="text-[0.625rem] font-medium px-1.5 py-0.5 rounded shrink-0"
          style={{
            color: "var(--status-warning)",
            backgroundColor: "var(--status-warning-muted)",
          }}
        >
          User Paused
        </span>
        <div className="flex items-center shrink-0">
          <button
            data-testid={`resume-button-${task.id}`}
            onClick={onResume}
            className="w-6 h-6 flex items-center justify-center rounded hover:bg-white/[0.08] transition-colors"
            style={{ color: pausedStyle.color }}
            title="Resume now"
          >
            <Play className="w-3 h-3" />
          </button>
          <button
            data-testid={`view-details-button-${task.id}`}
            onClick={onView}
            className="w-6 h-6 flex items-center justify-center rounded hover:bg-white/[0.08] transition-colors"
            style={{ color: "var(--text-muted)" }}
            title="View details"
          >
            <ExternalLink className="w-3 h-3" />
          </button>
        </div>
      </div>

      {/* Line 2: "Paused by user" · time since · previous status */}
      <div
        className="flex items-center gap-1.5 mt-0.5 pl-[22px] text-[0.6875rem] min-w-0"
        style={{ color: "var(--text-muted)" }}
      >
        <span>Paused by user</span>
        <span className="shrink-0" style={{ color: "var(--text-muted)" }}>·</span>
        <span className="shrink-0 tabular-nums">{timeSince}</span>
        <span className="shrink-0" style={{ color: "var(--text-muted)" }}>·</span>
        <span className="shrink-0">was {meta.previous_status}</span>
      </div>
    </div>
  );
}

/** Provider error pause card */
function ProviderErrorCard({
  task,
  meta,
  pausedStyle,
  onResume,
  onView,
}: {
  task: Task;
  meta: ProviderErrorMetadata;
  pausedStyle: { color: string };
  onResume: () => void;
  onView: () => void;
}) {
  const catStyle = getCategoryStyle(meta.category);
  const CatIcon = catStyle.icon;

  // Live countdown ticker
  const [countdown, setCountdown] = useState(() => formatCountdown(meta.retry_after));

  useEffect(() => {
    if (!meta.retry_after) return;

    setCountdown(formatCountdown(meta.retry_after));
    const interval = setInterval(() => {
      setCountdown(formatCountdown(meta.retry_after));
    }, 1000);

    return () => clearInterval(interval);
  }, [meta.retry_after]);

  const truncatedMessage =
    meta.message.length > 60
      ? meta.message.slice(0, 57) + "..."
      : meta.message;

  return (
    <div
      data-testid={`paused-task-card-${task.id}`}
      className="px-2 py-1.5 rounded-md hover:bg-white/[0.04] transition-colors"
    >
      {/* Line 1: Icon | Title | Category Badge | Actions */}
      <div className="flex items-center gap-2">
        <CatIcon
          className="w-3.5 h-3.5 shrink-0"
          style={{ color: catStyle.color }}
        />
        <button
          className="flex-1 text-xs font-medium truncate min-w-0 text-left cursor-pointer hover:opacity-75 transition-opacity"
          style={{ color: "var(--text-primary)" }}
          title={task.title}
          onClick={onView}
        >
          {task.title}
        </button>
        <span
          className="text-[0.625rem] font-medium px-1.5 py-0.5 rounded shrink-0"
          style={{
            color: catStyle.color,
            backgroundColor: catStyle.bgColor,
          }}
        >
          {catStyle.label}
        </span>
        <div className="flex items-center shrink-0">
          <button
            data-testid={`resume-button-${task.id}`}
            onClick={onResume}
            className="w-6 h-6 flex items-center justify-center rounded hover:bg-white/[0.08] transition-colors"
            style={{ color: pausedStyle.color }}
            title="Resume now"
          >
            <Play className="w-3 h-3" />
          </button>
          <button
            data-testid={`view-details-button-${task.id}`}
            onClick={onView}
            className="w-6 h-6 flex items-center justify-center rounded hover:bg-white/[0.08] transition-colors"
            style={{ color: "var(--text-muted)" }}
            title="View details"
          >
            <ExternalLink className="w-3 h-3" />
          </button>
        </div>
      </div>

      {/* Line 2: Error reason · Countdown · Resume attempts */}
      <div
        className="flex items-center gap-1.5 mt-0.5 pl-[22px] text-[0.6875rem] min-w-0"
        style={{ color: "var(--text-muted)" }}
      >
        <span className="truncate min-w-0" title={meta.message}>
          {truncatedMessage}
        </span>
        {countdown && (
          <>
            <span className="shrink-0" style={{ color: "var(--text-muted)" }}>·</span>
            <span className="shrink-0 tabular-nums" style={{ color: pausedStyle.color }}>
              {countdown}
            </span>
          </>
        )}
        <span className="shrink-0" style={{ color: "var(--text-muted)" }}>·</span>
        <span className="shrink-0 tabular-nums">
          {meta.resume_attempts}/{MAX_RESUME_ATTEMPTS}
        </span>
        {meta.auto_resumable && (
          <>
            <span className="shrink-0" style={{ color: "var(--text-muted)" }}>·</span>
            <span
              className="text-[0.625rem] font-medium px-1 rounded shrink-0"
              style={{
                color: "var(--status-success)",
                backgroundColor: "var(--status-success-muted)",
              }}
            >
              Auto
            </span>
          </>
        )}
      </div>
    </div>
  );
}
