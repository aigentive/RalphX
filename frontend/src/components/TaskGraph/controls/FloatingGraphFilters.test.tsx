/**
 * Render smoke tests for FloatingGraphFilters.
 *
 * Goal: exercise both branches in StatusFilterContent (someSelected = true)
 * and the GroupingDropdown (byPlan/byTier flags set), since those are the
 * gaps flagged on PR #128.
 */

import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";

import { FloatingGraphFilters } from "./FloatingGraphFilters";
import type {
  GraphFilters,
  GroupingState,
  LayoutDirection,
  NodeMode,
} from "./graph-controls";

interface RenderOpts {
  filters?: Partial<GraphFilters>;
  layoutDirection?: LayoutDirection;
  nodeMode?: NodeMode;
  isAutoCompact?: boolean;
  grouping?: Partial<GroupingState>;
  isCompact?: boolean;
}

function renderFilters(opts: RenderOpts = {}) {
  const filters: GraphFilters = {
    statuses: ["ready", "executing"],
    showArchived: false,
    ...opts.filters,
  };
  const grouping: GroupingState = {
    byPlan: true,
    byTier: true,
    showUncategorized: false,
    ...opts.grouping,
  };

  const onFiltersChange = vi.fn();
  const onLayoutDirectionChange = vi.fn();
  const onNodeModeChange = vi.fn();
  const onGroupingChange = vi.fn();

  const utils = render(
    <FloatingGraphFilters
      filters={filters}
      onFiltersChange={onFiltersChange}
      layoutDirection={opts.layoutDirection ?? "TB"}
      onLayoutDirectionChange={onLayoutDirectionChange}
      nodeMode={opts.nodeMode ?? "standard"}
      onNodeModeChange={onNodeModeChange}
      isAutoCompact={opts.isAutoCompact ?? false}
      grouping={grouping}
      onGroupingChange={onGroupingChange}
      isCompact={opts.isCompact ?? false}
    />,
  );

  return {
    ...utils,
    onFiltersChange,
    onLayoutDirectionChange,
    onNodeModeChange,
    onGroupingChange,
  };
}

describe("FloatingGraphFilters", () => {
  it("renders the floating filter container with status active count", () => {
    renderFilters();
    expect(screen.getByTestId("floating-graph-filters")).toBeInTheDocument();
    // active status badge "2" appears next to Status label
    expect(screen.getByText("Status")).toBeInTheDocument();
    expect(screen.getByText("2")).toBeInTheDocument();
  });

  it("shows the combined grouping label when byPlan and byTier are both set", () => {
    renderFilters({ grouping: { byPlan: true, byTier: true } });
    expect(screen.getByText("Plan + Tier")).toBeInTheDocument();
  });

  it("shows 'None' grouping label when neither toggle is set", () => {
    renderFilters({
      grouping: { byPlan: false, byTier: false, showUncategorized: false },
    });
    expect(screen.getByText("None")).toBeInTheDocument();
  });

  it("renders horizontal layout label when layoutDirection is LR", () => {
    renderFilters({ layoutDirection: "LR" });
    expect(screen.getByText("Horizontal")).toBeInTheDocument();
  });

  it("renders compact node mode label when nodeMode is compact", () => {
    renderFilters({ nodeMode: "compact", isAutoCompact: true });
    expect(screen.getByText("Compact")).toBeInTheDocument();
  });

  it("renders icon-only compact toolbar without text labels", () => {
    renderFilters({ isCompact: true });
    expect(screen.getByTestId("floating-graph-filters")).toBeInTheDocument();
    // labels suppressed in compact toolbar
    expect(screen.queryByText("Status")).not.toBeInTheDocument();
    expect(screen.queryByText("Vertical")).not.toBeInTheDocument();
  });

  it("opens status popover and renders Clear all + per-category counts when statuses selected", async () => {
    const user = userEvent.setup();
    renderFilters({
      filters: { statuses: ["ready", "executing"], showArchived: false },
    });

    // Click the Status filter button to open popover
    await user.click(screen.getByText("Status"));

    // someSelected branch renders count "1/N" badges and a Clear all button
    expect(await screen.findByText(/Clear all/i)).toBeInTheDocument();
  });

  it("opens grouping popover and shows grouping options", async () => {
    const user = userEvent.setup();
    renderFilters({ grouping: { byPlan: true, byTier: false } });

    // Open the grouping popover by clicking its button
    await user.click(screen.getByText("Plan"));

    expect(await screen.findByText("By Plan")).toBeInTheDocument();
    expect(screen.getByText("By Tier")).toBeInTheDocument();
    expect(screen.getByText("Uncategorized")).toBeInTheDocument();
  });
});
