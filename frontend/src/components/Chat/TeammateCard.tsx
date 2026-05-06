/**
 * TeammateCard — Individual teammate status card
 *
 * Shows: color dot + name + model badge, role description,
 * current activity, per-teammate cost, action buttons.
 */

import React from "react";
import { MessageSquare, Square } from "lucide-react";
import { Button } from "@/components/ui/button";
import type { TeammateState, TeammateStatus } from "@/stores/teamStore";

interface TeammateCardProps {
  teammate: TeammateState;
  onMessage?: ((name: string) => void) | undefined;
  onStop?: ((name: string) => void) | undefined;
}

const STATUS_CONFIG: Record<TeammateStatus, { label: string; dotColor: string }> = {
  spawning: { label: "Spawning", dotColor: "var(--text-muted)" },
  running: { label: "Running", dotColor: "var(--status-success)" },
  idle: { label: "Idle", dotColor: "var(--status-warning)" },
  completed: { label: "Done", dotColor: "var(--text-muted)" },
  failed: { label: "Failed", dotColor: "var(--status-error)" },
  shutdown: { label: "Stopped", dotColor: "var(--text-muted)" },
};

function formatCost(usd: number): string {
  return usd < 0.01 ? "<$0.01" : `$${usd.toFixed(2)}`;
}

function formatTokens(tokens: number): string {
  if (tokens < 1000) return `${tokens}`;
  return `~${Math.round(tokens / 1000)}K`;
}

export const TeammateCard = React.memo(function TeammateCard({
  teammate,
  onMessage,
  onStop,
}: TeammateCardProps) {
  const { name, color, model, roleDescription, status, currentActivity, tokensUsed, estimatedCostUsd } = teammate;
  const statusConfig = STATUS_CONFIG[status];
  const isShutdown = status === "shutdown";
  const canStop = status === "running" || status === "idle";

  return (
    <div
      className="rounded-lg px-3 py-2.5"
      style={{
        backgroundColor: isShutdown ? "var(--bg-base)" : "var(--bg-surface)",
        border: `1px solid ${isShutdown ? "var(--border-subtle)" : "var(--border-default)"}`,
        opacity: isShutdown ? 0.6 : 1,
      }}
    >
      {/* Header: color dot + name + model + status */}
      <div className="flex items-center gap-2 mb-1">
        <span
          className="w-2 h-2 rounded-full shrink-0"
          style={{ backgroundColor: color }}
        />
        <span className="text-[0.75rem] font-medium" style={{ color: "var(--text-secondary)" }}>
          {name}
        </span>
        <span
          className="text-[0.625rem] px-1.5 py-0 rounded"
          style={{
            backgroundColor: "var(--bg-elevated)",
            color: "var(--text-muted)",
          }}
        >
          {model}
        </span>
        <div className="flex items-center gap-1 ml-auto">
          <span
            className="w-1.5 h-1.5 rounded-full"
            style={{ backgroundColor: statusConfig.dotColor }}
          />
          <span className="text-[0.625rem]" style={{ color: "var(--text-muted)" }}>
            {statusConfig.label}
          </span>
        </div>
      </div>

      {/* Role description */}
      {roleDescription && (
        <p className="text-[0.6875rem] mb-1 truncate" style={{ color: "var(--text-muted)" }}>
          {roleDescription}
        </p>
      )}

      {/* Current activity */}
      {currentActivity && status !== "shutdown" && (
        <p className="text-[0.6875rem] mb-1 truncate" style={{ color: "var(--text-muted)" }}>
          {currentActivity}
        </p>
      )}

      {/* Cost */}
      <div className="flex items-center justify-between mt-1.5">
        <span className="text-[0.625rem]" style={{ color: "var(--text-muted)" }}>
          {formatTokens(tokensUsed)} tokens | {formatCost(estimatedCostUsd)}
        </span>

        {/* Actions */}
        {!isShutdown && (
          <div className="flex items-center gap-1">
            {onMessage && (
              <Button
                variant="ghost"
                size="icon-sm"
                onClick={() => onMessage(name)}
                aria-label={`Message ${name}`}
                className="h-5 w-5"
              >
                <MessageSquare className="w-3 h-3" />
              </Button>
            )}
            {canStop && onStop && (
              <Button
                variant="ghost"
                size="icon-sm"
                onClick={() => onStop(name)}
                aria-label={`Stop ${name}`}
                className="h-5 w-5"
              >
                <Square className="w-3 h-3" />
              </Button>
            )}
          </div>
        )}
      </div>
    </div>
  );
});
