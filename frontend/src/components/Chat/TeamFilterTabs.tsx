/**
 * TeamFilterTabs — Per-teammate message filter tabs
 *
 * Dynamic tabs from active team's member list:
 * [Lead] [coder-1] [coder-2] ...
 * Color indicator dots per teammate.
 */

import React from "react";
import { cn } from "@/lib/utils";
import { withAlpha } from "@/lib/theme-colors";
import type { TeammateState, TeammateStatus } from "@/stores/teamStore";

export type TeamFilterValue = "lead" | string;

const STATUS_DOT_COLORS: Record<TeammateStatus, string> = {
  running: "var(--status-success)",
  spawning: "var(--status-success)",
  idle: "var(--status-warning)",
  completed: "var(--text-muted)",
  shutdown: "var(--text-muted)",
  failed: "var(--text-muted)",
};

interface TeamFilterTabsProps {
  teammates: TeammateState[];
  activeFilter: TeamFilterValue;
  onFilterChange: (filter: TeamFilterValue) => void;
}

export const TeamFilterTabs = React.memo(function TeamFilterTabs({
  teammates,
  activeFilter,
  onFilterChange,
}: TeamFilterTabsProps) {
  return (
    <div
      className="flex items-center gap-1 px-3 py-1.5 overflow-x-auto shrink-0"
      style={{
        borderTop: "1px solid var(--border-subtle)",
      }}
    >
      {/* Lead tab */}
      <FilterChip
        label="Lead"
        isActive={activeFilter === "lead"}
        onClick={() => onFilterChange("lead")}
      />
      {/* Teammate tabs */}
      {teammates.map((mate) => (
        <FilterChip
          key={mate.name}
          label={mate.name}
          color={STATUS_DOT_COLORS[mate.status]}
          isActive={activeFilter === mate.name}
          onClick={() => onFilterChange(mate.name)}
        />
      ))}
    </div>
  );
});

// ============================================================================
// FilterChip
// ============================================================================

interface FilterChipProps {
  label: string;
  color?: string;
  isActive: boolean;
  onClick: () => void;
}

function FilterChip({ label, color, isActive, onClick }: FilterChipProps) {
  return (
    <button
      type="button"
      onClick={onClick}
      className={cn(
        "flex items-center gap-1.5 px-2 py-0.5 rounded text-[0.6875rem] whitespace-nowrap transition-colors",
        "outline-none focus-visible:ring-1",
      )}
      style={{
        backgroundColor: isActive ? "var(--bg-hover)" : "transparent",
        color: isActive ? "var(--text-primary)" : "var(--text-muted)",
        ...(isActive ? { outline: `1px solid ${withAlpha("var(--status-info)", 50)}` } : {}),
      }}
    >
      {color && (
        <span
          className="w-1.5 h-1.5 rounded-full shrink-0"
          style={{ backgroundColor: color }}
        />
      )}
      {label}
    </button>
  );
}
