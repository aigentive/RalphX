/* eslint-disable react-refresh/only-export-components */
/**
 * GraphControls - Filter, layout direction, and grouping controls for Task Graph
 *
 * Provides controls for:
 * - Status filter (multi-select by status category)
 * - Plan filter (select specific plans)
 * - Layout direction toggle (TB ↔ LR)
 * - Grouping options (by plan, tier, status, none)
 *
 * Houses the filtering and grouping controls used by the task graph.
 */

import { memo, useState, useCallback } from "react";
import {
  Filter,
  ChevronDown,
  ArrowDownFromLine,
  ArrowRightFromLine,
  Layers,
  X,
  Maximize2,
  Minimize2,
} from "lucide-react";
import { cn } from "@/lib/utils";
import {
  Popover,
  PopoverContent,
  PopoverTrigger,
} from "@/components/ui/popover";
import { Checkbox } from "@/components/ui/checkbox";
import { Button } from "@/components/ui/button";
import {
  STATUS_LEGEND_GROUPS,
  CATEGORY_LABELS,
  getCategoryColor,
  getNodeStyle,
  type StatusCategory,
} from "../nodes/nodeStyles";
import type { InternalStatus } from "@/types/status";

// ============================================================================
// Types
// ============================================================================

export type LayoutDirection = "TB" | "LR";

export type GroupingState = {
  byPlan: boolean;
  byTier: boolean;
  showUncategorized: boolean;
};

export interface GraphFilters {
  /** Selected status values (empty = show all) */
  statuses: InternalStatus[];
  /** Whether to include archived tasks (fetched from backend) */
  showArchived: boolean;
}

export type NodeMode = "standard" | "compact";

export interface GraphControlsProps {
  /** Current filter state */
  filters: GraphFilters;
  /** Callback when filters change */
  onFiltersChange: (filters: GraphFilters) => void;
  /** Current layout direction */
  layoutDirection: LayoutDirection;
  /** Callback when layout direction changes */
  onLayoutDirectionChange: (direction: LayoutDirection) => void;
  /** Current grouping option */
  grouping: GroupingState;
  /** Callback when grouping changes */
  onGroupingChange: (grouping: GroupingState) => void;
  /** Current node display mode */
  nodeMode: NodeMode;
  /** Callback when node mode changes */
  onNodeModeChange: (mode: NodeMode) => void;
  /** Whether any group has auto-compact active (8+ tasks in group) */
  isAutoCompact: boolean;
  /** Additional className for the container */
  className?: string;
}

// ============================================================================
// Constants
// ============================================================================

const STATUS_CATEGORIES: StatusCategory[] = [
  "idle",
  "blocked",
  "executing",
  "qa",
  "review",
  "merge",
  "complete",
  "terminal",
];

const GROUPING_OPTIONS = [
  { key: "byPlan", label: "By Plan", description: "Group tasks by originating plan" },
  { key: "byTier", label: "By Tier", description: "Group by dependency tier" },
  { key: "showUncategorized", label: "Uncategorized", description: "Include uncategorized tasks" },
] as const;

const getGroupingLabel = (grouping: GroupingState): string => {
  const active: string[] = [];
  if (grouping.byPlan) active.push("Plan");
  if (grouping.byTier) active.push("Tier");
  if (!grouping.byPlan && !grouping.byTier) return "None";
  return active.join(" + ");
};

// ============================================================================
// Sub-components
// ============================================================================

interface StatusFilterContentProps {
  filters: GraphFilters;
  onFiltersChange: (filters: GraphFilters) => void;
}

const StatusFilterContent = memo(function StatusFilterContent({
  filters,
  onFiltersChange,
}: StatusFilterContentProps) {
  const handleStatusToggle = useCallback(
    (status: InternalStatus) => {
      const newStatuses = filters.statuses.includes(status)
        ? filters.statuses.filter((s) => s !== status)
        : [...filters.statuses, status];
      onFiltersChange({ ...filters, statuses: newStatuses });
    },
    [filters, onFiltersChange]
  );

  const handleCategoryToggle = useCallback(
    (category: StatusCategory) => {
      const categoryStatuses = STATUS_LEGEND_GROUPS[category].map((item) => item.status);
      const allSelected = categoryStatuses.every((s) => filters.statuses.includes(s));

      if (allSelected) {
        // Remove all statuses in this category
        const newStatuses = filters.statuses.filter((s) => !categoryStatuses.includes(s));
        onFiltersChange({ ...filters, statuses: newStatuses });
      } else {
        // Add all statuses in this category
        const newStatuses = [...new Set([...filters.statuses, ...categoryStatuses])];
        onFiltersChange({ ...filters, statuses: newStatuses });
      }
    },
    [filters, onFiltersChange]
  );

  const handleClearAll = useCallback(() => {
    onFiltersChange({ ...filters, statuses: [] });
  }, [filters, onFiltersChange]);

  const handleShowArchivedToggle = useCallback(() => {
    onFiltersChange({ ...filters, showArchived: !filters.showArchived });
  }, [filters, onFiltersChange]);

  return (
    <div className="space-y-3">
      {/* Clear all button */}
      {filters.statuses.length > 0 && (
        <div className="flex justify-end">
          <Button
            variant="ghost"
            size="sm"
            onClick={handleClearAll}
            className="h-6 px-2 text-xs text-[var(--text-secondary)] hover:text-[var(--text-primary)]"
          >
            <X className="w-3 h-3 mr-1" />
            Clear all
          </Button>
        </div>
      )}

      {/* Status categories */}
      {STATUS_CATEGORIES.map((category) => {
        const items = STATUS_LEGEND_GROUPS[category];
        const categoryColor = getCategoryColor(category);
        const categoryLabel = CATEGORY_LABELS[category];
        const someSelected = items.some((item) => filters.statuses.includes(item.status));

        return (
          <div key={category} className="space-y-1">
            {/* Category header with toggle */}
            <button
              onClick={() => handleCategoryToggle(category)}
              className="flex items-center gap-2 w-full hover:bg-[var(--bg-elevated)] rounded px-1 py-0.5 transition-colors"
            >
              <div
                className="w-2.5 h-2.5 rounded-sm"
                style={{ backgroundColor: categoryColor }}
              />
              <span
                className="text-[0.6875rem] font-semibold uppercase tracking-wider"
                style={{ color: categoryColor }}
              >
                {categoryLabel}
              </span>
              {someSelected && (
                <span className="ml-auto text-[0.625rem] text-[var(--text-muted)]">
                  {items.filter((item) => filters.statuses.includes(item.status)).length}/{items.length}
                </span>
              )}
            </button>

            {/* Individual status items */}
            <div className="pl-4 space-y-0.5">
              {items.map((item) => {
                const style = getNodeStyle(item.status);
                const isSelected = filters.statuses.includes(item.status);

                return (
                  <label
                    key={item.status}
                    className="flex items-center gap-2 py-0.5 cursor-pointer hover:bg-[var(--bg-surface)] rounded px-1 transition-colors"
                  >
                    <Checkbox
                      checked={isSelected}
                      onCheckedChange={() => handleStatusToggle(item.status)}
                      className="h-3.5 w-3.5"
                    />
                    <div
                      className="w-2.5 h-2.5 rounded-sm border"
                      style={{
                        borderColor: style.borderColor,
                        backgroundColor: style.backgroundColor,
                      }}
                    />
                    <span className="text-xs text-[var(--text-secondary)]">{item.label}</span>
                  </label>
                );
              })}
            </div>
          </div>
        );
      })}

      {/* Show archived toggle */}
      <div className="pt-2 border-t border-[var(--border-subtle)]">
        <label className="flex items-center gap-2 cursor-pointer">
          <Checkbox
            checked={filters.showArchived}
            onCheckedChange={handleShowArchivedToggle}
            className="h-3.5 w-3.5"
          />
          <span className="text-xs text-[var(--text-secondary)]">Show archived tasks</span>
        </label>
      </div>
    </div>
  );
});


interface GroupingDropdownProps {
  grouping: GroupingState;
  onGroupingChange: (grouping: GroupingState) => void;
}

const GroupingDropdown = memo(function GroupingDropdown({
  grouping,
  onGroupingChange,
}: GroupingDropdownProps) {
  const [open, setOpen] = useState(false);

  const currentOption = getGroupingLabel(grouping);

  return (
    <Popover open={open} onOpenChange={setOpen}>
      <PopoverTrigger asChild>
        <button
          className={cn(
            "flex items-center gap-1.5 px-2 py-1.5 rounded text-xs transition-colors",
            "bg-[var(--bg-surface)] border border-[var(--border-default)]",
            "hover:bg-[var(--bg-elevated)] hover:border-[var(--border-default)]"
          )}
        >
          <Layers className="w-3.5 h-3.5 text-[var(--text-muted)]" />
          <span className="text-[var(--text-secondary)]">{currentOption}</span>
          <ChevronDown className="w-3 h-3 text-[var(--text-muted)]" />
        </button>
      </PopoverTrigger>
      <PopoverContent
        className="w-56 p-1 bg-[var(--bg-elevated)] border-[var(--border-default)]"
        align="start"
      >
        <div className="flex items-center justify-between px-2 py-1.5">
          <span className="text-[0.625rem] text-[var(--text-muted)] uppercase tracking-wide">
            Grouping
          </span>
          <button
            onClick={() => onGroupingChange({ byPlan: false, byTier: false, showUncategorized: false })}
            className="text-[0.625rem] text-[var(--text-secondary)] hover:text-[var(--text-primary)]"
          >
            None
          </button>
        </div>
        <div className="space-y-1 px-1 pb-1">
          {GROUPING_OPTIONS.map((option) => {
            const isChecked = grouping[option.key];
            const isDisabled = option.key === "showUncategorized" && !grouping.byPlan;
            return (
              <label
                key={option.key}
                className={cn(
                  "flex items-start gap-2 px-2 py-2 rounded transition-colors cursor-pointer",
                  "hover:bg-[var(--bg-hover)]",
                  isDisabled && "opacity-50 cursor-not-allowed"
                )}
              >
                <Checkbox
                  checked={isChecked}
                  onCheckedChange={(checked) => {
                    if (isDisabled) return;
                    onGroupingChange({
                      ...grouping,
                      [option.key]: Boolean(checked),
                    });
                  }}
                />
                <div className="flex-1">
                  <div className="text-xs text-[var(--text-primary)]">{option.label}</div>
                  <div className="text-[0.625rem] text-[var(--text-muted)]">{option.description}</div>
                </div>
              </label>
            );
          })}
        </div>
      </PopoverContent>
    </Popover>
  );
});

// ============================================================================
// Main Component
// ============================================================================

function GraphControlsComponent({
  filters,
  onFiltersChange,
  layoutDirection,
  onLayoutDirectionChange,
  grouping,
  onGroupingChange,
  nodeMode,
  onNodeModeChange,
  isAutoCompact,
  className = "",
}: GraphControlsProps) {
  const [statusFilterOpen, setStatusFilterOpen] = useState(false);

  const activeStatusCount = filters.statuses.length;

  const handleLayoutToggle = useCallback(() => {
    onLayoutDirectionChange(layoutDirection === "TB" ? "LR" : "TB");
  }, [layoutDirection, onLayoutDirectionChange]);

  const handleNodeModeToggle = useCallback(() => {
    onNodeModeChange(nodeMode === "standard" ? "compact" : "standard");
  }, [nodeMode, onNodeModeChange]);

  return (
    <div
      className={cn(
        "flex items-center gap-2 px-3 py-2",
        "bg-[color-mix(in_srgb,_var(--bg-elevated)_90%,_transparent)] backdrop-blur-sm",
        "border-b border-[var(--border-default)]",
        className
      )}
      data-testid="graph-controls"
    >
      {/* Status Filter */}
      <Popover open={statusFilterOpen} onOpenChange={setStatusFilterOpen}>
        <PopoverTrigger asChild>
          <button
            className={cn(
              "flex items-center gap-1.5 px-2 py-1.5 rounded text-xs transition-colors",
              "bg-[var(--bg-surface)] border border-[var(--border-default)]",
              "hover:bg-[var(--bg-elevated)] hover:border-[var(--border-default)]",
              activeStatusCount > 0 && "border-[var(--accent-primary)]"
            )}
          >
            <Filter className="w-3.5 h-3.5 text-[var(--text-muted)]" />
            <span className="text-[var(--text-secondary)]">
              Status
              {activeStatusCount > 0 && (
                <span className="ml-1 text-[var(--accent-primary)]">({activeStatusCount})</span>
              )}
            </span>
            <ChevronDown className="w-3 h-3 text-[var(--text-muted)]" />
          </button>
        </PopoverTrigger>
        <PopoverContent
          className="w-64 p-3 bg-[var(--bg-elevated)] border-[var(--border-default)]"
          align="start"
        >
          <StatusFilterContent filters={filters} onFiltersChange={onFiltersChange} />
        </PopoverContent>
      </Popover>

      {/* Separator */}
      <div className="h-5 w-px bg-[var(--border-default)]" />

      {/* Layout Direction Toggle */}
      <button
        onClick={handleLayoutToggle}
        className={cn(
          "flex items-center gap-1.5 px-2 py-1.5 rounded text-xs transition-colors",
          "bg-[var(--bg-surface)] border border-[var(--border-default)]",
          "hover:bg-[var(--bg-elevated)] hover:border-[var(--border-default)]"
        )}
        title={layoutDirection === "TB" ? "Switch to horizontal layout" : "Switch to vertical layout"}
      >
        {layoutDirection === "TB" ? (
          <ArrowDownFromLine className="w-3.5 h-3.5 text-[var(--text-muted)]" />
        ) : (
          <ArrowRightFromLine className="w-3.5 h-3.5 text-[var(--text-muted)]" />
        )}
        <span className="text-[var(--text-secondary)]">{layoutDirection}</span>
      </button>

      {/* Node Mode Toggle */}
      <button
        onClick={handleNodeModeToggle}
        className={cn(
          "flex items-center gap-1.5 px-2 py-1.5 rounded text-xs transition-colors",
          "bg-[var(--bg-surface)] border border-[var(--border-default)]",
          "hover:bg-[var(--bg-elevated)] hover:border-[var(--border-default)]",
          isAutoCompact && nodeMode === "compact" && "border-[var(--accent-strong)]"
        )}
        title={
          nodeMode === "standard"
            ? "Switch to compact nodes"
            : isAutoCompact
              ? "Some groups auto-compacted (8+ tasks) - click to use standard nodes"
              : "Switch to standard nodes"
        }
      >
        {nodeMode === "compact" ? (
          <Minimize2 className="w-3.5 h-3.5 text-[var(--text-muted)]" />
        ) : (
          <Maximize2 className="w-3.5 h-3.5 text-[var(--text-muted)]" />
        )}
        <span className="text-[var(--text-secondary)]">
          {nodeMode === "compact" ? "Compact" : "Standard"}
        </span>
        {isAutoCompact && nodeMode === "compact" && (
          <span className="text-[0.625rem] text-[var(--accent-primary)]">(auto)</span>
        )}
      </button>

      {/* Grouping Dropdown */}
      <GroupingDropdown grouping={grouping} onGroupingChange={onGroupingChange} />
    </div>
  );
}

/**
 * Memoized GraphControls component
 */
export const GraphControls = memo(GraphControlsComponent);

// ============================================================================
// Default Values
// ============================================================================

/**
 * Default filter state (show all)
 */
export const DEFAULT_GRAPH_FILTERS: GraphFilters = {
  statuses: [],
  showArchived: false,
};

/**
 * Default layout direction
 */
export const DEFAULT_LAYOUT_DIRECTION: LayoutDirection = "TB";

/**
 * Default grouping option
 */
export const DEFAULT_GROUPING: GroupingState = {
  byPlan: true,
  byTier: true,
  showUncategorized: true,
};

/**
 * Default node display mode
 */
export const DEFAULT_NODE_MODE: NodeMode = "standard";

/**
 * Threshold for auto-switching to compact mode
 */
export const COMPACT_MODE_THRESHOLD = 8;
