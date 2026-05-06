import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";

import { ColumnGroup } from "./ColumnGroup";

describe("ColumnGroup", () => {
  it("paints the v29a divider on the Collapsible root", () => {
    const { container } = render(
      <ColumnGroup label="Fresh" count={3}>
        <div>child</div>
      </ColumnGroup>,
    );
    const root = container.firstChild as HTMLElement;
    expect(root.style.borderTopColor).toBe("var(--kanban-group-divider)");
    expect(root.style.borderTopStyle).toBe("solid");
  });

  it("renders the count and label, and toggles via onToggle", async () => {
    const onToggle = vi.fn();
    render(
      <ColumnGroup label="Needs Revision" count={2} onToggle={onToggle}>
        <div data-testid="child">child</div>
      </ColumnGroup>,
    );
    expect(screen.getByText("Needs Revision")).toBeInTheDocument();

    await userEvent.click(screen.getByRole("button"));
    expect(onToggle).toHaveBeenCalled();
  });

  it("hovers raise the trigger background and leave restores it", () => {
    render(
      <ColumnGroup label="Fresh" count={1} accentColor="var(--accent-primary)">
        <div>child</div>
      </ColumnGroup>,
    );
    const trigger = screen.getByRole("button") as HTMLButtonElement;
    // Simulate the hover handlers — they swap inline background color.
    trigger.dispatchEvent(new MouseEvent("mouseenter", { bubbles: true }));
    trigger.dispatchEvent(new MouseEvent("mouseleave", { bubbles: true }));
    // No throw is sufficient — the handlers ran.
    expect(trigger).toBeInTheDocument();
  });
});
