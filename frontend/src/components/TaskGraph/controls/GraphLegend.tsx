/**
 * GraphLegend - Status color legend for Task Graph
 *
 * Shows status colors grouped by category in a compact horizontal layout.
 * Can be collapsed to save space.
 *
 * Per spec: Phase B.4 of Task Graph View implementation
 */

import { memo, useState } from "react";
import { ChevronDown, ChevronUp } from "lucide-react";
import {
  STATUS_LEGEND_GROUPS,
  CATEGORY_LABELS,
  getCategoryColor,
  getNodeStyle,
  type StatusCategory,
} from "../nodes/nodeStyles";

// ============================================================================
// Types
// ============================================================================

export interface GraphLegendProps {
  /** Start in collapsed state */
  defaultCollapsed?: boolean;
  /** Additional className */
  className?: string;
}

// ============================================================================
// Constants
// ============================================================================

/** Order of categories in the legend */
const CATEGORY_ORDER: StatusCategory[] = [
  "idle",
  "blocked",
  "executing",
  "qa",
  "review",
  "merge",
  "complete",
  "terminal",
];

// ============================================================================
// Sub-components
// ============================================================================

interface LegendItemProps {
  status: string;
  label: string;
}

const LegendItem = memo(function LegendItem({ status, label }: LegendItemProps) {
  const style = getNodeStyle(status);

  return (
    <div className="flex items-center gap-1.5" title={label}>
      <div
        className="w-3 h-3 rounded border-2 shrink-0"
        style={{
          borderColor: style.borderColor,
          backgroundColor: style.backgroundColor,
        }}
      />
      <span className="text-[0.625rem] text-[var(--text-secondary)] whitespace-nowrap">
        {label}
      </span>
    </div>
  );
});

interface CategoryGroupProps {
  category: StatusCategory;
}

const CategoryGroup = memo(function CategoryGroup({ category }: CategoryGroupProps) {
  const items = STATUS_LEGEND_GROUPS[category];
  const categoryColor = getCategoryColor(category);
  const categoryLabel = CATEGORY_LABELS[category];

  return (
    <div className="flex items-center gap-2">
      {/* Category label */}
      <span
        className="text-[0.625rem] font-semibold uppercase tracking-wider"
        style={{ color: categoryColor }}
      >
        {categoryLabel}
      </span>
      {/* Status items */}
      <div className="flex items-center gap-2">
        {items.map((item) => (
          <LegendItem key={item.status} status={item.status} label={item.label} />
        ))}
      </div>
    </div>
  );
});

// ============================================================================
// Main Component
// ============================================================================

function GraphLegendComponent({
  defaultCollapsed = false,
  className = "",
}: GraphLegendProps) {
  const [collapsed, setCollapsed] = useState(defaultCollapsed);

  return (
    <div
      className={`
        bg-[color-mix(in_srgb,_var(--bg-elevated)_90%,_transparent)] backdrop-blur-sm
        border border-[var(--border-default)] rounded-lg
        ${className}
      `}
      data-testid="graph-legend"
    >
      {/* Header with toggle */}
      <button
        onClick={() => setCollapsed((prev) => !prev)}
        className="
          w-full flex items-center justify-between
          px-3 py-1.5 text-xs font-medium text-[var(--text-secondary)]
          hover:text-[var(--text-primary)] transition-colors
        "
        aria-expanded={!collapsed}
        aria-controls="legend-content"
      >
        <span>Status Legend</span>
        {collapsed ? (
          <ChevronDown className="w-3.5 h-3.5" />
        ) : (
          <ChevronUp className="w-3.5 h-3.5" />
        )}
      </button>

      {/* Collapsible content */}
      {!collapsed && (
        <div
          id="legend-content"
          className="px-3 pb-2 flex flex-wrap gap-x-4 gap-y-1.5"
        >
          {CATEGORY_ORDER.map((category) => (
            <CategoryGroup key={category} category={category} />
          ))}
        </div>
      )}
    </div>
  );
}

/**
 * Memoized GraphLegend component
 */
export const GraphLegend = memo(GraphLegendComponent);
