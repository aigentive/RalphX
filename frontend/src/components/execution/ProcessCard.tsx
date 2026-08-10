/**
 * ProcessCard - Compact two-line row for a running process
 *
 * Line 1: status spinner | title | status badge | pause/stop
 * Line 2: step progress · elapsed · origin · branch
 */

import { Pause, Square, Loader2 } from "lucide-react";
import type { RunningProcess } from "@/api/running-processes";
import { getStatusIconConfig } from "@/types/status-icons";
import { getStepProgressDisplay } from "@/types/task-step";
import { useElapsedTimer } from "@/hooks/useElapsedTimer";
import { formatElapsedTime } from "@/lib/formatters";
import { useAgentGate } from "@/hooks/useAgentGate";
import {
  Tooltip,
  TooltipContent,
  TooltipTrigger,
} from "@/components/ui/tooltip";

interface ProcessCardProps {
  /** The running process data */
  process: RunningProcess;
  /** Called when pause button clicked */
  onPause: (taskId: string) => void;
  /** Called when stop button clicked */
  onStop: (taskId: string) => void;
  /** Whether pause/stop actions are in progress */
  isLoading?: boolean;
  /** Called when the card row is clicked to navigate to the task */
  onNavigate?: (taskId: string) => void;
}

/**
 * Get status badge color and icon based on internal status
 */
function getStatusBadgeStyle(status: string): {
  color: string;
  bgColor: string;
  label: string;
} {
  const statusConfig = getStatusIconConfig(status);
  const statusLower = status.toLowerCase();
  const opacityPct = Math.round(parseFloat(statusConfig.bgOpacity) * 100);
  const bgColor = `color-mix(in srgb, ${statusConfig.color} ${opacityPct}%, transparent)`;

  switch (statusLower) {
    case "executing":
      return {
        color: statusConfig.color,
        bgColor,
        label: "Executing",
      };
    case "re_executing":
      return {
        color: statusConfig.color,
        bgColor,
        label: "Re-executing",
      };
    case "reviewing":
      return {
        color: statusConfig.color,
        bgColor,
        label: "Reviewing",
      };
    case "qa_refining":
      return {
        color: statusConfig.color,
        bgColor,
        label: "QA Refining",
      };
    case "qa_testing":
      return {
        color: statusConfig.color,
        bgColor,
        label: "QA Testing",
      };
    case "merging":
      return {
        color: statusConfig.color,
        bgColor,
        label: "Merging",
      };
    case "waiting_on_pr":
      return {
        color: statusConfig.color,
        bgColor,
        label: "Waiting on PR",
      };
    default:
      return {
        color: statusConfig.color,
        bgColor,
        label: status,
      };
  }
}

/**
 * Get origin badge style based on trigger origin
 */
function getOriginBadgeStyle(origin: string | null): {
  color: string;
  bgColor: string;
  label: string;
} | null {
  if (!origin) return null;

  const originLower = origin.toLowerCase();

  switch (originLower) {
    case "scheduler":
      return {
        color: "var(--text-secondary)",
        bgColor: "var(--overlay-moderate)",
        label: "Scheduled",
      };
    case "revision":
      return {
        color: "var(--accent-primary)",
        bgColor: "var(--accent-muted)",
        label: "Revision",
      };
    case "recovery":
      return {
        color: "var(--status-warning)",
        bgColor: "var(--status-warning-muted)",
        label: "Recovered",
      };
    case "retry":
      return {
        color: "var(--status-info)",
        bgColor: "var(--status-info-muted)",
        label: "Retried",
      };
    case "qa":
      return {
        color: "var(--status-info)",
        bgColor: "var(--status-info-muted)",
        label: "QA Cycle",
      };
    default:
      return {
        color: "var(--text-secondary)",
        bgColor: "var(--overlay-moderate)",
        label: origin,
      };
  }
}

export function ProcessCard({
  process,
  onPause,
  onStop,
  isLoading = false,
  onNavigate,
}: ProcessCardProps) {
  const statusStyle = getStatusBadgeStyle(process.internalStatus);
  const originStyle = getOriginBadgeStyle(process.triggerOrigin);
  const pauseGate = useAgentGate("taskPause");

  // Live elapsed time ticker (updates every second)
  const elapsedTime = useElapsedTimer(process.elapsedSeconds, process.taskId);

  const stepDisplay = process.stepProgress
    ? getStepProgressDisplay(process.stepProgress)
    : null;
  const stepInfo =
    stepDisplay && stepDisplay.total > 0
      ? `${stepDisplay.completed}/${stepDisplay.total} steps`
      : null;

  return (
    <div
      data-testid={`process-card-${process.taskId}`}
      className="px-2 py-1.5 rounded-md hover:bg-[var(--overlay-faint)] transition-colors cursor-pointer focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-[var(--accent-border)]"
      role="button"
      tabIndex={0}
      onClick={() => onNavigate?.(process.taskId)}
      onKeyDown={(e) => {
        if (e.key === "Enter" || e.key === " ") {
          e.preventDefault();
          onNavigate?.(process.taskId);
        }
      }}
    >
      {/* Line 1: Spinner | Title | Status Badge | Actions */}
      <div className="flex items-center gap-2">
        <Loader2
          className="w-3.5 h-3.5 animate-spin shrink-0"
          style={{ color: statusStyle.color }}
        />
        <span
          className="flex-1 text-xs font-medium truncate min-w-0 text-left"
          style={{ color: "var(--text-primary)" }}
          title={process.title}
        >
          {process.title}
        </span>
        <span
          className="text-[0.625rem] font-medium px-1.5 py-0.5 rounded shrink-0"
          style={{
            color: statusStyle.color,
            backgroundColor: statusStyle.bgColor,
          }}
        >
          {statusStyle.label}
        </span>
        <div className="flex items-center shrink-0">
          {/* Soft-disabled when gated, never `disabled`: a disabled button has
              pointer-events:none and leaves the focus order, so its reason would be
              unreachable by both mouse and keyboard. Same rule the explained menu
              items follow. */}
          <Tooltip>
            <TooltipTrigger asChild>
              <button
                data-testid={`pause-button-${process.taskId}`}
                aria-label="Pause task"
                onClick={(e) => {
                  e.stopPropagation();
                  if (pauseGate.gated) return;
                  onPause(process.taskId);
                }}
                onKeyDown={(e) => e.stopPropagation()}
                disabled={isLoading}
                aria-disabled={pauseGate.gated || undefined}
                data-disabled-explained={pauseGate.gated ? "true" : undefined}
                className={`w-6 h-6 flex items-center justify-center rounded hover:bg-white/[0.08] transition-colors disabled:opacity-40${
                  pauseGate.gated ? " opacity-50" : ""
                }`}
                style={{ color: "var(--text-muted)" }}
              >
                {isLoading ? (
                  <Loader2 className="w-3 h-3 animate-spin" />
                ) : (
                  <Pause className="w-3 h-3" />
                )}
              </button>
            </TooltipTrigger>
            <TooltipContent>
              {pauseGate.gated ? pauseGate.reason : "Pause task"}
            </TooltipContent>
          </Tooltip>
          <button
            data-testid={`stop-button-${process.taskId}`}
            onClick={(e) => {
              e.stopPropagation();
              onStop(process.taskId);
            }}
            onKeyDown={(e) => e.stopPropagation()}
            disabled={isLoading}
            className="w-6 h-6 flex items-center justify-center rounded hover:bg-white/[0.08] transition-colors disabled:opacity-40"
            style={{ color: "var(--status-error)" }}
            title="Stop task"
          >
            {isLoading ? (
              <Loader2 className="w-3 h-3 animate-spin" />
            ) : (
              <Square className="w-2.5 h-2.5 fill-current" />
            )}
          </button>
        </div>
      </div>

      {/* Line 2: Step · Elapsed · Origin · Branch */}
      <div
        className="flex items-center gap-1.5 mt-0.5 pl-[22px] text-[0.6875rem] min-w-0"
        style={{ color: "var(--text-muted)" }}
      >
        {stepInfo && <span className="shrink-0">{stepInfo}</span>}
        {stepInfo && (
          <span className="shrink-0" style={{ color: "var(--text-muted)" }}>
            ·
          </span>
        )}
        <span className="shrink-0 tabular-nums">
          {formatElapsedTime(elapsedTime)}
        </span>
        {originStyle && (
          <>
            <span className="shrink-0" style={{ color: "var(--text-muted)" }}>
              ·
            </span>
            <span
              className="text-[0.625rem] font-medium px-1 rounded shrink-0"
              style={{
                color: originStyle.color,
                backgroundColor: originStyle.bgColor,
              }}
            >
              {originStyle.label}
            </span>
          </>
        )}
        {process.taskBranch && (
          <>
            <span className="shrink-0" style={{ color: "var(--text-muted)" }}>
              ·
            </span>
            <span
              className="font-mono text-[0.625rem] truncate min-w-0"
              style={{ color: "var(--text-muted)" }}
              title={process.taskBranch}
            >
              {process.taskBranch}
            </span>
          </>
        )}
      </div>
    </div>
  );
}
