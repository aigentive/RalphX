/**
 * PaneHeader — Teammate pane header
 *
 * Shows color dot + name, model badge, status indicator,
 * role description, and hover-visible stop button.
 */

import React, { useState } from "react";
import { X } from "lucide-react";
import type { TeammateStatus } from "@/stores/teamStore";

interface PaneHeaderProps {
  name: string;
  color: string;
  model: string;
  status: TeammateStatus;
  roleDescription: string;
  onStop?: (() => void) | undefined;
}

const STATUS_DISPLAY: Record<TeammateStatus, { label: string; color: string; pulse: boolean }> = {
  spawning: { label: "spawning", color: "var(--text-muted)", pulse: false },
  running: { label: "running", color: "var(--status-success)", pulse: true },
  idle: { label: "idle", color: "var(--status-warning)", pulse: false },
  completed: { label: "done", color: "var(--text-muted)", pulse: false },
  failed: { label: "failed", color: "var(--status-error)", pulse: false },
  shutdown: { label: "stopped", color: "var(--text-muted)", pulse: false },
};

export const PaneHeader = React.memo(function PaneHeader({
  name,
  color,
  model,
  status,
  roleDescription,
  onStop,
}: PaneHeaderProps) {
  const [hovered, setHovered] = useState(false);
  const statusConfig = STATUS_DISPLAY[status];
  const canStop = status === "running" || status === "idle";

  return (
    <div
      className="flex items-center gap-2 px-2.5 py-1.5 shrink-0"
      style={{ borderBottom: "1px solid var(--border-subtle)" }}
      onMouseEnter={() => setHovered(true)}
      onMouseLeave={() => setHovered(false)}
    >
      {/* Color dot + name */}
      <span
        className="w-2.5 h-2.5 rounded-full shrink-0"
        style={{ backgroundColor: color }}
      />
      <span
        className="text-[0.8125rem] font-medium truncate"
        style={{ color: "var(--text-primary)" }}
      >
        {name}
      </span>

      {/* Model badge */}
      <span
        className="text-[0.625rem] px-1.5 py-px rounded shrink-0"
        style={{
          backgroundColor: "var(--bg-elevated)",
          color: "var(--text-muted)",
        }}
      >
        {model}
      </span>

      {/* Status */}
      <div className="flex items-center gap-1 shrink-0">
        <span
          className={`w-1.5 h-1.5 rounded-full${statusConfig.pulse ? " animate-pulse" : ""}`}
          style={{ backgroundColor: statusConfig.color }}
        />
        <span className="text-[0.625rem]" style={{ color: "var(--text-muted)" }}>
          {statusConfig.label}
        </span>
      </div>

      {/* Role (truncated) */}
      {roleDescription && (
        <span
          className="text-[0.6875rem] truncate ml-1"
          style={{ color: "var(--text-muted)", maxWidth: 120 }}
        >
          {roleDescription}
        </span>
      )}

      {/* Stop button (hover-visible) */}
      {canStop && onStop && (
        <button
          type="button"
          onClick={(e) => {
            e.stopPropagation();
            onStop();
          }}
          aria-label={`Stop ${name}`}
          className="ml-auto w-5 h-5 flex items-center justify-center rounded transition-opacity"
          style={{
            opacity: hovered ? 0.8 : 0,
            color: "var(--status-error)",
          }}
        >
          <X className="w-3 h-3" />
        </button>
      )}
    </div>
  );
});
