/**
 * TeamSplitGrid — CSS Grid layout for the team split view
 *
 * Two-column grid: coordinator (left) + teammates (right).
 * Responsive: stacks vertically below 1024px (coordinator on top, teammates below).
 */

import React from "react";
import { useSplitPaneStore, selectCoordinatorWidth } from "@/stores/splitPaneStore";

interface TeamSplitGridProps {
  /** Coordinator pane content (left column) */
  coordinatorSlot?: React.ReactNode;
  /** Teammates pane content (right column) */
  teammatesSlot?: React.ReactNode;
}

export const TeamSplitGrid = React.memo(function TeamSplitGrid({
  coordinatorSlot,
  teammatesSlot,
}: TeamSplitGridProps) {
  const coordinatorWidth = useSplitPaneStore(selectCoordinatorWidth);

  return (
    <div
      className="team-split-grid flex-1 overflow-hidden"
      style={{
        display: "grid",
        gridTemplateColumns: `${coordinatorWidth}% 1fr`,
        gridTemplateRows: "1fr",
        minHeight: 0,
      }}
    >
      {/* Coordinator Pane (left / top when stacked) */}
      <div
        className="team-split-grid__coordinator overflow-hidden"
        style={{ backgroundColor: "var(--bg-base)" }}
      >
        {coordinatorSlot ?? (
          <div className="flex items-center justify-center h-full">
            <span className="text-[0.75rem]" style={{ color: "var(--text-muted)" }}>
              Coordinator
            </span>
          </div>
        )}
      </div>

      {/* Teammates Pane (right / bottom when stacked) */}
      <div
        className="team-split-grid__teammates overflow-hidden"
        style={{ backgroundColor: "var(--bg-base)" }}
      >
        {teammatesSlot ?? (
          <div className="flex items-center justify-center h-full">
            <span className="text-[0.75rem]" style={{ color: "var(--text-muted)" }}>
              Teammates
            </span>
          </div>
        )}
      </div>

      <style>{`
        /* Desktop: side-by-side with vertical separator */
        .team-split-grid__coordinator {
          border-right: 1px solid var(--border-subtle);
        }

        /* Below 1024px: stack coordinator on top, teammates below */
        @media (max-width: 1024px) {
          .team-split-grid {
            grid-template-columns: 1fr !important;
            grid-template-rows: minmax(240px, 40%) 1fr !important;
          }
          .team-split-grid__coordinator {
            border-right: none;
            border-bottom: 1px solid var(--border-subtle);
          }
        }
      `}</style>
    </div>
  );
});
