/**
 * TeamProcessGroup - Collapsible group for team-mode running processes
 *
 * Shows a team header with agent count and collapsible teammate sub-entries.
 * Each teammate shows status dot, name, step info, progress bar, and model.
 * Includes wave validation gate indicator when wave data is available.
 */

import { useState } from "react";
import { ChevronDown, ChevronRight, Pause, Square, Loader2 } from "lucide-react";
import type { RunningProcess } from "@/api/running-processes";
import { TeammateProgressBar } from "./TeammateProgressBar";
import { WaveGateIndicator } from "./WaveGateIndicator";

interface TeamProcessGroupProps {
  /** The parent process representing the team task */
  process: RunningProcess;
  /** Called when pause button clicked */
  onPause: (taskId: string) => void;
  /** Called when stop button clicked */
  onStop: (taskId: string) => void;
  /** Called when the team header row is clicked to navigate to the task */
  onNavigate?: (taskId: string) => void;
}

/** Status dot color for teammate status */
function getTeammateDotColor(status: string): string {
  switch (status.toLowerCase()) {
    case "active":
    case "executing":
    case "running":
      return "var(--status-success)";
    case "completed":
    case "done":
      return "var(--text-muted)";
    case "failed":
    case "error":
      return "var(--status-error)";
    case "idle":
    case "waiting":
      return "var(--status-warning)";
    default:
      return "var(--text-muted)";
  }
}

export function TeamProcessGroup({
  process,
  onPause,
  onStop,
  onNavigate,
}: TeamProcessGroupProps) {
  const [isExpanded, setIsExpanded] = useState(true);
  const teammates = process.teammates ?? [];
  const activeCount = teammates.filter(
    (t) => !["completed", "done"].includes(t.status.toLowerCase())
  ).length;

  const hasWaveData =
    process.currentWave !== undefined && process.totalWaves !== undefined;

  const ChevronIcon = isExpanded ? ChevronDown : ChevronRight;

  return (
    <div
      data-testid={`team-group-${process.taskId}`}
      className="rounded-md"
      style={{
        backgroundColor: "var(--overlay-faint)",
        backdropFilter: "blur(8px)",
        WebkitBackdropFilter: "blur(8px)",
      }}
    >
      {/* Team Header */}
      <div
        className="flex items-center gap-2 px-2 py-1.5 hover:bg-[var(--overlay-faint)] transition-colors rounded-t-md cursor-pointer focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-[var(--accent-border)]"
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
        <button
          data-testid={`team-toggle-${process.taskId}`}
          onClick={(e) => {
            e.stopPropagation();
            setIsExpanded(!isExpanded);
          }}
          onKeyDown={(e) => e.stopPropagation()}
          className="flex items-center justify-center w-4 h-4 shrink-0"
          style={{ color: "var(--text-muted)" }}
        >
          <ChevronIcon className="w-3 h-3" />
        </button>

        <Loader2
          className="w-3.5 h-3.5 animate-spin shrink-0"
          style={{ color: "var(--accent-primary)" }}
        />

        <span
          className="flex-1 text-xs font-medium truncate min-w-0"
          style={{ color: "var(--text-primary)" }}
          title={process.title}
        >
          {process.title}
        </span>

        <span
          className="text-[0.625rem] font-medium px-1.5 py-0.5 rounded shrink-0"
          style={{
            color: "var(--accent-primary)",
            backgroundColor: "var(--accent-muted)",
          }}
        >
          Team: {activeCount}/{teammates.length}
        </span>

        {/* Pause/Stop for the whole team task */}
        <div className="flex items-center shrink-0">
          <button
            data-testid={`pause-button-${process.taskId}`}
            onClick={(e) => {
              e.stopPropagation();
              onPause(process.taskId);
            }}
            onKeyDown={(e) => e.stopPropagation()}
            className="w-6 h-6 flex items-center justify-center rounded hover:bg-white/[0.08] transition-colors"
            style={{ color: "var(--text-muted)" }}
            title="Pause team"
          >
            <Pause className="w-3 h-3" />
          </button>
          <button
            data-testid={`stop-button-${process.taskId}`}
            onClick={(e) => {
              e.stopPropagation();
              onStop(process.taskId);
            }}
            onKeyDown={(e) => e.stopPropagation()}
            className="w-6 h-6 flex items-center justify-center rounded hover:bg-white/[0.08] transition-colors"
            style={{ color: "var(--status-error)" }}
            title="Stop team"
          >
            <Square className="w-2.5 h-2.5 fill-current" />
          </button>
        </div>
      </div>

      {/* Expanded content */}
      {isExpanded && teammates.length > 0 && (
        <div
          className="pb-1"
          style={{ borderTop: "1px solid var(--overlay-faint)" }}
        >
          {/* Wave Gate Indicator */}
          {hasWaveData && (
            <div className="px-2 pt-1.5 pb-1">
              <WaveGateIndicator
                currentWave={process.currentWave!}
                totalWaves={process.totalWaves!}
                teammates={teammates}
              />
            </div>
          )}

          {/* Teammate Sub-entries (display-only — no click handlers) */}
          {teammates.map((teammate, idx) => {
            const dotColor = teammate.color ?? getTeammateDotColor(teammate.status);
            const isDone = ["completed", "done"].includes(teammate.status.toLowerCase());
            const hasProgress =
              teammate.stepsCompleted !== undefined &&
              teammate.stepsTotal !== undefined &&
              teammate.stepsTotal > 0;

            return (
              <div
                key={teammate.name + idx}
                data-testid={`teammate-${teammate.name}`}
                className="px-2 py-1 pl-8"
                style={{ opacity: isDone ? 0.5 : 1 }}
              >
                {/* Top row: dot + name + step + model */}
                <div className="flex items-center gap-2">
                  {/* Status dot */}
                  <span
                    className="w-1.5 h-1.5 rounded-full shrink-0"
                    style={{ backgroundColor: dotColor }}
                  />

                  {/* Name */}
                  <span
                    className="text-[0.6875rem] font-medium shrink-0"
                    style={{ color: "var(--text-secondary)" }}
                  >
                    {teammate.name}
                  </span>

                  {/* Step info */}
                  {teammate.step && (
                    <>
                      <span
                        className="shrink-0 text-[0.6875rem]"
                        style={{ color: "var(--text-muted)" }}
                      >
                        ·
                      </span>
                      <span
                        className="text-[0.625rem] truncate min-w-0"
                        style={{ color: "var(--text-muted)" }}
                      >
                        {teammate.step}
                      </span>
                    </>
                  )}

                  {/* Model badge */}
                  {teammate.model && (
                    <>
                      <span className="flex-1" />
                      <span
                        className="text-[0.5625rem] font-medium px-1 rounded shrink-0"
                        style={{
                          color: "var(--text-muted)",
                          backgroundColor: "var(--overlay-weak)",
                        }}
                      >
                        {teammate.model}
                      </span>
                    </>
                  )}
                </div>

                {/* Progress bar row (only when step data available) */}
                {hasProgress && (
                  <div className="mt-0.5 pl-3.5">
                    <TeammateProgressBar
                      completed={teammate.stepsCompleted!}
                      total={teammate.stepsTotal!}
                    />
                  </div>
                )}
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
}
