import { describe, it, expect, vi } from "vitest";
import { render, screen, fireEvent } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

import { TierGroupHeader } from "./TierGroupHeader";

describe("TierGroupHeader", () => {
  it("renders tier number, label, and task count", () => {
    render(
      <TierGroupHeader tier={2} taskCount={5} isCollapsed={false} onToggleCollapse={vi.fn()} />,
    );
    expect(screen.getByText(/Tier 2/i)).toBeInTheDocument();
    expect(screen.getByText(/5 tasks/i)).toBeInTheDocument();
  });

  it("toggles collapse via the CollapseToggle button click", async () => {
    const user = userEvent.setup();
    const onToggle = vi.fn();
    render(
      <TierGroupHeader tier={1} taskCount={3} isCollapsed={false} onToggleCollapse={onToggle} />,
    );
    await user.click(screen.getByLabelText("Collapse tier"));
    expect(onToggle).toHaveBeenCalledOnce();
  });

  it("shows the expand label when collapsed and toggles via double-click on the row", () => {
    const onToggle = vi.fn();
    render(
      <TierGroupHeader tier={3} taskCount={0} isCollapsed={true} onToggleCollapse={onToggle} />,
    );
    expect(screen.getByLabelText("Expand tier")).toBeInTheDocument();

    fireEvent.doubleClick(screen.getByText(/Tier 3/i));
    expect(onToggle).toHaveBeenCalled();
  });
});
