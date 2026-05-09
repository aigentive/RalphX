/**
 * TeammatePaneGrid — Right column container for teammate panes
 *
 * CSS Grid layout with equal-height rows for each teammate.
 * 1px gap between panes, scrollable when many teammates.
 */

import React, { useMemo } from "react";
import { useTeamStore, selectTeammates } from "@/stores/teamStore";
import { TeammatePane } from "./TeammatePane";
import type { TeammateState } from "@/stores/teamStore";

interface TeammatePaneGridProps {
  contextKey: string;
  onStopTeammate?: ((name: string) => void) | undefined;
  onSendMessage?: ((name: string, content: string) => void) | undefined;
}

export const TeammatePaneGrid = React.memo(function TeammatePaneGrid({
  contextKey,
  onStopTeammate,
  onSendMessage,
}: TeammatePaneGridProps) {
  const teammatesSelector = useMemo(() => selectTeammates(contextKey), [contextKey]);
  const teammates = useTeamStore(teammatesSelector);

  if (teammates.length === 0) {
    return (
      <div
        className="flex items-center justify-center h-full"
        style={{ backgroundColor: "var(--bg-base)" }}
      >
        <p className="text-[0.75rem]" style={{ color: "var(--text-muted)" }}>
          No teammates spawned yet
        </p>
      </div>
    );
  }

  return (
    <div
      className="h-full overflow-y-auto"
      style={{
        display: "grid",
        gridTemplateRows: `repeat(${teammates.length}, 1fr)`,
        gap: 1,
        backgroundColor: "var(--border-subtle)",
      }}
    >
      {teammates.map((mate: TeammateState) => (
        <TeammatePane
          key={mate.name}
          contextKey={contextKey}
          teammateName={mate.name}
          onStop={onStopTeammate}
          onSendMessage={onSendMessage}
        />
      ))}
    </div>
  );
});
