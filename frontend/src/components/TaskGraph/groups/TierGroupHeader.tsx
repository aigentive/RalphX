import { memo } from "react";
import { ChevronDown, ChevronRight } from "lucide-react";
import { cn } from "@/lib/utils";
import { getTierLabel } from "./tierGroupUtils";

export interface TierGroupHeaderProps {
  tier: number;
  taskCount: number;
  isCollapsed: boolean;
  onToggleCollapse: () => void;
}

const CollapseToggle = memo(function CollapseToggle({
  isCollapsed,
  onClick,
}: {
  isCollapsed: boolean;
  onClick: () => void;
}) {
  return (
    <button
      onClick={onClick}
      className="flex-shrink-0 p-0.5 rounded hover:bg-[var(--bg-surface)] transition-colors"
      aria-label={isCollapsed ? "Expand tier" : "Collapse tier"}
    >
      {isCollapsed ? (
        <ChevronRight className="w-3.5 h-3.5 text-[var(--text-muted)]" />
      ) : (
        <ChevronDown className="w-3.5 h-3.5 text-[var(--text-muted)]" />
      )}
    </button>
  );
});

export const TierGroupHeader = memo(function TierGroupHeader({
  tier,
  taskCount,
  isCollapsed,
  onToggleCollapse,
}: TierGroupHeaderProps) {
  const label = getTierLabel(tier);

  return (
    <div
      className={cn(
        "flex items-center gap-2 px-3 py-1.5 cursor-pointer",
        "bg-[color-mix(in_srgb,_var(--bg-elevated)_70%,_transparent)]",
        isCollapsed ? "rounded-md" : "rounded-t-md"
      )}
      onDoubleClick={(event) => {
        event.stopPropagation();
        onToggleCollapse();
      }}
    >
      <CollapseToggle isCollapsed={isCollapsed} onClick={onToggleCollapse} />
      <span className="text-[0.625rem] font-semibold uppercase tracking-wider text-[var(--accent-primary)]">
        Tier {tier}
      </span>
      <span className="text-[0.6875rem] text-[var(--text-muted)] truncate">{label}</span>
      <span className="ml-auto text-[0.6875rem] text-[var(--text-muted)]">
        {taskCount} tasks
      </span>
    </div>
  );
});
