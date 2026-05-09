/**
 * TeamOverviewHeader — Compact horizontal stats bar
 *
 * Shows teammate avatar dots, active count, task count, and total cost.
 * Height: 36px max. Sits at the top of the coordinator pane.
 */

import React, { useMemo } from "react";
import { useTeamStore, selectActiveTeam, selectTeammates } from "@/stores/teamStore";
import type { TeammateState } from "@/stores/teamStore";

interface TeamOverviewHeaderProps {
  contextKey: string;
}

function formatCost(usd: number): string {
  return usd < 0.01 ? "<$0.01" : `$${usd.toFixed(2)}`;
}

export const TeamOverviewHeader = React.memo(function TeamOverviewHeader({
  contextKey,
}: TeamOverviewHeaderProps) {
  const teamSelector = useMemo(() => selectActiveTeam(contextKey), [contextKey]);
  const team = useTeamStore(teamSelector);
  const teammatesSelector = useMemo(() => selectTeammates(contextKey), [contextKey]);
  const teammates = useTeamStore(teammatesSelector);

  const activeCount = useMemo(
    () => teammates.filter((m: TeammateState) => m.status !== "shutdown" && m.status !== "completed").length,
    [teammates],
  );

  if (!team) return null;

  return (
    <div
      className="flex items-center gap-3 px-3 shrink-0"
      style={{
        height: 36,
        backgroundColor: "var(--bg-surface)",
        borderBottom: "1px solid var(--border-subtle)",
      }}
    >
      {/* Teammate avatar dots */}
      <div className="flex items-center gap-1">
        {teammates.map((mate: TeammateState) => (
          <span
            key={mate.name}
            className="w-2 h-2 rounded-full"
            title={`${mate.name} (${mate.status})`}
            style={{
              backgroundColor: mate.color,
              opacity: mate.status === "shutdown" ? 0.3 : 1,
            }}
          />
        ))}
      </div>

      {/* Active count */}
      <span className="text-[0.6875rem]" style={{ color: "var(--text-secondary)" }}>
        {activeCount} active
      </span>

      {/* Task count */}
      <span className="text-[0.6875rem]" style={{ color: "var(--text-secondary)" }}>
        {teammates.length} tasks
      </span>

      {/* Total cost */}
      <span className="text-[0.6875rem] ml-auto" style={{ color: "var(--text-muted)" }}>
        {formatCost(team.totalEstimatedCostUsd)}
      </span>
    </div>
  );
});
