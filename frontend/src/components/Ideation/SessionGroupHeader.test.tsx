import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { Zap } from "lucide-react";

import { SessionGroupHeader } from "./SessionGroupHeader";

describe("SessionGroupHeader", () => {
  it("renders chevron rotated when open and badge when count > 0", () => {
    const onToggle = vi.fn();
    render(
      <SessionGroupHeader
        icon={Zap}
        label="In Progress"
        count={3}
        isOpen={true}
        onToggle={onToggle}
      >
        <span>child</span>
      </SessionGroupHeader>,
    );
    const trigger = screen.getByTestId("session-group-trigger");
    const chevron = trigger.querySelector("svg.agents-project-chevron");
    expect(chevron?.className.baseVal ?? chevron?.className).toContain("rotate-90");
    expect(screen.getByText("3")).toBeInTheDocument();
  });

  it("calls onToggle with the inverted open state when clicked", async () => {
    const onToggle = vi.fn();
    render(
      <SessionGroupHeader
        icon={Zap}
        label="Drafts"
        count={1}
        isOpen={false}
        onToggle={onToggle}
      >
        <span>child</span>
      </SessionGroupHeader>,
    );
    await userEvent.click(screen.getByTestId("session-group-trigger"));
    expect(onToggle).toHaveBeenCalledWith(true);
  });

  it("flips aria-current=true when isActive is set", () => {
    render(
      <SessionGroupHeader
        icon={Zap}
        label="Accepted"
        count={2}
        isOpen={false}
        onToggle={vi.fn()}
        isActive
      >
        <span>child</span>
      </SessionGroupHeader>,
    );
    expect(screen.getByTestId("session-group-trigger")).toHaveAttribute("aria-current", "true");
  });

  it("hides the count badge when count === 0", () => {
    render(
      <SessionGroupHeader
        icon={Zap}
        label="Done"
        count={0}
        isOpen={true}
        onToggle={vi.fn()}
      >
        <span>child</span>
      </SessionGroupHeader>,
    );
    // No badge text when count is zero.
    expect(screen.queryByText("0")).toBeNull();
  });
});
