import { describe, it, expect, vi } from "vitest";
import { fireEvent, render, screen } from "@testing-library/react";
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
    fireEvent.mouseEnter(trigger);
    expect(trigger.style.background).toContain("overlay-faint");
    fireEvent.mouseLeave(trigger);
    expect(trigger.style.background).toBe("transparent");
  });

  it("renders the count value", () => {
    render(
      <ColumnGroup label="Fresh" count={42}>
        <div>child</div>
      </ColumnGroup>,
    );
    expect(screen.getByText("(42)")).toBeInTheDocument();
  });

  it("rotates the chevron when collapsed", () => {
    const { container } = render(
      <ColumnGroup label="Fresh" count={1} collapsed>
        <div>child</div>
      </ColumnGroup>,
    );
    const chevron = container.querySelector("svg");
    expect(chevron).not.toBeNull();
    expect(chevron?.getAttribute("class")).toContain("-rotate-90");
  });

  it("does not rotate the chevron when expanded (default)", () => {
    const { container } = render(
      <ColumnGroup label="Fresh" count={1}>
        <div>child</div>
      </ColumnGroup>,
    );
    const chevron = container.querySelector("svg");
    expect(chevron?.getAttribute("class")).not.toContain("-rotate-90");
  });

  it("renders the icon with the provided accentColor", () => {
    render(
      <ColumnGroup
        label="Fresh"
        count={1}
        icon={<span data-testid="group-icon">I</span>}
        accentColor="rgb(255, 107, 53)"
      >
        <div>child</div>
      </ColumnGroup>,
    );
    const icon = screen.getByTestId("group-icon");
    const wrapper = icon.parentElement as HTMLElement;
    expect(wrapper.style.color).toBe("rgb(255, 107, 53)");
  });

  it("falls back to text-secondary color when accentColor is omitted", () => {
    render(
      <ColumnGroup
        label="Fresh"
        count={1}
        icon={<span data-testid="group-icon">I</span>}
      >
        <div>child</div>
      </ColumnGroup>,
    );
    const icon = screen.getByTestId("group-icon");
    const wrapper = icon.parentElement as HTMLElement;
    expect(wrapper.style.color).toBe("var(--text-secondary)");
  });

  it("does not render an icon wrapper when no icon prop is provided", () => {
    render(
      <ColumnGroup label="Fresh" count={1}>
        <div data-testid="child">child</div>
      </ColumnGroup>,
    );
    // Only the chevron svg should be present, no extra icon wrapper span before label
    const button = screen.getByRole("button");
    const spans = button.querySelectorAll("span");
    // Spans inside the button: label + count = 2 (no icon wrapper span)
    expect(spans).toHaveLength(2);
  });

  it("renders children in the expanded content region", () => {
    render(
      <ColumnGroup label="Fresh" count={1}>
        <div data-testid="card-1">card 1</div>
        <div data-testid="card-2">card 2</div>
      </ColumnGroup>,
    );
    expect(screen.getByTestId("card-1")).toBeInTheDocument();
    expect(screen.getByTestId("card-2")).toBeInTheDocument();
  });

  it("renders without crashing when onToggle is omitted (no-op handler path)", async () => {
    render(
      <ColumnGroup label="Fresh" count={1}>
        <div>child</div>
      </ColumnGroup>,
    );
    // Clicking the trigger without onToggle should not throw
    const button = screen.getByRole("button");
    await userEvent.click(button);
    expect(button).toBeInTheDocument();
  });
});
