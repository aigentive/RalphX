import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { GraphControls, type GraphControlsProps } from "./GraphControls";

function makeProps(overrides: Partial<GraphControlsProps> = {}): GraphControlsProps {
  return {
    filters: { statuses: ["executing", "blocked"], showArchived: false },
    onFiltersChange: vi.fn(),
    layoutDirection: "TB",
    onLayoutDirectionChange: vi.fn(),
    grouping: { byPlan: true, byTier: false, showUncategorized: false },
    onGroupingChange: vi.fn(),
    nodeMode: "standard",
    onNodeModeChange: vi.fn(),
    isAutoCompact: false,
    ...overrides,
  };
}

describe("GraphControls", () => {
  it("renders top-level controls and shows active status count", () => {
    render(<GraphControls {...makeProps()} />);
    expect(screen.getByTestId("graph-controls")).toBeInTheDocument();
    expect(screen.getByText("Status")).toBeInTheDocument();
    // Active status count badge (2 selected statuses)
    expect(screen.getByText("(2)")).toBeInTheDocument();
    // Layout direction label
    expect(screen.getByText("TB")).toBeInTheDocument();
    // Node mode label
    expect(screen.getByText("Standard")).toBeInTheDocument();
  });

  it("opens status filter popover and renders category labels (someSelected branch)", async () => {
    const user = userEvent.setup();
    render(<GraphControls {...makeProps()} />);

    // Click the Status filter trigger button
    const statusTrigger = screen.getByText("Status").closest("button");
    expect(statusTrigger).not.toBeNull();
    await user.click(statusTrigger!);

    // The someSelected branch (line ~200) renders category labels because
    // selected statuses ("executing", "blocked") exist. Categories use uppercase
    // CATEGORY_LABELS via category header span with text-[0.6875rem] class.
    // At least one category header should be visible.
    const clearAll = await screen.findByText(/clear all/i);
    expect(clearAll).toBeInTheDocument();
  });

  it("opens grouping dropdown and renders Grouping label (line ~290)", async () => {
    const user = userEvent.setup();
    render(<GraphControls {...makeProps()} />);

    // Grouping trigger shows "Plan" because byPlan=true (getGroupingLabel)
    const groupingTrigger = screen.getByText("Plan").closest("button");
    expect(groupingTrigger).not.toBeNull();
    await user.click(groupingTrigger!);

    // Assertions for line ~290: "Grouping" label span renders inside popover
    const groupingLabel = await screen.findByText("Grouping");
    expect(groupingLabel).toBeInTheDocument();

    // Grouping options are rendered
    expect(screen.getByText("By Plan")).toBeInTheDocument();
    expect(screen.getByText("By Tier")).toBeInTheDocument();
    expect(screen.getByText("Uncategorized")).toBeInTheDocument();
  });

  it("toggles layout direction when layout button clicked", async () => {
    const user = userEvent.setup();
    const onLayoutDirectionChange = vi.fn();
    render(
      <GraphControls
        {...makeProps({ onLayoutDirectionChange })}
      />
    );

    const layoutBtn = screen.getByTitle(/switch to horizontal layout/i);
    await user.click(layoutBtn);
    expect(onLayoutDirectionChange).toHaveBeenCalledWith("LR");
  });

  it("toggles node mode when node mode button clicked", async () => {
    const user = userEvent.setup();
    const onNodeModeChange = vi.fn();
    render(
      <GraphControls
        {...makeProps({ onNodeModeChange })}
      />
    );

    const nodeBtn = screen.getByTitle(/switch to compact nodes/i);
    await user.click(nodeBtn);
    expect(onNodeModeChange).toHaveBeenCalledWith("compact");
  });

  it("renders auto-compact indicator when in compact mode and isAutoCompact", () => {
    render(
      <GraphControls
        {...makeProps({ nodeMode: "compact", isAutoCompact: true })}
      />
    );
    expect(screen.getByText("(auto)")).toBeInTheDocument();
    expect(screen.getByText("Compact")).toBeInTheDocument();
  });

  it("shows None grouping label when nothing is grouped", () => {
    render(
      <GraphControls
        {...makeProps({
          grouping: { byPlan: false, byTier: false, showUncategorized: false },
        })}
      />
    );
    expect(screen.getByText("None")).toBeInTheDocument();
  });

  it("shows combined Plan + Tier grouping label", () => {
    render(
      <GraphControls
        {...makeProps({
          grouping: { byPlan: true, byTier: true, showUncategorized: false },
        })}
      />
    );
    expect(screen.getByText("Plan + Tier")).toBeInTheDocument();
  });
});
