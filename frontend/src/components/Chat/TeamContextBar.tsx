/**
 * TeamContextBar — Contextual top bar for team mode.
 *
 * "Lead" tab: compact summary (counts + cost).
 * Teammate tab: detail row(s) with status, model, activity, stop.
 */

import React, { useMemo } from "react";
import { Square } from "lucide-react";
import { useTeamStore, selectActiveTeam, selectTeammates, selectTeammateByName } from "@/stores/teamStore";
import type { TeammateState, TeammateStatus } from "@/stores/teamStore";
import type { TeamFilterValue } from "./TeamFilterTabs";

interface TeamContextBarProps {
  contextKey: string;
  activeFilter: TeamFilterValue;
  isHistorical?: boolean;
  onStopTeammate?: (name: string) => void;
}

const STATUS_COLORS: Record<TeammateStatus, string> = {
  running: "var(--status-success)",
  idle: "var(--status-warning)",
  spawning: "var(--text-muted)",
  completed: "var(--text-muted)",
  failed: "var(--text-muted)",
  shutdown: "var(--text-muted)",
};

function formatCost(usd: number): string {
  return usd < 0.01 ? "<$0.01" : `$${usd.toFixed(2)}`;
}

function formatTokens(tokens: number): string {
  return tokens >= 1000 ? `~${Math.round(tokens / 1000)}K` : `~${tokens}`;
}

// ============================================================================
// Summary mode — "lead" tab
// ============================================================================

function TeamSummaryRow({ contextKey, isHistorical }: { contextKey: string; isHistorical: boolean }) {
  const teamSelector = useMemo(() => selectActiveTeam(contextKey), [contextKey]);
  const team = useTeamStore(teamSelector);
  const teammatesSelector = useMemo(() => selectTeammates(contextKey), [contextKey]);
  const teammates = useTeamStore(teammatesSelector);

  if (!team) return null;

  const activeCount = teammates.filter((m: TeammateState) => m.status !== "shutdown").length;
  const runningCount = teammates.filter((m: TeammateState) => m.status === "running" || m.status === "spawning").length;

  return (
    <div className="flex items-center justify-between text-[0.6875rem]" style={{ color: "var(--text-secondary)" }}>
      <div className="flex items-center gap-1.5">
        <span>{activeCount} active</span>
        <span style={{ color: "var(--text-muted)" }}>·</span>
        <span>{runningCount} running</span>
        <span style={{ color: "var(--text-muted)" }}>·</span>
        <span>{formatCost(team.totalEstimatedCostUsd)}</span>
      </div>
      {isHistorical && (
        <span className="text-[0.625rem] px-1.5 py-0.5 rounded" style={{ backgroundColor: "var(--bg-elevated)", color: "var(--text-muted)" }}>
          Session ended
        </span>
      )}
    </div>
  );
}

// ============================================================================
// Detail mode — teammate tab
// ============================================================================

function TeammateDetailRow({ contextKey, name, onStop }: { contextKey: string; name: string; onStop?: ((name: string) => void) | undefined }) {
  const selector = useMemo(() => selectTeammateByName(contextKey, name), [contextKey, name]);
  const mate = useTeamStore(selector);

  if (!mate) return null;

  const dotColor = STATUS_COLORS[mate.status];
  const statusLabel = mate.status.charAt(0).toUpperCase() + mate.status.slice(1);

  return (
    <div className="flex flex-col gap-0.5">
      {/* Row 1: status dot, name, model, status, stop */}
      <div className="flex items-center justify-between">
        <div className="flex items-center gap-2 min-w-0">
          <span className="w-1.5 h-1.5 rounded-full shrink-0" style={{ backgroundColor: dotColor }} />
          <span className="text-[0.75rem] font-medium truncate" style={{ color: "var(--text-secondary)" }}>{mate.name}</span>
          <span className="text-[0.625rem] px-1.5 py-0.5 rounded shrink-0" style={{ backgroundColor: "var(--bg-elevated)", color: "var(--text-secondary)" }}>
            {mate.model}
          </span>
          <span className="text-[0.6875rem]" style={{ color: dotColor }}>{statusLabel}</span>
        </div>
        {mate.status !== "shutdown" && onStop && (
          <button
            type="button"
            onClick={() => onStop(mate.name)}
            className="shrink-0 p-0.5 rounded hover:brightness-125 transition-[filter]"
            style={{ color: "var(--text-muted)" }}
          >
            <Square className="w-3 h-3" />
          </button>
        )}
      </div>
      {/* Row 2: activity + tokens/cost (only if activity exists) */}
      {mate.currentActivity && (
        <div className="flex items-center justify-between text-[0.6875rem] pl-3.5">
          <span className="truncate mr-2" style={{ color: "var(--text-muted)" }}>{mate.currentActivity}</span>
          <div className="flex items-center gap-1.5 shrink-0" style={{ color: "var(--text-muted)" }}>
            <span>{formatTokens(mate.tokensUsed)} tok</span>
            <span style={{ color: "var(--text-muted)" }}>·</span>
            <span>{formatCost(mate.estimatedCostUsd)}</span>
          </div>
        </div>
      )}
    </div>
  );
}

// ============================================================================
// TeamContextBar
// ============================================================================

export const TeamContextBar = React.memo(function TeamContextBar({
  contextKey,
  activeFilter,
  isHistorical = false,
  onStopTeammate,
}: TeamContextBarProps) {
  const isSummary = activeFilter === "lead";

  return (
    <div className="px-3 py-1.5 shrink-0" style={{ borderBottom: "1px solid var(--border-subtle)" }}>
      {isSummary ? (
        <TeamSummaryRow contextKey={contextKey} isHistorical={isHistorical} />
      ) : (
        <TeammateDetailRow contextKey={contextKey} name={activeFilter} onStop={isHistorical ? undefined : onStopTeammate} />
      )}
    </div>
  );
});
