/**
 * TeamSplitHeader — Header bar for the team split view
 *
 * Shows back button, team name, active count badge, aggregate cost, and stop all button.
 */

import React, { useMemo } from "react";
import { ChevronLeft, Square } from "lucide-react";
import { Button } from "@/components/ui/button";
import { useTeamStore, selectActiveTeam, selectTeammates } from "@/stores/teamStore";
import { useUiStore } from "@/stores/uiStore";
import type { TeammateState } from "@/stores/teamStore";

interface TeamSplitHeaderProps {
  contextKey: string;
  onStopAll?: (() => void) | undefined;
}

export const TeamSplitHeader = React.memo(function TeamSplitHeader({
  contextKey,
  onStopAll,
}: TeamSplitHeaderProps) {
  const teamSelector = useMemo(() => selectActiveTeam(contextKey), [contextKey]);
  const team = useTeamStore(teamSelector);
  const teammatesSelector = useMemo(() => selectTeammates(contextKey), [contextKey]);
  const teammates = useTeamStore(teammatesSelector);

  const previousView = useUiStore((s) => s.previousView);
  const setCurrentView = useUiStore((s) => s.setCurrentView);
  const setPreviousView = useUiStore((s) => s.setPreviousView);

  const activeCount = useMemo(
    () => teammates.filter((m: TeammateState) => m.status !== "shutdown").length,
    [teammates],
  );

  const handleBack = () => {
    const target = previousView ?? "kanban";
    setPreviousView(null);
    setCurrentView(target);
  };

  const costLabel = team
    ? `$${team.totalEstimatedCostUsd.toFixed(2)}`
    : "$0.00";

  return (
    <div
      className="flex items-center justify-between px-4 shrink-0"
      style={{
        height: 48,
        backgroundColor: "var(--bg-surface)",
        borderBottom: "1px solid var(--border-subtle)",
        backdropFilter: "blur(24px)",
        WebkitBackdropFilter: "blur(24px)",
      }}
    >
      {/* Left: Back + Team Name + Badge */}
      <div className="flex items-center gap-3">
        <Button
          variant="ghost"
          size="sm"
          onClick={handleBack}
          className="gap-1.5 h-7 px-2 text-[0.8125rem]"
          style={{ color: "var(--text-secondary)" }}
        >
          <ChevronLeft className="w-4 h-4" />
          Back
        </Button>

        <span
          className="text-[0.8125rem] font-medium"
          style={{ color: "var(--text-primary)" }}
        >
          {team?.teamName ?? "Team"}
        </span>

        <span
          className="text-[0.625rem] px-1.5 py-0.5 rounded"
          style={{
            backgroundColor: "var(--border-subtle)",
            color: "var(--text-muted)",
          }}
        >
          {activeCount}/{teammates.length} active
        </span>

        <span
          className="text-[0.6875rem] font-mono"
          style={{ color: "var(--text-muted)" }}
        >
          {costLabel}
        </span>
      </div>

      {/* Right: Stop All */}
      {onStopAll && activeCount > 0 && (
        <Button
          variant="ghost"
          size="sm"
          onClick={onStopAll}
          className="gap-1.5 h-7 text-[0.6875rem]"
          style={{ color: "var(--status-error)" }}
        >
          <Square className="w-3 h-3" />
          Stop All
        </Button>
      )}
    </div>
  );
});
